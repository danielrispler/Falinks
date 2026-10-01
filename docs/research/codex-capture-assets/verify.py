#!/usr/bin/env python3
"""Offline assertions over retained trial evidence; no model calls."""
import hashlib, json, subprocess, tempfile
from pathlib import Path
root=Path(__file__).parent/'evidence'
r=json.loads((root/'results.json').read_text())
base=(root/'base.ts').read_text(); ready=base.replace('return 1;','return 2;')
assert (root/'candidate/fixture.ts').read_text()==ready
assert hashlib.sha256(ready.encode()).hexdigest()==r['isolated_candidate']['sha256']
assert 'UNFINISHED_B' in (root/'agent-b/fixture.ts').read_text()
assert 'UNFINISHED_B' in (root/'shared-candidate.ts').read_text()
assert r['active_external_process']['capture_is_not_quiescence']
assert r['sequential_group_counterexample']['consistent_group'] is False
for name in ('generated-a.ts','generated-b.ts'):
    assert (root/'group-candidate'/name).exists()
assert r['shell_group']['generated_exact_contents'] is False
hooks=[json.loads(x) for x in (root/'hooks-events.jsonl').read_text().splitlines()]
assert {x['tool_name'] for x in hooks}>={'Bash','apply_patch'}
assert {x['hook_event_name'] for x in hooks}>={'PreToolUse','PostToolUse'}
watch=json.loads((root/'interfaces/interface-results.json').read_text())['watch_event']['params']
assert set(watch)=={'watchId','changedPaths'}
with tempfile.TemporaryDirectory() as tmp:
    p=Path(tmp); (p/'fixture.ts').write_text(base)
    subprocess.run(['git','apply',str((root/'ready-turn.diff').resolve())],cwd=p,check=True)
    assert (p/'fixture.ts').read_text()==ready
assert (root/'retained-base-candidate.ts').read_text()==ready
print('PASS: exact isolated/native-replay candidates; draft exclusion/preservation; shared contamination; hook/watch/process boundaries')
