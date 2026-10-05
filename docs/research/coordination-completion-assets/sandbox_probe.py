"""Scratch-only Codex sandbox probe; no inference, dependencies, or production engine."""
import base64
import hashlib
import json
import os
from pathlib import Path
import queue
import re
import signal
import subprocess
import sys
import tempfile
import threading
import time


PROBE = r'''
import errno, json, os, subprocess, sys
from pathlib import Path
root = Path(sys.argv[1])
source = root / 'source'
assert (source / 'read.txt').read_text() == 'original\n'
(root / 'scratch-control.txt').write_text('writable\n')
Path(sys.argv[2]).write_text('tmp-grant-writable\n')
operations = {
    'write': lambda: (source / 'write.txt').write_text('bypass\n'),
    'delete': lambda: (source / 'delete.txt').unlink(),
    'rename': lambda: (source / 'rename.txt').rename(source / 'renamed.txt'),
    'replace': lambda: os.replace(root / 'incoming.txt', source / 'replace.txt'),
    'symlink_create': lambda: (source / 'new-link').symlink_to(root / 'incoming.txt'),
    'symlink_write': lambda: (root / 'alias.txt').write_text('bypass\n'),
    'directory_rename': lambda: source.rename(root / 'renamed-source'),
}
results = {}
for name, operation in operations.items():
    try:
        operation()
    except OSError as error:
        results[name] = {'denied': error.errno in (errno.EACCES, errno.EPERM), 'errno': error.errno}
    else:
        results[name] = {'denied': False}
child = subprocess.run([
    '/usr/bin/python3', '-c',
    "import errno,json,sys; from pathlib import Path\n"
    "try: Path(sys.argv[1]).write_text('child-bypass\\n')\n"
    "except OSError as e: print(json.dumps({'denied':e.errno in (errno.EACCES,errno.EPERM),'errno':e.errno}))\n"
    "else: print(json.dumps({'denied':False}))",
    str(source / 'child.txt')
], start_new_session=True, capture_output=True, text=True, check=True)
results['new_session_child_write'] = json.loads(child.stdout)
print(json.dumps({'read': True, 'scratch_write': True, 'outside_tmp_write': True, 'attempts': results}))
assert all(result['denied'] for result in results.values()), results
'''


def verify(result):
    assert result['sandbox_exit'] == 0, result['sandbox_stderr']
    assert result['probe']['read'] and result['probe']['scratch_write'] and result['probe']['outside_tmp_write']
    assert all(item['denied'] for item in result['probe']['attempts'].values())
    assert result['source_unchanged']
    assert result['controller']['accepted'] and result['controller']['stale_rejected']
    assert result['controller']['client_id'] == 'client-a'


def sandbox_command(root, source, argv):
    profile = ('permissions.falinks-boundary-trial={extends=":workspace",'
               'filesystem={"' + str(source) + '"="read"},network={enabled=false}}')
    return [
        '/opt/homebrew/Caskroom/codex/0.160.0/bin/codex', 'sandbox',
        '-P', 'falinks-boundary-trial', '-C', str(root), '-c', profile, '--'
    ] + argv


def verify_worker(result):
    if 'failure' in result:
        assert not result['observed'] and not result['responses']
        assert result['worker_exit'] != 0
        if 'post_exit_source_unchanged' in result:
            assert result['post_exit_source_unchanged']
        if any(arg.startswith('sqlite_home=') for arg in result['command']):
            assert 'Error: Operation not permitted (os error 1)' in result['stderr']
        else:
            assert 'failed to initialize sqlite state runtime under' in result['stderr']
        return False
    assert result['observed']['initialized']
    assert result['observed']['read'] and result['observed']['write_denied']
    assert result['observed']['scratch_write'] and result['observed']['source_unchanged']
    return True


def worker(output, scratch_state=True, log_denials=False):
    root = Path(tempfile.mkdtemp(prefix='falinks-worker-boundary-', dir='/private/tmp'))
    source = root / 'source'
    source.mkdir()
    target = source / 'source.txt'
    target.write_bytes(b'original\n')
    binary = '/opt/homebrew/Caskroom/codex/0.160.0/bin/codex'
    worker_argv = [binary, 'app-server', '--stdio', '-c', 'analytics.enabled=false']
    if scratch_state:
        for key, name in [('sqlite_home', 'worker-state'), ('log_dir', 'worker-logs')]:
            path = root / name
            path.mkdir()
            worker_argv.extend(['-c', key + '=' + json.dumps(str(path))])
    command = sandbox_command(root, source, worker_argv)
    if log_denials:
        command.insert(command.index('--'), '--log-denials')
    result = {'root': str(root), 'command': command, 'responses': [], 'observed': {}}
    messages = queue.Queue()
    diagnostic_lines, worker_pids = [], set()
    with output.with_suffix('.stderr.log').open('w') as errors:
        process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE if log_denials else errors,
                                   text=True, start_new_session=True)
        def track_worker():
            while process.poll() is None:
                listing = subprocess.check_output(['/bin/ps', '-axo', 'pid=,ppid=,comm='], text=True)
                rows = [line.split(None, 2) for line in listing.splitlines()]
                descendants = {process.pid}
                for _ in range(4):
                    descendants.update(int(pid) for pid, parent, name in rows if int(parent) in descendants)
                worker_pids.update(int(pid) for pid, parent, name in rows
                                   if int(pid) != process.pid and int(pid) in descendants and Path(name).name == 'codex')
                time.sleep(0.02)
        if log_denials:
            threading.Thread(target=track_worker, daemon=True).start()
            def read_diagnostics():
                diagnostic_lines.extend(process.stderr)
            diagnostic_reader = threading.Thread(target=read_diagnostics, daemon=True)
            diagnostic_reader.start()
        def read_messages():
            for line in process.stdout:
                if log_denials:
                    try:
                        json.loads(line)
                    except json.JSONDecodeError:
                        diagnostic_lines.append(line)
                        continue
                messages.put(line)
            messages.put(None)
        threading.Thread(target=read_messages, daemon=True).start()
        def request(method, params, identifier):
            process.stdin.write(json.dumps({'method': method, 'params': params, 'id': identifier}) + '\n')
            process.stdin.flush()
            deadline = time.monotonic() + 10
            while True:
                line = messages.get(timeout=max(0.01, deadline - time.monotonic()))
                if line is None:
                    raise RuntimeError('worker stdout closed')
                item = json.loads(line)
                if item.get('id') == identifier:
                    result['responses'].append({'method': method, 'response': item})
                    return item
                if time.monotonic() >= deadline:
                    raise TimeoutError(method)
        try:
            initialized = request('initialize', {'clientInfo': {'name': 'falinks_boundary_probe', 'version': '0.1'},
                                                'capabilities': {'experimentalApi': True}}, 1)
            assert 'result' in initialized, initialized
            result['observed']['initialized'] = True
            process.stdin.write(json.dumps({'method': 'initialized', 'params': {}}) + '\n')
            process.stdin.flush()
            read = request('fs/readFile', {'path': str(target)}, 2)
            result['observed']['read'] = base64.b64decode(read['result']['dataBase64']) == b'original\n'
            write = request('fs/writeFile', {'path': str(target), 'dataBase64': base64.b64encode(b'bypass\n').decode()}, 3)
            message = write.get('error', {}).get('message', '')
            result['observed']['write_denied'] = 'Operation not permitted' in message or 'Permission denied' in message
            scratch = root / 'api-control.txt'
            control = request('fs/writeFile', {'path': str(scratch), 'dataBase64': base64.b64encode(b'control\n').decode()}, 4)
            result['observed']['scratch_write'] = 'result' in control and scratch.read_bytes() == b'control\n'
            result['observed']['source_unchanged'] = target.read_bytes() == b'original\n'
            verify_worker(result)
        except Exception as error:
            result['failure'] = type(error).__name__ + ': ' + str(error)
        finally:
            process.stdin.close()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGTERM)
                process.wait(timeout=5)
            result['worker_exit'] = process.returncode
        if log_denials:
            diagnostic_reader.join(timeout=2)
            kept, redacted = [], 0
            for line in diagnostic_lines:
                related = str(root) in line or any(re.search(r'(?<!\d)' + str(pid) + r'(?!\d)', line) for pid in worker_pids)
                setup = line.startswith(('WARNING:', 'Error:')) or 'log stream' in line or 'log:' in line or 'sandbox denial' in line.lower()
                if related or setup:
                    kept.append(line)
                else:
                    redacted += 1
            errors.writelines(kept)
            if redacted:
                errors.write('[Unrelated diagnostic content redacted: ' + str(redacted) + ' lines]\n')
            result['diagnostic'] = {'wrapper_pid': process.pid, 'worker_pids': sorted(worker_pids),
                                    'retained_lines': kept, 'redacted_line_count': redacted}
    result['stderr'] = output.with_suffix('.stderr.log').read_text()
    result['post_exit_source_unchanged'] = target.read_bytes() == b'original\n'
    output.write_text(json.dumps(result, indent=2) + '\n')
    print(output)
    if 'failure' in result:
        print(result['failure'])
        print(result['stderr'])
        return 2
    print('PASS: whole worker initialized; source read; denied source fs/writeFile; scratch write')
    return 0


def main(output):
    root = Path(tempfile.mkdtemp(prefix='falinks-write-boundary-', dir='/private/tmp'))
    outside = Path(tempfile.mkdtemp(prefix='falinks-tmp-control-', dir='/private/tmp')) / 'control.txt'
    source = root / 'source'
    source.mkdir()
    original = b'original\n'
    names = ['read', 'write', 'delete', 'rename', 'replace', 'child']
    for name in names:
        (source / (name + '.txt')).write_bytes(original)
    (root / 'incoming.txt').write_text('replacement\n')
    (root / 'alias.txt').symlink_to(source / 'write.txt')
    # ponytail: one exact subtree; test the full integration boundary before generalizing.
    command = sandbox_command(root, source, ['/usr/bin/python3', '-c', PROBE, str(root), str(outside)])
    process = subprocess.run(command, capture_output=True, text=True, timeout=30)
    result = {
        'root': str(root), 'source': str(source), 'outside_tmp_control': str(outside), 'command': command,
        'version': subprocess.check_output([command[0], '--version'], text=True).strip(),
        'sandbox_exit': process.returncode,
        'sandbox_stdout': process.stdout, 'sandbox_stderr': process.stderr,
        'source_unchanged': source.is_dir() and sorted(p.name for p in source.iterdir()) == sorted(name + '.txt' for name in names)
            and all((source / (name + '.txt')).read_bytes() == original for name in names),
    }
    if process.returncode == 0:
        result['probe'] = json.loads(process.stdout)
        target = source / 'read.txt'
        expected = hashlib.sha256(original).hexdigest()
        event = {'client_id': 'client-a', 'expected': expected}
        # ponytail: single host caller; production check+write needs the engine's serialized queue.
        def edit(expected_version, content):
            if hashlib.sha256(target.read_bytes()).hexdigest() != expected_version:
                return False
            temporary = source / '.controller-edit'
            temporary.write_bytes(content)
            os.replace(temporary, target)
            return True
        event['accepted'] = edit(expected, b'controller-edit\n')
        event['new_version'] = hashlib.sha256(target.read_bytes()).hexdigest()
        event['stale_rejected'] = not edit(expected, b'stale-overwrite\n')
        assert target.read_bytes() == b'controller-edit\n'
        result['controller'] = event
    output.write_text(json.dumps(result, indent=2) + '\n')
    print(output)
    if process.returncode:
        print(process.stderr, end='')
        return 2
    verify(result)
    print('PASS: readable source; 8 denied bypasses; writable scratch; host version check')
    return 0


if __name__ == '__main__':
    if len(sys.argv) == 3 and sys.argv[1] in ('--worker', '--worker-default', '--worker-denials'):
        sys.exit(worker(Path(sys.argv[2]), scratch_state=sys.argv[1] != '--worker-default',
                        log_denials=sys.argv[1] == '--worker-denials'))
    elif len(sys.argv) == 3 and sys.argv[1] == '--verify-worker':
        succeeded = verify_worker(json.loads(Path(sys.argv[2]).read_text()))
        print('PASS: saved whole-worker enforcement' if succeeded else 'PASS: saved worker startup-limit evidence')
    elif len(sys.argv) == 3 and sys.argv[1] == '--verify':
        verify(json.loads(Path(sys.argv[2]).read_text()))
        print('PASS: saved evidence')
    elif len(sys.argv) == 2:
        sys.exit(main(Path(sys.argv[1])))
    else:
        raise SystemExit('Usage: check.py [--worker | --verify | --verify-worker] OUTPUT.json')
