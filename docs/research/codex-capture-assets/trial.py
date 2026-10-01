#!/usr/bin/env python3
"""Bounded scratch experiment; never operates on the caller's repository."""
import hashlib, json, os, shutil, subprocess, sys, tempfile, time
from pathlib import Path
BASE = 'export function ready(): number {\n  return 1;\n}\n\nexport function unfinished(): number {\n  return 10;\n}\n'
READY = BASE.replace('return 1;', 'return 2;')
ROOT = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(tempfile.mkdtemp(prefix='codex-capture-replay.'))
ROOT.mkdir(parents=True, exist_ok=True)
results = {}
def run(args, cwd=None):
    return subprocess.run(args, cwd=cwd, text=True, capture_output=True, check=True).stdout
def init(name):
    p = ROOT / name
    p.mkdir(exist_ok=True)
    run(['git','init','-b','research/codex-capture-trial'],p)
    (p/'fixture.ts').write_text(BASE)
    run(['git','add','fixture.ts'],p)
    run(['git','-c','user.name=Research','-c','user.email=research@example.invalid','commit','-m','fixture'],p)
    return p
def codex(name,p,prompt):
    args=['codex','exec','--ignore-user-config','--ephemeral','-C',str(p),'-s','workspace-write','--json',prompt]
    (ROOT/(name+'.command.json')).write_text(json.dumps(args,indent=2))
    start=time.monotonic()
    with (ROOT/(name+'.jsonl')).open('w') as out, (ROOT/(name+'.stderr')).open('w') as err:
        proc=subprocess.Popen(args,stdin=subprocess.DEVNULL,stdout=out,stderr=err)
        # ponytail: bounded researcher run only, not an engine deadline or agent readiness policy.
        try: code=proc.wait(timeout=240)
        except subprocess.TimeoutExpired:
            proc.terminate(); code=proc.wait(); results[name+'_gap']='Research timebox; harness did not complete'
    results[name]={'exit':code,'seconds':round(time.monotonic()-start,3)}
    return code
start=time.monotonic()
a=init('agent-a'); b=init('agent-b')
results['setup_seconds']=round(time.monotonic()-start,6)
assert codex('unfinished',b,'Use shell redirection to replace fixture.ts with exactly this unfinished draft, then stop your turn without repairing it or committing. It is deliberately unfinished, not ready:\n'+BASE.replace('return 10;','return ; // UNFINISHED_B')) == 0
assert 'UNFINISHED_B' in (b/'fixture.ts').read_text()
assert codex('ready',a,'Use native apply_patch to change only ready() in fixture.ts from return 1 to return 2. Do not change unfinished(), do not commit. Finish with READY.') == 0
assert (a/'fixture.ts').read_text()==READY
start=time.monotonic()
candidate=ROOT/'candidate'; shutil.copytree(a,candidate,ignore=shutil.ignore_patterns('.git','.codex'))
results['capture_seconds']=round(time.monotonic()-start,6)
hash_before=hashlib.sha256((candidate/'fixture.ts').read_bytes()).hexdigest()
(b/'fixture.ts').write_text((b/'fixture.ts').read_text().replace('UNFINISHED_B','UNFINISHED_B_CONTINUES'))
assert hashlib.sha256((candidate/'fixture.ts').read_bytes()).hexdigest()==hash_before
assert 'UNFINISHED_B_CONTINUES' in (b/'fixture.ts').read_text()
results['isolated_candidate']={'sha256':hash_before,'exact_ready':True,'draft_preserved':True,'stable_after_draft_change':True}
# Temporary file copy with explicit retained base; not a general symbol extractor.
shared=init('shared'); draft=BASE.replace('return 10;','return ; // UNFINISHED_B')
(shared/'fixture.ts').write_text(draft)
basefile=ROOT/'base.ts'; basefile.write_text(BASE)
readyfile=ROOT/'ready.ts'; readyfile.write_text(READY)
merge=subprocess.run(['git','merge-file','-p',str(shared/'fixture.ts'),str(basefile),str(readyfile)],text=True,capture_output=True)
results['retained_base_merge']={'exit':merge.returncode,'contents':merge.stdout}
# Adjacent owner edits may conflict with line-oriented merge; never publish conflict output.
if merge.returncode==0:
    assert 'return 2;' in merge.stdout and 'UNFINISHED_B' in merge.stdout
# A fixed ready candidate is base + A, not shared live bytes containing B.
assert 'UNFINISHED_B' not in READY
assert codex('shared-native',shared,'Use apply_patch to change only ready() from return 1 to return 2. Preserve the deliberately unfinished other function verbatim. Do not fix it, do not commit. Finish with READY.')==0
assert (shared/'fixture.ts').read_text()==READY.replace('return 10;','return ; // UNFINISHED_B')
shared_capture=ROOT/'shared-candidate.ts'; shared_capture.write_bytes((shared/'fixture.ts').read_bytes())
assert 'UNFINISHED_B' in shared_capture.read_text()
results['shared_before_after']={'draft_contaminates_capture':True,'draft_preserved_in_live_file':True}
assert codex('shell-group',a,'Use ordinary shell tools, not apply_patch: printf shell-ready into shell.txt; then run a single Python generator command that writes generated-a.ts containing export const a = 1; and generated-b.ts containing export const b = 2; (each with newline). Do not commit. Finish with READY.')==0
assert (a/'shell.txt').read_text()=='shell-ready'
assert (a/'generated-a.ts').exists() and (a/'generated-b.ts').exists()
shutil.copytree(a,ROOT/'group-candidate',ignore=shutil.ignore_patterns('.git','.codex'))
results['shell_group']={'both_members_captured':True,'generated_exact_contents':(a/'generated-a.ts').read_text()=='export const a = 1;\n' and (a/'generated-b.ts').read_text()=='export const b = 2;\n'}
# Deterministic external writer proves the capture boundary independently of model choice.
writer=subprocess.Popen([sys.executable,'-c',"from pathlib import Path; import sys,time; p=Path(sys.argv[1]); p.write_text('phase-one'); time.sleep(2); p.write_text('phase-two')",str(a/'still-writing.txt')])
while not (a/'still-writing.txt').exists(): time.sleep(.01)
(ROOT/'during-write.txt').write_bytes((a/'still-writing.txt').read_bytes())
assert writer.poll() is None
writer.wait()
assert (ROOT/'during-write.txt').read_text()=='phase-one'
assert (a/'still-writing.txt').read_text()=='phase-two'
results['active_external_process']={'writer_alive_at_capture':True,'captured':'phase-one','later_live':'phase-two','capture_is_not_quiescence':True}
# Torn group counterexample: capture A before writer changes both, B afterwards.
(a/'pair-a.txt').write_text('old'); (a/'pair-b.txt').write_text('old')
x=(a/'pair-a.txt').read_text()
(a/'pair-a.txt').write_text('new'); (a/'pair-b.txt').write_text('new')
y=(a/'pair-b.txt').read_text()
assert (x,y)==('old','new')
results['sequential_group_counterexample']={'captured':[x,y],'consistent_group':False}
results['versions']={k:run(v).strip() for k,v in {'codex':['codex','--version'],'node':['node','--version'],'git':['git','--version'],'python':[sys.executable,'--version']}.items()}
results['tsc_available']=shutil.which('tsc')
(ROOT/'results.json').write_text(json.dumps(results,indent=2))
print(json.dumps(results,indent=2))
