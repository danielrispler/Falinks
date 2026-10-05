import fcntl, json, os, statistics, subprocess, tempfile, time
from pathlib import Path
OUT = Path(__file__).resolve().parent
RUN = Path(tempfile.mkdtemp(prefix='trial-', dir=OUT))
REPO = RUN / 'live'
TEST = RUN / 'test-worktree'
REPO.mkdir()

def git(*args, cwd=REPO):
    return subprocess.check_output(['git', *args], cwd=cwd, stderr=subprocess.PIPE).decode().strip()

def commit(message):
    git('add', '.')
    git('commit', '-m', message)
    return git('rev-parse', 'HEAD')

def switch(sha):
    start = time.perf_counter_ns()
    git('reset', '--hard', sha, cwd=TEST)
    return (time.perf_counter_ns() - start) / 1e6

def check():
    env = dict(os.environ, GOPROXY='off', GOTOOLCHAIN='local', GOCACHE=str(RUN / 'go-cache'))
    p = subprocess.run(['go', 'test', '-count=1', './...'], cwd=TEST, env=env, capture_output=True, text=True)
    return {'exit': p.returncode, 'output': (p.stdout + p.stderr).strip()}

git('init', '-b', 'scratch')
git('config', 'user.name', 'Scratch probe')
git('config', 'user.email', 'scratch@example.invalid')
(REPO / 'go.mod').write_text('module example.invalid/worktreeprobe\n\ngo 1.20\n')
(REPO / 'price.go').write_text('package probe\nfunc Quote(amount int) int { return amount }\nfunc Shipping() int { return 5 }\n')
(REPO / 'checkout.go').write_text('package probe\nfunc Checkout() int { return Quote(100) + Shipping() }\n')
(REPO / 'checkout_test.go').write_text('package probe\nimport "testing"\nfunc TestCheckout(t *testing.T) { if Checkout() != 105 { t.Fatal(Checkout()) } }\n')
for n in range(1000):
    p = REPO / 'fixtures' / f'file-{n}.txt'
    p.parent.mkdir(exist_ok=True)
    p.write_bytes((f'fixture {n}\n'.encode() + b'x' * 4096)[:4096])
base = commit('published base')
git('update-ref', 'refs/probe/published', base)
price = REPO / 'price.go'
price.write_text('package probe\nfunc Quote(amount int, currency string) int { return amount }\nfunc Shipping() int { return 5 }\n')
alice = commit('Alice signature only; incomplete group')
git('update-ref', 'refs/probe/alice', alice)
(REPO / 'checkout.go').write_text('package probe\nfunc Checkout() int { return Quote(100, "USD") + Shipping() }\n')
team = commit('Alice plus Bob; complete exact group')
git('update-ref', 'refs/probe/team', team)
# The live workspace continues with unfinished draft bytes after the candidate exists.
price.write_text(price.read_text() + '// later live draft, outside retained candidate\n')
live_before = price.read_bytes()
git('worktree', 'add', '--detach', str(TEST), base)
lock = open(RUN / 'test-worktree.lock', 'w')
fcntl.flock(lock, fcntl.LOCK_EX)
contender = subprocess.run(['python3', '-c', 'import fcntl,sys; f=open(sys.argv[1]);\ntry: fcntl.flock(f,fcntl.LOCK_EX|fcntl.LOCK_NB)\nexcept BlockingIOError: sys.exit(7)', str(RUN / 'test-worktree.lock')])
assert contender.returncode == 7
baseline_check = check()
assert baseline_check['exit'] == 0, baseline_check
switch(alice)
alice_check = check()
assert alice_check['exit'] != 0 and 'not enough arguments' in alice_check['output'], alice_check
switch(base)
assert git('status', '--porcelain', cwd=TEST) == ''
assert price.read_bytes() == live_before
switch(team)
# Advance the published pointer during the check; do not sync the locked worktree.
# Same file, different function: Shipping changes while Quote was proposed earlier.
# Construct an immutable publication using an independent Git index, no live writes.
index = RUN / 'published.index'
env = dict(os.environ, GIT_INDEX_FILE=str(index))
subprocess.run(['git', 'read-tree', base], cwd=REPO, env=env, check=True)
blob = subprocess.check_output(['git', 'hash-object', '-w', '--stdin'], cwd=REPO, input=b'package probe\nfunc Quote(amount int) int { return amount }\nfunc Shipping() int { return 7 }\n').decode().strip()
subprocess.run(['git','update-index','--cacheinfo',f'100644,{blob},price.go'],cwd=REPO,env=env,check=True)
tree = subprocess.check_output(['git','write-tree'],cwd=REPO,env=env).decode().strip()
advanced = subprocess.check_output(['git','commit-tree',tree,'-p',base,'-m','new published shipping revision'],cwd=REPO).decode().strip()
git('update-ref', 'refs/probe/published', advanced, base)
team_check = check()
assert team_check['exit'] == 0, team_check
assert git('rev-parse', 'HEAD', cwd=TEST) == team
assert git('rev-parse', 'refs/probe/published') != base
# This deliberately surfaces eligibility change; a passing old candidate is not accepted.
stale_base = {'checked_candidate': team, 'expected_base': base, 'current_published': advanced, 'outcome': 'base changed; no acceptance, reconsider/recombine and check new exact candidate'}
switch(advanced)
assert 'Shipping() int { return 7 }' in (TEST / 'price.go').read_text()
assert 'Quote(amount int)' in (TEST / 'price.go').read_text()
assert git('status', '--porcelain', cwd=TEST) == ''
assert price.read_bytes() == live_before
fcntl.flock(lock, fcntl.LOCK_UN)
# Benchmark base <-> team only: two modified source files, 1,000 unchanged fixture files.
fixture = TEST / 'fixtures/file-0.txt'
identity = (fixture.stat().st_ino, fixture.stat().st_mtime_ns)
apply, restore = [], []
switch(base)
for _ in range(15):
    apply.append(switch(team))
    restore.append(switch(base))
assert (fixture.stat().st_ino, fixture.stat().st_mtime_ns) == identity

def stats(values): return {'median_ms':round(statistics.median(values),3),'min_ms':round(min(values),3),'max_ms':round(max(values),3),'runs_ms':[round(v,3) for v in values]}
result = {'scratch_run':str(RUN),'git':git('--version'),'go':subprocess.check_output(['go','version']).decode().strip(),'files':1004,'fixture_bytes':4096000,'changed_files':2,'baseline_test':baseline_check,'individual_test':alice_check,'team_test':team_check,'exclusive_lock_contender_exit':contender.returncode,'published_during_test':stale_base,'live_draft_preserved':True,'unchanged_fixture_inode_and_mtime_preserved':True,'apply_candidate':stats(apply),'restore_base':stats(restore),'scope':'Real scratch Git worktree and Go tests; known prepared candidates. No runtime capture/attribution, publication engine, group approval, crash recovery, build-artifact cleanup or automatic semantic rebase implementation.'}
(OUT / 'results.json').write_text(json.dumps(result,indent=2))
print(json.dumps(result,indent=2))
