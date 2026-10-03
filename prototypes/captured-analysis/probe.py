#!/usr/bin/env python3
"""Throwaway issue-18 probe. Run with --tools PATH; never applies source edits."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import queue
import shutil
import subprocess
import tempfile
import threading
import time

HERE = Path(__file__).resolve().parent
RUST = {
    'Cargo.toml': '[package]\nname = "captured_probe"\nversion = "0.1.0"\nedition = "2024"\n[features]\nalternate = []\n',
    'Cargo.lock': 'version = 4\n\n[[package]]\nname = "captured_probe"\nversion = "0.1.0"\n',
    'src/lib.rs': 'mod dep;\npub fn left() -> i32 { 1 }\npub fn right() -> i32 { dep::callee() }\n',
    'src/dep.rs': '#[cfg(not(feature = "alternate"))]\npub fn callee() -> i32 { 7 }\n#[cfg(feature = "alternate")]\npub fn callee() -> i32 { 9 }\n',
}
GO = {
    'go.mod': 'module captured.test/probe\n\ngo 1.26.0\n',
    'siblings.go': 'package probe\nfunc Left() int { return 1 }\nfunc Right() int { return Callee() }\n',
    'callee.go': '//go:build !alternate\n\npackage probe\nfunc Callee() int { return 7 }\n',
    'callee_alt.go': '//go:build alternate\n\npackage probe\nfunc Callee() int { return 9 }\n',
}


def digest(value):
    return hashlib.sha256(value).hexdigest()


def encoded(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':')).encode()


def run(args, env, cwd=None):
    return subprocess.check_output(args, env=env, cwd=cwd, text=True, timeout=90).strip()


def snapshot(root, lang, stage, sources, config, versions):
    path = root / lang / stage
    for name, source in sources.items():
        target = path / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(source)
        # Cargo copies lockfile permissions to its own temporary metadata workspace.
        target.chmod(0o644 if name == 'Cargo.lock' and config['lock_writable'] else 0o444)
    basis = {'sources': sources, 'config': config, 'versions': versions}
    return {'id': digest(encoded(basis)), 'path': str(path), 'config': config,
            'stage': stage, 'files': {k: digest(v.encode()) for k, v in sources.items()}, 'sources': sources}


def unchanged(capture):
    path = Path(capture['path'])
    files = {str(p.relative_to(path)): digest(p.read_bytes()) for p in path.rglob('*') if p.is_file()}
    assert files == capture['files'], 'Analyzer mutated captured source/membership'


def accept(request_capture, current_capture, canceled=False):
    # ponytail: whole-capture equality only; narrower invalidation needs separate evidence.
    return not canceled and request_capture == current_capture


class LSP:
    def __init__(self, binary, env, capture, log):
        self.capture, self.log = capture, log
        self.events, self.saved, self.notifications = queue.Queue(), {}, []
        self.serial = 0
        self.stderr = tempfile.TemporaryFile()
        self.proc = subprocess.Popen([binary], env=env, cwd=capture['path'], stdin=subprocess.PIPE,
                                     stdout=subprocess.PIPE, stderr=self.stderr)
        threading.Thread(target=self.read, daemon=True).start()

    def read(self):
        try:
            while True:
                headers = {}
                while True:
                    line = self.proc.stdout.readline()
                    if not line: return
                    if line == b'\r\n': break
                    k, v = line.decode().split(':', 1)
                    headers[k.lower()] = v.strip()
                length = int(headers['content-length'])
                data = b''
                while len(data) < length:
                    chunk = self.proc.stdout.read(length - len(data))
                    if not chunk: return
                    data += chunk
                self.events.put(json.loads(data))
        except Exception as e:
            self.events.put({'reader_error': str(e)})

    def send(self, method, params, request=False):
        msg = {'jsonrpc': '2.0', 'method': method, 'params': params}
        if request:
            self.serial += 1
            msg['id'] = self.serial
            self.log.append({'id': self.serial, 'method': method, 'params': params,
                             'capture': self.capture['id']})
        self.write(msg)
        return msg.get('id')

    def write(self, msg):
        body = encoded(msg)
        self.proc.stdin.write(('Content-Length: %d\r\n\r\n' % len(body)).encode() + body)
        self.proc.stdin.flush()

    def receive(self, request_id):
        deadline = time.monotonic() + 30
        while request_id not in self.saved:
            msg = self.events.get(timeout=max(.01, deadline - time.monotonic()))
            if 'method' in msg:
                self.notifications.append(msg)
                if 'id' in msg:
                    self.write({'jsonrpc': '2.0', 'id': msg['id'], 'result': None})
            elif 'id' in msg:
                self.saved[msg['id']] = msg
            else:
                raise RuntimeError(msg)
        result = self.saved.pop(request_id)
        return result

    def query(self, method, params):
        return self.receive(self.send(method, params, True))

    def close(self):
        try:
            self.query('shutdown', None)
            self.send('exit', None)
            self.proc.wait(timeout=5)
        finally:
            if self.proc.poll() is None: self.proc.kill(); self.proc.wait()
            self.stderr.seek(0)
            self.log.append({'stderr': self.stderr.read().decode(errors='replace')[-3000:]})
            self.stderr.close()


def position(text, needle):
    offset = text.index(needle)
    prefix = text[:offset]
    # Fixtures are ASCII; LSP UTF-16 positions equal character counts here only.
    return {'line': prefix.count('\n'), 'character': len(prefix.rsplit('\n', 1)[-1])}


def rust_owner(owners, location):
    name = 'src/' + location['uri'].rsplit('/', 1)[-1]
    point = tuple(location['range']['start'][k] for k in ['line','character'])
    matches = []
    def visit(symbols, parents):
        for symbol in symbols:
            start, end = [tuple(symbol['range'][p][k] for k in ['line','character']) for p in ['start','end']]
            if start <= point < end:
                if symbol['kind'] in [6,12]: matches.append((len(parents),symbol,parents))
                visit(symbol.get('children',[]),parents+[symbol['name']])
    visit(owners[name].get('result') or [],[])
    if not matches: return None
    _, symbol, parents = max(matches,key=lambda item:item[0])
    module = [] if name=='src/lib.rs' else [Path(name).stem]
    return {'key': '::'.join(['captured_probe']+module+parents+[symbol['name']]),
            'file': name, 'range': symbol['range'], 'selectionRange': symbol['selectionRange']}


def rust_probe(capture, env, tools, shifted_id, config_id):
    trace = []
    client = LSP(str(tools / 'rust-analyzer'), env, capture, trace)
    root = Path(capture['path'])
    uri = (root / 'src/lib.rs').as_uri()
    params = {'textDocument': {'uri': uri}, 'position': position(capture['sources']['src/lib.rs'], 'callee()')}
    try:
        init = client.query('initialize', {'processId': os.getpid(), 'rootUri': root.as_uri(),
            'workspaceFolders': [{'uri': root.as_uri(), 'name': 'capture'}],
            'capabilities': {'textDocument': {'documentSymbol': {'hierarchicalDocumentSymbolSupport': True}},
                             'general': {'positionEncodings': ['utf-16']},
                             'experimental': {'serverStatusNotification': True}},
            'initializationOptions': {'cargo': {'features': capture['config']['features'],
                'target': 'aarch64-apple-darwin', 'allTargets': False, 'buildScripts': {'enable': False}},
                'cfg': {'setTest': False}, 'procMacro': {'enable': False}, 'checkOnSave': False}})['result']
        client.send('initialized', {})
        for name, text in capture['sources'].items():
            if name.endswith('.rs'):
                client.send('textDocument/didOpen', {'textDocument': {'uri': (root/name).as_uri(),
                    'languageId': 'rust', 'version': 1, 'text': text}})
        deadline = time.monotonic() + 25
        while True:
            definition = client.query('textDocument/definition', params)
            statuses = [n['params'] for n in client.notifications if n['method']=='experimental/serverStatus']
            if (statuses and statuses[-1].get('quiescent')) or time.monotonic() > deadline: break
            time.sleep(.1)
        owners = {}
        for name in ['src/lib.rs', 'src/dep.rs']:
            owners[name] = client.query('textDocument/documentSymbol', {'textDocument': {'uri': (root/name).as_uri()}})
        refs = client.query('textDocument/references', {**params, 'context': {'includeDeclaration': False}})
        forward = [{'owner': rust_owner(owners,{'uri':uri,'range':{'start':params['position']}}),
                    'target': rust_owner(owners,loc)} for loc in definition.get('result') or []]
        reverse = [rust_owner(owners,loc) for loc in refs.get('result') or []]
        calls = {}
        if init['capabilities'].get('callHierarchyProvider'):
            prep = client.query('textDocument/prepareCallHierarchy', {'textDocument': {'uri': uri},
                'position': position(capture['sources']['src/lib.rs'], 'right()')})
            calls['prepare'] = prep
            if prep.get('result'):
                calls['outgoing'] = client.query('callHierarchy/outgoingCalls', {'item': prep['result'][0]})
        races = []
        if capture['stage'] == 'baseline':
            for new_id, canceled in [(shifted_id, True), (config_id, False)]:
                rid = client.send('textDocument/references', {**params, 'context': {'includeDeclaration': False}}, True)
                # Advance the simulated consumer to a different immutable capture before consuming.
                if canceled: client.send('$/cancelRequest', {'id': rid})
                response = client.receive(rid)
                allowed = accept(capture['id'], new_id, canceled)
                assert not allowed
                races.append({'request': rid, 'current_capture': new_id, 'canceled': canceled,
                              'response': response, 'accepted': allowed})
        diagnostics = [n for n in client.notifications if n['method'] == 'textDocument/publishDiagnostics']
        caps = init['capabilities']
        return {'definition': definition, 'owners': owners, 'references': refs, 'calls': calls,
                'forward': forward, 'reverse': reverse,
                'server_status': statuses,
                'diagnostics': diagnostics, 'races': races, 'server': init.get('serverInfo'),
                'capabilities': {k: caps.get(k) for k in ['positionEncoding','textDocumentSync',
                    'definitionProvider','referencesProvider','documentSymbolProvider','callHierarchyProvider']},
                'trace': trace}
    finally:
        client.close()
        unchanged(capture)


def go_probe(capture, env, helper, cancel=False, current=None):
    args = [str(helper), capture['path'], capture['id'], capture['config']['tags']]
    if cancel:
        proc = subprocess.Popen(args + ['cancel'], env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                stderr=subprocess.PIPE, text=True)
        assert proc.stdout.readline().strip() == 'loading'
        stdout, stderr = proc.communicate('cancel\n', timeout=30)
        assert proc.returncode == 0, stderr
        result = json.loads(stdout)
    else:
        result = json.loads(run(args, env))
    unchanged(capture)
    result['reverse'] = {}
    for use in result['uses']:
        result['reverse'].setdefault(use['target'],[]).append(use['owner'])
    if current:
        result['accepted'] = accept(capture['id'], current, cancel)
        result['current_capture'] = current
        assert not result['accepted']
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--tools', type=Path, required=True)
    parser.add_argument('--readonly-lock', action='store_true', help='reproduce degraded Rust metadata/config control')
    args = parser.parse_args()
    tools = args.tools.resolve()
    rust_bin = tools / 'rustup/toolchains/1.99.0-aarch64-apple-darwin/bin'
    env = {'PATH': str(rust_bin) + ':' + os.environ['PATH'], 'HOME': os.environ['HOME'],
           'CARGO_HOME': str(tools/'cargo'), 'RUSTUP_HOME': str(tools/'rustup'), 'GOCACHE': str(tools/'go-cache'),
           'GOMODCACHE': str(tools/'go-mod'), 'GOENV': 'off', 'GOTOOLCHAIN': 'local',
           'GOWORK': 'off', 'GOPACKAGESDRIVER': 'off', 'CGO_ENABLED': '0',
           'GOOS': 'darwin', 'GOARCH': 'arm64', 'GOPROXY': 'off', 'CARGO_NET_OFFLINE': 'true'}
    versions = {name: run(cmd, env, HERE) for name, cmd in {
        'rustc': ['rustc', '-vV'], 'cargo': ['cargo','-V'], 'rustup': [str(tools/'cargo/bin/rustup'),'-V'],
        'rust_analyzer': [str(tools/'rust-analyzer'),'-V'],
        'go': ['go','version'], 'go_env': ['go','env','-json','GOROOT','GOTOOLDIR','GOOS','GOARCH',
                   'GOENV','GOFLAGS','GOWORK','CGO_ENABLED','GOTOOLCHAIN'],
        'modules': ['go','list','-m','-json','all']}.items()}
    versions['host'] = platform.platform()
    versions['python'] = platform.python_version()
    versions['probe_sha256'] = digest(Path(__file__).read_bytes())
    versions['module_files_sha256'] = {name: digest((HERE/name).read_bytes()) for name in ['go.mod','go.sum']}
    user_config = Path(os.environ['HOME'])/'Library/Application Support/rust-analyzer/rust-analyzer.toml'
    versions['rust_analyzer_user_config_sha256'] = digest(user_config.read_bytes()) if user_config.exists() else None
    versions['binary_sha256'] = {name: digest(Path(binary).read_bytes()) for name, binary in {
        'rustc': rust_bin/'rustc', 'cargo': rust_bin/'cargo', 'cargo_proxy': tools/'cargo/bin/cargo',
        'rust_analyzer': tools/'rust-analyzer',
        'go': Path(shutil.which('go', path=env['PATH']))}.items()}
    helper = tools / 'go-analyze'
    run(['go','build','-trimpath','-mod=readonly','-o',str(helper),'analyze.go'], env, HERE)
    versions['helper_sha256'] = digest(helper.read_bytes())
    evidence = {'versions': versions, 'environment': {k:v for k,v in env.items() if k not in ['HOME','PATH']},
                'capture_identity': 'sha256(canonical JSON of exact sources, explicit config and executed version records)',
                'independent_symbol_application': False,
                'captures': {}, 'results': {}}
    output = HERE/('readonly-lock-evidence.json' if args.readonly_lock else 'evidence.json')
    with tempfile.TemporaryDirectory(prefix='falinks-analysis-run-') as work:
        root = Path(work).resolve()
        for lang, original, sibling, old, edit in [
            ('rust', RUST, 'src/lib.rs', '1 }', '1 + }'),
            ('go', GO, 'siblings.go', 'return 1', 'return Undefined()')]:
            captures = {}
            for stage in ['baseline','shifted','unfinished','config']:
                sources = dict(original)
                if stage == 'shifted':
                    sources[sibling] = sources[sibling].replace(' { 1 }', ' {\n    // sibling edit\n    1\n}') if lang == 'rust' else sources[sibling].replace('return 1', '\n// sibling edit\nreturn 1\n')
                if stage == 'unfinished': sources[sibling] = sources[sibling].replace(old,edit)
                config = {'target': 'aarch64-apple-darwin' if lang=='rust' else 'darwin/arm64',
                          'features': ['alternate'] if stage=='config' else [],
                          'tags': 'alternate' if stage=='config' else '', 'tests': False,
                          'build_scripts': False, 'proc_macros': False, 'check_on_save': False,
                          'lock_writable': not args.readonly_lock, 'cargo_all_targets': False,
                          'go_load_mode': 'LoadAllSyntax|NeedModule', 'go_pattern': './...'}
                captures[stage] = snapshot(root,lang,stage,sources,config,versions)
            evidence['captures'][lang] = captures
            results = {}
            for stage, capture in captures.items():
                print(lang, stage, flush=True)
                if lang == 'rust':
                    env['CARGO_TARGET_DIR'] = str(root/('target-'+stage))
                    result = rust_probe(capture,env,tools,captures['shifted']['id'],captures['config']['id'])
                    cmd = ['cargo','check','--offline','--locked','--message-format=json']
                    if stage == 'config': cmd += ['--features','alternate']
                    check = subprocess.run(cmd,env=env,cwd=capture['path'],capture_output=True,text=True,timeout=60)
                    result['cargo_check'] = {'exit': check.returncode, 'messages': [json.loads(line) for line in check.stdout.splitlines() if line.startswith('{')], 'stderr': check.stderr}
                    unchanged(capture)
                else:
                    result = go_probe(capture,env,helper)
                results[stage] = result
                evidence['results'][lang] = results
                output.write_text(json.dumps(evidence,indent=2,sort_keys=True)+'\n')
            baseline, shifted, broken, configured = [results[s] for s in ['baseline','shifted','unfinished','config']]
            if lang == 'rust':
                owner = lambda r: next(x for x in r['owners']['src/lib.rs']['result'] if x['name']=='right')
                assert owner(baseline)['selectionRange']['start']['line'] < owner(shifted)['selectionRange']['start']['line']
                assert baseline['definition']['result'] and baseline['references']['result']
                assert baseline['forward'][0]['owner']['key']=='captured_probe::right'
                assert baseline['forward'][0]['owner']['key']==shifted['forward'][0]['owner']['key']
                assert baseline['forward'][0]['target']['key']=='captured_probe::dep::callee'
                assert any(o and o['key']=='captured_probe::right' for o in baseline['reverse'])
                configured['feature_effect_observed'] = baseline['forward'][0]['target']['selectionRange'] != configured['forward'][0]['target']['selectionRange']
                assert baseline['server_status'][-1]['quiescent']
                if args.readonly_lock:
                    assert baseline['server_status'][-1]['health']=='warning' and not configured['feature_effect_observed']
                else:
                    assert baseline['server_status'][-1]['health']=='ok' and configured['feature_effect_observed']
                assert baseline['cargo_check']['exit']==0 and broken['cargo_check']['exit']!=0
            else:
                owner = lambda r: next(x for x in r['owners'] if x['name']=='Right')
                assert owner(baseline)['key']==owner(shifted)['key'] and owner(baseline)['start']<owner(shifted)['start']
                assert any(u['owner'].endswith('.Right') and u['target'].endswith('.Callee') for u in baseline['uses'])
                assert baseline['reverse']['captured.test/probe.Callee']==['captured.test/probe.Right']
                assert not baseline['errors'] and broken['errors']
                assert 'callee.go' in baseline['compiled_files'] and 'callee_alt.go' in configured['compiled_files']
                results['delayed'] = go_probe(captures['baseline'],env,helper,current=captures['config']['id'])
                results['canceled'] = go_probe(captures['baseline'],env,helper,True,captures['shifted']['id'])
            assert captures['baseline']['id'] != captures['config']['id']
            assert not accept(captures['baseline']['id'],captures['shifted']['id'])
            assert not accept(captures['baseline']['id'],captures['baseline']['id'],True)
            assert accept(captures['baseline']['id'],captures['baseline']['id'])
            evidence['results'][lang] = results
    output.write_text(json.dumps(evidence,indent=2,sort_keys=True)+'\n')
    print('PASS: captured inputs unchanged, owners shifted, dependencies observed, stale/canceled evidence rejected; no write or independence guarantee.')


if __name__ == '__main__': main()
