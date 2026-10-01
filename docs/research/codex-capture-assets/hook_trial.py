#!/usr/bin/env python3
"""Replay vetted hooks; only invocation-local hook trust is bypassed."""
import json, subprocess, sys, tempfile
from pathlib import Path
root=Path(sys.argv[1]) if len(sys.argv)>1 else Path(tempfile.mkdtemp(prefix='codex-hooks-'))
root.mkdir(parents=True,exist_ok=True)
subprocess.run(['git','init','-b','research/codex-capture-trial',str(root)],check=True,capture_output=True)
hook=Path(__file__).with_name('hooks.py').resolve()
command=f'{sys.executable} {hook} {root}/hooks-events.jsonl'
args=['codex','exec','--ignore-user-config','--ephemeral','--dangerously-bypass-hook-trust','-C',str(root),'-s','workspace-write','--json']
for event in ('PreToolUse','PostToolUse'):
    args+=['-c','hooks.'+event+'=[{matcher=".*",hooks=[{type="command",command='+json.dumps(command)+'}]}]']
args+=['Use apply_patch to add native-hook.txt containing native. Then use shell redirection printf shell > shell-hook.txt. Do not commit. Finish.']
(root/'hooks.command.json').write_text(json.dumps(args,indent=2))
with (root/'hooks.jsonl').open('w') as out,(root/'hooks.stderr').open('w') as err:
    p=subprocess.run(args,stdin=subprocess.DEVNULL,stdout=out,stderr=err,timeout=240)
assert p.returncode==0
rows=[json.loads(x) for x in (root/'hooks-events.jsonl').read_text().splitlines()]
assert {r['tool_name'] for r in rows}>={'apply_patch','Bash'}
assert {r['hook_event_name'] for r in rows}>={'PreToolUse','PostToolUse'}
print(root)
