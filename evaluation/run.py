#!/usr/bin/env python3
"""Frozen fixture checks and a two-process communicating Git-worktree baseline."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import queue
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parent
FIXTURES = ('rust-errors', 'go-page', 'go-relationships')
FIXED_ENV = dict(GIT_AUTHOR_NAME='Fixture', GIT_AUTHOR_EMAIL='fixture@example.invalid',
                 GIT_COMMITTER_NAME='Fixture', GIT_COMMITTER_EMAIL='fixture@example.invalid',
                 GIT_AUTHOR_DATE='2026-10-07T00:00:00Z', GIT_COMMITTER_DATE='2026-10-07T00:00:00Z',
                 GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL='/dev/null',
                 GIT_NO_REPLACE_OBJECTS='1', GIT_ATTR_NOSYSTEM='1')


def read(path):
    return json.loads(Path(path).read_text())


def write(path, data):
    Path(path).write_text(json.dumps(data, indent=2, sort_keys=True) + '\n')


def command(argv, cwd=None, env=None, timeout=120):
    process = subprocess.Popen(argv, cwd=cwd, env=env, text=True, stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, start_new_session=True)
    try:
        output, error = process.communicate(timeout=timeout)
        return subprocess.CompletedProcess(argv, process.returncode, output, error)
    finally:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.wait()


def git(repo, *args, guard=None):
    argv = ['git', '-c', 'core.hooksPath=/dev/null', *args]
    result = command(guard(argv) if guard else argv, repo,
                     dict(PATH=os.environ['PATH'], **FIXED_ENV))
    if result.returncode:
        raise RuntimeError(result.stdout + result.stderr)
    return result.stdout.strip()


def put_files(dest, files):
    for name, text in files.items():
        path = dest / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)


def initial(dest, fixture):
    dest.mkdir()
    put_files(dest, fixture['files'])
    git(dest, 'init', '--initial-branch=main')
    git(dest, 'add', '.')
    git(dest, 'commit', '-m', 'Frozen initial task')
    return git(dest, 'rev-parse', 'HEAD')


def material_hashes():
    return {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest()
            for directory in ('fixtures', 'protected', 'instructions')
            for p in sorted((ROOT / directory).rglob('*')) if p.is_file()} | {
                name: hashlib.sha256((ROOT / name).read_bytes()).hexdigest()
                for name in ('config.json', 'run.py', 'codex_worker.py', 'result-schema.json')}


def freeze():
    with tempfile.TemporaryDirectory(prefix='falinks-freeze-') as tmp:
        commits = {name: initial(Path(tmp) / name, read(ROOT / 'fixtures' / (name + '.json')))
                   for name in FIXTURES}
    write(ROOT / 'manifest.json', dict(version=1, hashes=material_hashes(), initial_commits=commits))


def frozen():
    manifest = read(ROOT / 'manifest.json')
    if material_hashes() != manifest['hashes']:
        raise RuntimeError('Frozen materials changed: version repairs and explicitly run freeze before a new batch')
    return manifest


def protected_roots():
    common = Path(git(ROOT, 'rev-parse', '--git-common-dir'))
    if not common.is_absolute():
        common = ROOT / common
    roots = [ROOT.parent, common.resolve()]
    for line in git(ROOT, 'worktree', 'list', '--porcelain').splitlines():
        if line.startswith('worktree '):
            roots.append(Path(line[len('worktree '):]).resolve())
    return roots


def sandbox(argv, writable, denied, network=False):
    """Host launches workers/checks; absence of the supported sandbox is fatal."""
    if sys.platform != 'darwin' or not Path('/usr/bin/sandbox-exec').exists():
        raise RuntimeError('This runner requires macOS sandbox-exec; no unprotected fallback')
    quote = lambda p: json.dumps(str(Path(p).resolve()))
    profile = ['(version 1)', '(allow default)', '(deny file-write*)', '(deny network*)', '(allow file-write* (literal "/dev/null"))']
    if network:
        profile.append('(allow network-outbound)')
    profile += ['(allow file-write* (subpath %s))' % quote(p) for p in writable]
    profile += ['(deny file-read* (subpath %s)) (deny file-write* (subpath %s))' % (quote(p), quote(p))
                for p in denied]
    return ['/usr/bin/sandbox-exec', '-p', '\n'.join(profile), *argv]


def check(candidate, fixture, private, timeout=120, denied_roots=()):
    """Checks execute only in a host-owned copy, with protected tests and fixed build inputs."""
    check_deadline = time.monotonic() + timeout
    work = private / 'check'
    if work.exists():
        shutil.rmtree(work)
    work.mkdir()
    # Raw tree blobs preserve exact committed bytes; archive export attributes do not.
    capture_env = dict(PATH=os.environ['PATH'], **FIXED_ENV)
    tree = subprocess.check_output(['git', 'ls-tree', '-rz', '--full-tree', 'HEAD'],
                                   cwd=candidate, env=capture_env)
    for entry in tree.split(b'\0'):
        if not entry:
            continue
        metadata, name_bytes = entry.split(b'\t', 1)
        mode, kind, oid = metadata.split()
        name = name_bytes.decode('utf-8')
        if mode not in (b'100644', b'100755') or kind!=b'blob' or '..' in Path(name).parts or Path(name).is_absolute():
            raise RuntimeError('Candidate contains a non-regular or unsafe path')
        data = subprocess.check_output(['git', 'cat-file', 'blob', oid.decode()], cwd=candidate, env=capture_env)
        if len(data)>1_000_000:
            raise RuntimeError('Fixture file exceeds 1 MB bound')
        target = work / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
    for name in ('Cargo.toml', 'Cargo.lock', 'go.mod'):
        if name in fixture['files'] and (work / name).read_text() != fixture['files'][name]:
            raise RuntimeError('Fixture dependencies/build configuration must remain frozen')
    if (work / 'build.rs').exists() or (work / '.cargo').exists():
        raise RuntimeError('Custom build configuration is outside the fixture contract')
    source = {str(p.relative_to(work)): p.read_bytes() for p in work.rglob('*') if p.is_file()}
    for name in ('tmp', 'cache', 'home', 'bin', 'target'):
        (work / name).mkdir()
    env = {key: value for key, value in os.environ.items()
           if key in ('PATH', 'RUSTUP_HOME', 'CARGO_HOME', 'DEVELOPER_DIR', 'SDKROOT')}
    env.update(HOME=str(work / 'home'), TMPDIR=str(work / 'tmp'), GOCACHE=str(work / 'cache'),
               GOPATH=str(work / 'home'), GOPROXY='off', GOSUMDB='off', GOTOOLCHAIN='local',
               CARGO_TARGET_DIR=str(work / 'target'))
    denied = protected_roots() + [private.parent / 'agents'] + list(denied_roots)
    # Capture is the only allowed exception beneath private state: deny sibling paths individually.
    denied += [p for p in private.parent.iterdir() if p != private]
    denied += [p for p in private.iterdir() if p != work]
    visible = []
    commands = ([['cargo', 'check', '--offline', '--locked'], ['cargo', 'test', '--offline', '--locked']] if fixture['language'] == 'rust'
                else [['go', 'test', './...']])
    for argv in commands:
        result = command(sandbox(argv, [work / name for name in ('tmp', 'cache', 'home', 'target', 'bin')], denied),
                         work, env, max(0.01, check_deadline-time.monotonic()))
        visible.append(dict(command=argv, passed=result.returncode == 0, output=result.stdout + result.stderr))
    source_changed = any(not (work / name).is_file() or (work / name).is_symlink() or
                         (work / name).read_bytes() != data for name, data in source.items())
    if source_changed:
        visible.append(dict(command=['source-integrity'], passed=False, output='Check modified captured source'))
    # Independent oracle always starts from original captured bytes, even after a mutating visible check.
    shutil.rmtree(work)
    work.mkdir()
    for name, data in source.items():
        target = work / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
    for name in ('tmp', 'cache', 'home', 'bin', 'target'):
        (work / name).mkdir()
    # Agent tests are useful feedback but cannot alter or replace the independent oracle.
    for path in list(work.rglob('*_test.go')) + list((work / 'tests').rglob('*.rs')):
        path.unlink()
    protected = read(ROOT / 'protected' / (fixture['name'] + '.json'))
    if fixture['language'] == 'rust':
        (work / 'oracle.rs').write_text(protected['oracle'])
        oracle_commands = [
            ['rustc', '--edition=2021', '--crate-name', 'catalog', '--crate-type', 'lib', 'src/lib.rs', '-o', 'bin/libcatalog.rlib'],
            ['rustc', '--edition=2021', '--test', 'oracle.rs', '--extern', 'catalog=bin/libcatalog.rlib', '-o', 'bin/oracle'],
            [str(work / 'bin' / 'oracle')],
        ]
    else:
        (work / 'oracle_test.go').write_text(protected['oracle'])
        oracle_commands = [['go', 'test', '-count=1', './...']]
    results = []
    for argv in oracle_commands:
        result = command(sandbox(argv, [work / name for name in ('tmp', 'cache', 'home', 'target', 'bin')], denied),
                         work, env, max(0.01, check_deadline-time.monotonic()))
        results.append(dict(command=argv, passed=result.returncode == 0, output=result.stdout + result.stderr))
        if result.returncode:
            break
    return dict(visible=dict(passed=all(r['passed'] for r in visible), checks=visible),
                oracle=dict(passed=all(r['passed'] for r in results), checks=results),
                candidate=git(candidate, 'rev-parse', 'HEAD'))


def verify():
    manifest = frozen()
    results = {}
    with tempfile.TemporaryDirectory(prefix='falinks-verify-') as tmp:
        base = Path(tmp)
        for name in FIXTURES:
            fixture = read(ROOT / 'fixtures' / (name + '.json'))
            repo = base / name
            sha = initial(repo, fixture)
            if sha != manifest['initial_commits'][name]:
                raise RuntimeError('Initial commit is not reproducible')
            private = base / 'private'
            private.mkdir(exist_ok=True)
            first = check(repo, fixture, private)
            put_files(repo, read(ROOT / 'protected' / (name + '.json'))['reference'])
            git(repo, 'add', '.')
            git(repo, 'commit', '-m', 'Known-correct reference')
            reference = check(repo, fixture, private)
            results[name] = dict(initial=first['oracle'], reference=reference['oracle'],
                                 initial_visible=first['visible'], reference_visible=reference['visible'],
                                 initial_commit=sha, reference_commit=reference['candidate'])
    print(json.dumps(dict(fixtures=results), indent=2))
    return 0 if all(not r['initial']['passed'] and r['reference']['passed'] and
                    r['initial_visible']['passed'] and r['reference_visible']['passed']
                    for r in results.values()) else 1


def schedule():
    rows = []
    for index, name in enumerate(FIXTURES):
        for repetition in range(3):
            arms = ['git', 'falinks'] if (index + repetition) % 2 == 0 else ['falinks', 'git']
            rows.append(dict(pair=f'{name}-{repetition+1}', fixture=name, arms=arms))
    return dict(pilot=dict(fixture='go-relationships', arms=['git', 'falinks'], scored=False), pairs=rows)


def baseline(args):
    started = time.monotonic()
    config = read(args.config)
    fixture = read(ROOT / 'fixtures' / (args.fixture + '.json'))
    manifest = frozen()
    destination = Path(args.output).resolve()
    destination.mkdir()  # Never reset/reuse a run directory or previous contexts.
    private = destination / 'controller'
    private.mkdir(mode=0o700)
    public = destination / 'agents'
    public.mkdir()
    evidence = dict(schema_version=1, run_id=destination.name, arm='git', fixture=args.fixture,
                    fixture_version=fixture['version'], freeze=manifest, configuration=config,
                    order=args.order, pair=args.pair, scored=False, outcome='infrastructure_failure',
                    usage=dict(status='unavailable', reason='worker has not reported usage'),
                    usage_limit_interruptions=[], rejected_operations=0, retries=0,
                    discarded_or_rewritten_work=dict(status='unavailable'), validation_attempts=[],
                    recommendations=0, transitions=0, timing=dict(start_utc=time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),
                    endpoint='clean preparation through exact candidate passing checks'),
                    tool_differences=read(ROOT / 'config.json')['tool_differences'])
    events = private / 'events.jsonl'
    def event(kind, **values):
        row = dict(elapsed=time.monotonic()-started, kind=kind, **values)
        with events.open('a') as stream:
            stream.write(json.dumps(row)+'\n')
    processes = {}
    incoming = queue.Queue()
    shares = {'A': None, 'B': None}
    sequence = {'A': 0, 'B': 0}
    milestone_index = 0
    awaiting_ack = set()
    ready = set()
    milestone_floor = {'A': 0, 'B': 0}
    check_number = 0
    final_resolution = None
    deadline = started + config['timeout_seconds']
    try:
        for key in ('model', 'model_version', 'reasoning', 'runtime_version', 'runtime_sha256', 'worker_command', 'timeout_seconds'):
            if not config.get(key):
                raise RuntimeError('Missing frozen run configuration: '+key)
        if any(str(config[key]).startswith('REQUIRED:') for key in ('model','model_version','reasoning','runtime_version','runtime_sha256')):
            raise RuntimeError('Replace run configuration placeholders before launch')
        binary = Path(config['runtime_binary']).resolve()
        actual_hash = hashlib.sha256(binary.read_bytes()).hexdigest()
        if actual_hash != config['runtime_sha256']:
            raise RuntimeError('Runtime binary hash differs from frozen run configuration')
        evidence['runtime_verified_sha256'] = actual_hash
        if not isinstance(config['worker_command'], list) or not 0 < config['timeout_seconds'] <= 1800:
            raise RuntimeError('Worker command must be argv; maximum run duration is 1800 seconds')
        repo = public / 'repo'
        sha = initial(repo, fixture)
        if sha != manifest['initial_commits'][args.fixture]:
            raise RuntimeError('Initial commit differs from frozen fixture')
        evidence['initial_snapshot'] = sha
        for agent in ('A','B'):
            git(repo,'worktree','add','-b',agent,str(public / agent),sha)
            (public / agent / 'scratch').mkdir()
        git(repo, 'worktree', 'add', '-b', 'integration', str(public / 'integration'), sha)
        # The controller Git repository and all exact checks are outside worker access.
        retained = private / 'retained'
        initial(retained, fixture)
        denied = protected_roots() + [private] + [Path(p).resolve() for p in config.get('denied_roots', [])]
        public_guard = lambda argv: sandbox(argv, [public], denied)
        def public_git(repo, *args):
            return git(repo, *args, guard=public_guard)
        def send(agent, payload):
            processes[agent].stdin.write(json.dumps(payload)+'\n')
            processes[agent].stdin.flush()
        def drain(agent, process):
            for line in process.stdout:
                incoming.put((agent, line))
            incoming.put((agent, None))
        for agent in ('A','B'):
            env = {k:v for k,v in os.environ.items() if k in ('PATH','CARGO_HOME','RUSTUP_HOME','DEVELOPER_DIR','SDKROOT')}
            scratch = public / agent / 'scratch'
            env.update(HOME=str(scratch), TMPDIR=str(scratch), FALINKS_AGENT=agent,
                       GOCACHE=str(scratch / 'go-cache'), GOTOOLCHAIN='local', GOPROXY='off')
            worker_script = Path(config['worker_script']).resolve()
            worker_copy = scratch / 'worker.py'
            shutil.copyfile(worker_script, worker_copy)
            evidence['worker_sha256'] = hashlib.sha256(worker_script.read_bytes()).hexdigest()
            argv = [word.replace('{worker}', str(worker_copy)) for word in config['worker_command']]
            if config.get('auth_file'):
                codex_home = scratch / 'codex'
                codex_home.mkdir()
                shutil.copyfile(config['auth_file'], codex_home / 'auth.json')
                env['CODEX_HOME'] = str(codex_home)
            stderr = (private / (agent+'.stderr')).open('w')
            argv = sandbox(argv, [public / agent, repo / '.git', public / 'integration'], denied, network=True)
            process = subprocess.Popen(argv, cwd=public / agent, env=env, text=True, stdin=subprocess.PIPE,
                                       stdout=subprocess.PIPE, stderr=stderr, start_new_session=True)
            stderr.close()
            processes[agent] = process
            threading.Thread(target=drain,args=(agent,process),daemon=True).start()
            send(agent, dict(type='start', agent=agent, workspace=str(public / agent),
                             peer_workspace=str(public / ('B' if agent=='A' else 'A')),
                             integration_workspace=str(public / 'integration'),
                             task={k:v for k,v in fixture.items() if k not in ('files','milestones')},
                             development_count=len(fixture['milestones']),
                             allocation=fixture['allocation'][agent],
                             workflow=(ROOT / 'instructions' / 'git.md').read_text(),
                             runtime_binary=config.get('runtime_binary', 'codex'),
                             model={k:config[k] for k in ('model','model_version','reasoning')}))
            event('agent_started',agent=agent)
        while len(ready) < 2:
            remaining = deadline-time.monotonic()
            if remaining <= 0:
                evidence['outcome']='timeout'
                raise TimeoutError('30-minute run budget expired')
            try:
                agent,line=incoming.get(timeout=min(remaining,0.5))
            except queue.Empty:
                continue
            if line is None:
                if agent not in ready:
                    raise RuntimeError(agent+' exited before final readiness')
                continue
            request=json.loads(line)
            event('worker_request',agent=agent,request=request)
            action=request['action']
            peer='B' if agent=='A' else 'A'
            if action=='message':
                send(peer,dict(type='peer_message',sender=agent,text=request['text']))
                send(agent,dict(type='ok',action=action))
            elif action=='ack':
                expected=fixture['milestones'][milestone_index-1]['id'] if milestone_index else None
                if request.get('milestone')!=expected or agent not in awaiting_ack:
                    raise RuntimeError('Invalid milestone acknowledgment')
                awaiting_ack.remove(agent)
                milestone_floor[agent]=sequence[agent]
                send(agent,dict(type='ok',action=action))
            elif action=='share':
                if agent in ready:
                    raise RuntimeError('Cannot edit after final readiness')
                work=public / agent
                public_git(work,'add','--all','.',':(exclude)scratch')
            elif action=='usage':
                reports = evidence.setdefault('worker_usage', {})
                reports.setdefault(agent, []).append(request['usage'])
                evidence['usage'] = dict(status='available' if len(reports)==2 else 'partial',
                                         agents=reports, format='per-turn reports; not cumulative totals')
                send(agent,dict(type='ok',action=action))
            elif action=='limit':
                evidence['usage_limit_interruptions'].append(dict(agent=agent,detail=request.get('detail')))
                evidence['outcome']='usage_limit'
                raise RuntimeError('Subscription/usage interruption; batch paused, no provider switch')
            elif action=='ready':
                if milestone_index!=len(fixture['milestones']) or agent in awaiting_ack:
                    raise RuntimeError('Final readiness before developments delivered/acknowledged')
                if not shares[agent] or request.get('commit')!=shares[agent]:
                    raise RuntimeError('Final readiness must name last shared commit')
                ready.add(agent)
                send(agent,dict(type='ok',action=action))
            elif action=='check':
                # Either worker can ask for feedback on an unfinished shared combination.
                result = combine(public, private, retained, fixture, shares, request.get('resolved'),
                                 min(120,max(1,deadline-time.monotonic())), denied)
                if result['passed']:
                    final_resolution = result['candidate']
                check_number+=1
                write(private / f'check-{check_number}.json',result)
                evidence['validation_attempts'].append(dict(number=check_number,**{k:result[k] for k in ('candidate','passed','conflict')}))
                send(agent,dict(type='check_result',**{k:result[k] for k in ('candidate','passed','conflict')}))
                event('validation',agent=agent,candidate=result['candidate'],passed=result['passed'])
            else:
                raise RuntimeError('Unknown worker action: '+action)
            if action=='share':
                final_resolution = None
                work=public / agent
                diff=public_git(work,'diff','--cached','--binary')
                if not diff:
                    raise RuntimeError('Share must contain new unfinished work')
                public_git(work,'commit','-m',agent+' draft '+str(sequence[agent]+1))
                shares[agent]=public_git(work,'rev-parse','HEAD')
                sequence[agent]+=1
                patch=public_git(work,'diff','--binary',sha,shares[agent])
                event('draft_shared',agent=agent,commit=shares[agent],diff=patch)
                send(agent,dict(type='shared',commit=shares[agent]))
                send(peer,dict(type='peer_diff',sender=agent,commit=shares[agent],diff=patch))
                if not awaiting_ack and milestone_index<len(fixture['milestones']) and all(sequence[a]>milestone_floor[a] for a in ('A','B')):
                    milestone=fixture['milestones'][milestone_index]
                    milestone_index+=1
                    awaiting_ack={'A','B'}
                    ready.clear()
                    event('development',milestone=milestone)
                    for member in ('A','B'):
                        send(member,dict(type='development',**milestone))
        result=combine(public,private,retained,fixture,shares,final_resolution,min(120,max(1,deadline-time.monotonic())), denied)
        check_number+=1
        write(private / f'check-{check_number}.json',result)
        evidence['validation_attempts'].append(dict(number=check_number,**{k:result[k] for k in ('candidate','passed','conflict')}))
        evidence.update(final_snapshot=result['candidate'],oracle=result.get('oracle'),
                        outcome='success' if result['passed'] else 'task_failure')
        event('final_validation',candidate=result['candidate'],passed=result['passed'])
        evidence['timing']['elapsed_seconds'] = time.monotonic()-started
        evidence['timing']['stop_utc'] = time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime())
    except Exception as error:
        evidence['failure_reason']=str(error)
        event('failure',reason=str(error))
    finally:
        for process in processes.values():
            if process.poll() is None:
                os.killpg(process.pid,signal.SIGTERM)
        for process in processes.values():
            try:
                process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid,signal.SIGKILL)
                process.wait()
        if 'elapsed_seconds' not in evidence['timing']:
            evidence['timing'].update(elapsed_seconds=time.monotonic()-started,
                                      stop_utc=time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()))
        evidence['timing']['cleanup_elapsed_seconds'] = time.monotonic()-started-evidence['timing']['elapsed_seconds']
        evidence['harness_sha256']=hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
        write(private / 'result.json',evidence)
    print(json.dumps(dict(outcome=evidence['outcome'],evidence=str(private / 'result.json'))))
    return 0 if evidence['outcome']=='success' else 1


def combine(public, private, retained, fixture, shares, resolved, timeout, denied):
    integration=public / 'integration'
    public_guard = lambda argv: sandbox(argv, [public], denied)
    if not all(shares.values()):
        return dict(candidate=None,passed=False,conflict='Both drafts are needed')
    # Agents may resolve conflicts with ordinary Git in the integration worktree.
    if resolved:
        if len(resolved)!=40 or any(char not in '0123456789abcdef' for char in resolved):
            raise RuntimeError('Resolved candidate must be an exact 40-character Git commit ID')
        for commit in shares.values():
            git(integration,'merge-base','--is-ancestor',commit,resolved,guard=public_guard)
        candidate=git(integration,'rev-parse',resolved+'^{commit}',guard=public_guard)
    else:
        git(integration,'reset','--hard',shares['A'],guard=public_guard)
        git(integration,'clean','-fdx',guard=public_guard)
        result=command(public_guard(['git','-c','core.hooksPath=/dev/null','merge','--no-edit',shares['B']]),
                       integration,dict(PATH=os.environ['PATH'],**FIXED_ENV))
        if result.returncode:
            return dict(candidate=None,passed=False,conflict=result.stdout + result.stderr)
        candidate=git(integration,'rev-parse','HEAD',guard=public_guard)
    fetch_denied = [path for path in denied if path!=private] + [path for path in private.iterdir() if path!=retained]
    fetch_guard = lambda argv: sandbox(argv, [retained], fetch_denied)
    git(retained,'fetch',str(public / 'repo'),candidate,guard=fetch_guard)
    git(retained,'checkout','--detach',candidate)
    checks=check(retained,fixture,private,timeout,denied_roots=[path for path in denied if path!=private])
    return dict(candidate=candidate,passed=checks['visible']['passed'] and checks['oracle']['passed'],
                conflict=None,visible=checks['visible'],oracle=checks['oracle'])


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    sub=parser.add_subparsers(dest='command',required=True)
    sub.add_parser('freeze')
    sub.add_parser('verify')
    sub.add_parser('schedule')
    prepare_parser=sub.add_parser('prepare')
    prepare_parser.add_argument('fixture',choices=FIXTURES)
    prepare_parser.add_argument('--output',required=True)
    oracle_parser=sub.add_parser('oracle')
    oracle_parser.add_argument('fixture',choices=FIXTURES)
    oracle_parser.add_argument('--repo',required=True)
    oracle_parser.add_argument('--evidence',required=True)
    oracle_parser.add_argument('--deny-root',action='append',default=[])
    baseline_parser=sub.add_parser('baseline')
    baseline_parser.add_argument('fixture',choices=FIXTURES)
    baseline_parser.add_argument('--config',required=True)
    baseline_parser.add_argument('--output',required=True)
    baseline_parser.add_argument('--pair',default='unscored')
    baseline_parser.add_argument('--order',type=int,default=0)
    args=parser.parse_args()
    if args.command=='freeze':
        freeze()
        return 0
    if args.command=='verify':
        return verify()
    if args.command=='prepare':
        manifest=frozen()
        fixture=read(ROOT / 'fixtures' / (args.fixture+'.json'))
        sha=initial(Path(args.output).resolve(),fixture)
        if sha!=manifest['initial_commits'][args.fixture]:
            raise RuntimeError('Initial commit differs from manifest')
        print(json.dumps(dict(initial_snapshot=sha)))
        return 0
    if args.command=='oracle':
        frozen()
        fixture=read(ROOT / 'fixtures' / (args.fixture+'.json'))
        evidence=Path(args.evidence).resolve()
        evidence.mkdir()
        result=check(Path(args.repo).resolve(),fixture,evidence,denied_roots=[Path(path).resolve() for path in args.deny_root])
        write(evidence / 'checks.json',result)
        print(json.dumps(dict(candidate=result['candidate'], passed=result['oracle']['passed'] and result['visible']['passed'])))
        return 0 if result['oracle']['passed'] and result['visible']['passed'] else 1
    if args.command=='schedule':
        print(json.dumps(schedule(),indent=2))
        return 0
    return baseline(args)


if __name__=='__main__':
    try:
        sys.exit(main())
    except Exception as error:
        print(str(error),file=sys.stderr)
        sys.exit(1)
