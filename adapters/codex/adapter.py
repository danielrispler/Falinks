"""Pinned Codex transport. Engine requests contain no Codex protocol fields."""
import hashlib
import json
import os
import platform
import queue
import signal
import sqlite3
import subprocess
import tempfile
import threading
import time
import uuid
from pathlib import Path

PIN = '112fae7a5a1223e673c8a1791d32338f37df8b527ff1159bb8adac6c4dbf1b4b'
HOST_PIN = '679eedaea70529aa1cffc9bc0a0788c186412663544fa76c09d63b57f383a65a'
PROFILE = 'falinks'
DISABLED = ('apps', 'browser_use', 'browser_use_external', 'browser_use_full_cdp_access',
            'computer_use', 'image_generation', 'multi_agent', 'web_search_request', 'code_mode')
OPERATIONS = {'falinks_edit': 'edit', 'falinks_review': 'review', 'falinks_offer': 'offer'}


class SafetyError(RuntimeError):
    pass


class ControlGate:
    REQUIRED = frozenset(('schema', 'profile', 'disabled_capabilities', 'mediated_calls',
                          'source_write_denial', 'storage_protection', 'scratch_access',
                          'unknown_change', 'steering', 'completion_race', 'resume_replay'))

    def __init__(self):
        self.controls = {}
        self.failed = False

    def record(self, name, passed):
        if name not in self.REQUIRED or type(passed) is not bool:
            raise SafetyError('unknown safety control')
        self.controls[name] = passed
        self.failed |= not passed

    def require_supported(self):
        if self.failed or set(self.controls) != self.REQUIRED or not all(self.controls.values()):
            raise SafetyError('fresh safety controls required; publication/scoring disabled')


class Boundary:
    """Trusted transport identity plus durable presentation, not reconsideration."""
    def __init__(self, database, workspace, session, engine):
        self.workspace = str(Path(workspace).resolve(strict=True))
        self.session, self.engine = session, engine
        self.thread = self.agent = self.turn = None
        self.db = sqlite3.connect(database)
        self.db.execute('create table if not exists binding(thread text primary key, agent text, workspace text)')
        self.db.execute('create table if not exists context(thread text, event text, payload text, status text, primary key(thread,event))')

    def bind(self, thread, agent, resume=False):
        row = self.db.execute('select agent,workspace from binding where thread=?', (thread,)).fetchone()
        if resume and row != (agent, self.workspace):
            raise SafetyError('resume identity/workspace mismatch')
        if row and row != (agent, self.workspace):
            raise SafetyError('thread already bound')
        with self.db:
            self.db.execute('insert or ignore into binding values(?,?,?)', (thread, agent, self.workspace))
        self.thread, self.agent, self.turn = thread, agent, None

    def begin(self, turn):
        if not self.thread or self.turn or not turn:
            raise SafetyError('unbound thread or overlapping turn')
        self.turn = turn

    def end(self, turn):
        if self.turn == turn:
            self.turn = None

    def pending(self):
        rows = self.db.execute('select event,payload,status from context where thread=? and status!=? order by rowid',
                               (self.thread, 'reconsidered')).fetchall()
        return [{'event': event, 'context': json.loads(payload), 'status': status} for event, payload, status in rows]

    def enqueue(self, event, context):
        payload = json.dumps(context, sort_keys=True)
        row = self.db.execute('select payload from context where thread=? and event=?', (self.thread, event)).fetchone()
        if row and row[0] != payload:
            raise SafetyError('event identity reused with different context')
        with self.db:
            self.db.execute('insert or ignore into context values(?,?,?,?)', (self.thread, event, payload, 'pending'))

    def dispatch(self, call):
        if (not self.turn or call.get('threadId') != self.thread or call.get('turnId') != self.turn
                or not isinstance(call.get('callId'), str) or not call['callId']
                or call.get('tool') not in OPERATIONS):
            raise SafetyError('unregistered tool or inactive runtime identity')
        arguments = call.get('arguments')
        if not isinstance(arguments, dict) or set(arguments) != {'workspace', 'request'}:
            raise SafetyError('expected workspace and engine request')
        request = arguments['request']
        if (arguments['workspace'] != self.workspace or not isinstance(request, dict)
                or any(key in request for key in ('author', 'identity', 'agent', 'session', 'thread', 'turn', 'workspace'))):
            raise SafetyError('unauthorized workspace or caller-supplied identity')
        envelope = {'identity': {'agent': self.agent, 'session': self.session, 'thread': self.thread,
                                'turn': self.turn, 'workspace': self.workspace},
                    'operation': OPERATIONS[call['tool']], 'request': request}
        result = self.engine(envelope)
        # Only engine-confirmed revision-bound reconsideration clears presentation context.
        if (envelope['operation'] == 'review' and result.get('accepted') is True
                and request.get('action') in ('defer', 'keep', 'revise', 'drop')):
            event = request.get('event')
            status = 'deferred' if request.get('action') == 'defer' else 'reconsidered'
            with self.db:
                self.db.execute('update context set status=? where thread=? and event=?', (status, self.thread, event))
        return result

    def close(self):
        self.db.close()


def tool_definitions(workspace):
    return [{'name': name, 'description': 'Submit an engine ' + operation + '. Workspace: ' + str(workspace),
             'inputSchema': {'type': 'object', 'properties': {'workspace': {'type': 'string'},
                 'request': {'type': 'object'}}, 'required': ['workspace', 'request'], 'additionalProperties': False}}
            for name, operation in OPERATIONS.items()]


def configuration(root, binary):
    root = Path(root).resolve(strict=True)
    filesystem = ','.join(json.dumps(str(path)) + '=' + json.dumps(mode) for path, mode in
                          [(root, 'deny'), (root / 'source', 'read'), (root / 'scratch', 'write'),
                           (Path(binary).resolve().parent, 'read')])
    return ['-c', 'analytics.enabled=false', '-c', 'default_permissions="falinks"',
            '-c', 'permissions.falinks={extends=":workspace",filesystem={' + filesystem + '},network={enabled=false}}',
            '-c', 'sqlite_home=' + json.dumps(str(root / 'worker-state')),
            '-c', 'log_dir=' + json.dumps(str(root / 'worker-logs')),
            '-c', 'mcp_servers={}', '-c', 'web_search="disabled"',
            *[argument for feature in DISABLED for argument in ('--disable', feature)]]


def verify_binary(binary):
    binary = Path(binary).resolve(strict=True)
    if platform.system() != 'Darwin' or hashlib.sha256(binary.read_bytes()).hexdigest() != PIN:
        raise SafetyError('unsupported platform or binary SHA-256; publication/scoring disabled')
    helper = binary.with_name('codex-code-mode-host')
    if not helper.is_file() or hashlib.sha256(helper.read_bytes()).hexdigest() != HOST_PIN:
        raise SafetyError('missing or mismatched pinned code-mode host')
    version = subprocess.check_output([str(binary), '--version'], text=True, timeout=10).strip()
    if version != 'codex-cli 0.160.0':
        raise SafetyError('unsupported version')
    return str(binary)


def verify_schema(binary):
    expected = json.loads(Path(__file__).with_name('protocol-pin.json').read_text())
    with tempfile.TemporaryDirectory(prefix='falinks-schema-') as directory:
        subprocess.run([binary, 'app-server', 'generate-json-schema', '--experimental', '--out', directory],
                       check=True, capture_output=True, text=True, timeout=30)
        actual = {name: hashlib.sha256((Path(directory) / name).read_bytes()).hexdigest() for name in expected}
    if actual != expected:
        raise SafetyError('protocol schema mismatch')
    return actual


class RpcError(SafetyError):
    def __init__(self, method, error):
        super().__init__(method + ': ' + json.dumps(error))
        self.method, self.error = method, error


class Runtime:
    """One trusted app-server connection; all engine calls pass through Boundary."""
    def __init__(self, binary, root, engine, control_host=False):
        self.binary = verify_binary(binary)
        self.control_host = control_host
        self.root = Path(root).resolve(strict=True)
        for name in ('source', 'scratch', 'controller', 'snapshots', 'validation', 'worker-home'):
            path = self.root / name
            if not path.is_dir() or path.is_symlink() or path.resolve() != path:
                raise SafetyError('unsupported workspace layout: ' + name)
        self.boundary = Boundary(self.root / 'controller/context.sqlite', self.root / 'source', str(uuid.uuid4()), engine)
        self.sequence, self.responses, self.events = 0, {}, []
        self.closed = False
        self.gate = ControlGate()
        self.failure = None
        self.schema = verify_schema(self.binary)
        self.gate.record('schema', True)
        self.command = [self.binary, 'app-server', '--stdio', *configuration(self.root, self.binary)]
        self.errors = (self.root / 'controller/server.stderr').open('a')
        self.process = subprocess.Popen(self.command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                        stderr=self.errors, text=True, start_new_session=True,
                                        env=dict(os.environ, CODEX_HOME=str(self.root / 'worker-home')))
        self.messages = queue.Queue()
        threading.Thread(target=self._read, daemon=True).start()
        try:
            self.request('initialize', {'clientInfo': {'name': 'falinks_adapter', 'version': '1'},
                                        'capabilities': {'experimentalApi': True}})
            self.send({'method': 'initialized', 'params': {}})
        except Exception:
            self.close()
            raise

    def _read(self):
        try:
            for line in self.process.stdout:
                self.messages.put(json.loads(line))
        finally:
            self.messages.put(None)

    def send(self, item):
        self.process.stdin.write(json.dumps(item) + '\n')
        self.process.stdin.flush()

    def handle(self, item):
        method, params = item.get('method'), item.get('params', {})
        if not method:
            self.responses[item['id']] = item
        elif 'id' in item:
            if method == 'item/tool/call':
                try:
                    if self.gate.failed:
                        raise SafetyError('runtime safety failure; engine calls blocked')
                    if not self.control_host:
                        self.require_supported()
                    result = self.boundary.dispatch(params)
                except SafetyError as error:
                    result = {'accepted': False, 'reason': str(error)}
                self.send({'id': item['id'], 'result': {'contentItems': [{'type': 'inputText', 'text': json.dumps(result)}],
                                                       'success': bool(result.get('accepted'))}})
                self.events.append({'method': method, 'params': params, 'result': result})
            elif method in ('item/commandExecution/requestApproval', 'item/fileChange/requestApproval'):
                self.send({'id': item['id'], 'result': {'decision': 'decline'}})
            elif method == 'item/permissions/requestApproval':
                self.send({'id': item['id'], 'result': {'permissions': {}, 'scope': 'turn'}})
            else:
                self.send({'id': item['id'], 'error': {'code': -32601, 'message': 'Host capability disabled'}})
        elif method == 'turn/started' and params.get('threadId') == self.boundary.thread:
            if not self.boundary.turn:
                self.boundary.begin(params['turn']['id'])
        elif method == 'turn/completed' and params.get('threadId') == self.boundary.thread:
            self.boundary.end(params['turn']['id'])
            self.events.append(item)
        elif method == 'item/completed':
            kind = params.get('item', {}).get('type')
            if kind in ('commandExecution', 'fileChange', 'dynamicToolCall', 'agentMessage'):
                self.events.append(item)
            elif kind in ('mcpToolCall', 'webSearch', 'imageGeneration', 'collabAgentToolCall'):
                self.failure = 'disabled capability attempted: ' + kind
                self.gate.failed = True
                raise SafetyError(self.failure)

    def pump(self, timeout):
        try:
            item = self.messages.get(timeout=timeout)
        except queue.Empty as error:
            raise SafetyError('runtime response timeout') from error
        if item is None:
            self.gate.failed = True
            raise SafetyError('app-server disconnected')
        self.handle(item)

    def request(self, method, params, timeout=20):
        self.sequence += 1
        identifier = self.sequence
        self.send({'id': identifier, 'method': method, 'params': params})
        deadline = time.monotonic() + timeout
        while identifier not in self.responses:
            self.pump(max(.01, deadline - time.monotonic()))
            if time.monotonic() >= deadline:
                raise SafetyError('runtime request timed out: ' + method)
        item = self.responses.pop(identifier)
        if 'error' in item:
            raise RpcError(method, item['error'])
        return item['result']

    def register(self, agent, thread=None):
        params = {'cwd': str(self.root / 'source'), 'runtimeWorkspaceRoots': [str(self.root)],
                  'permissions': PROFILE, 'approvalPolicy': 'never'}
        if thread:
            params.update(threadId=thread, excludeTurns=True)
        else:
            params.update(dynamicTools=tool_definitions(self.root / 'source'), ephemeral=False)
        result = self.request('thread/resume' if thread else 'thread/start', params)
        profile = result.get('activePermissionProfile', {})
        if (profile.get('id') != PROFILE or result.get('approvalPolicy') != 'never'
                or result.get('cwd') != str(self.root / 'source')
                or result.get('runtimeWorkspaceRoots') != [str(self.root)]):
            raise SafetyError('effective runtime profile/workspace mismatch')
        self.boundary.bind(result['thread']['id'], agent, resume=bool(thread))
        return result

    def start(self, prompt):
        context = json.dumps(self.boundary.pending(), sort_keys=True)
        result = self.request('turn/start', {'threadId': self.boundary.thread, 'permissions': PROFILE,
            'approvalPolicy': 'never', 'input': [{'type': 'text', 'text': 'Host pending/deferred context: ' + context + '\n' + prompt}]})
        if not self.boundary.turn:
            self.boundary.begin(result['turn']['id'])
        return result['turn']['id']

    def attention(self, event, context):
        self.boundary.enqueue(event, context)
        turn = self.boundary.turn
        if not turn:
            return 'next-boundary'
        try:
            result = self.request('turn/steer', {'threadId': self.boundary.thread, 'expectedTurnId': turn,
                'input': [{'type': 'text', 'text': 'Host pending/deferred context: ' + json.dumps(self.boundary.pending())}]})
        except RpcError as error:
            # Only the pinned protocol's completion/mismatch race is safe to defer.
            message = error.error.get('message', '').lower()
            if 'no active turn' not in message and 'does not match' not in message and 'not active' not in message:
                raise
            return 'next-boundary'
        if result.get('turnId') != turn:
            raise SafetyError('steering accepted for a different turn')
        return 'steered'

    def wait(self, timeout=180):
        deadline = time.monotonic() + timeout
        while self.boundary.turn:
            if time.monotonic() >= deadline:
                self.gate.failed = True
                raise SafetyError('bounded turn deadline exceeded')
            self.pump(max(.01, deadline - time.monotonic()))
        completed = next(event['params']['turn'] for event in reversed(self.events) if event['method'] == 'turn/completed')
        if completed['status'] != 'completed':
            raise SafetyError('turn did not complete: ' + json.dumps(completed))
        return completed

    def require_supported(self):
        self.gate.require_supported()
        if self.failure or self.process.poll() is not None:
            raise SafetyError('fresh safety controls required; publication/scoring disabled')
        verify_binary(self.binary)

    def close(self):
        if self.closed:
            return
        self.closed = True
        self.gate.failed = True
        self.process.stdin.close()
        try:
            self.process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            os.killpg(self.process.pid, signal.SIGTERM)
            self.process.wait(timeout=5)
        self.process.stdout.close()
        self.errors.close()
        self.boundary.close()
