"""Verify retained bounded observations; starts no model turn or external process."""
import json
from pathlib import Path
ROOT=Path(__file__).resolve().parent
load=lambda name:json.loads((ROOT/name).read_text())
c=load('capture-results.json')
assert c['coherent_mid_capture'] and c['captured_mid_install']!=c['captured_after_install']
assert c['new_revision_excludes_partial_install'] and c['unchanged_blob_reused'] and c['test_worktree_isolated_from_later_draft']
for name in ['runtime-results.json','runtime-limited-results.json','runtime-protected-results.json']:
    r=load(name)
    assert r['completed']['status']=='completed' and r['server_exit']==0 and not r.get('failure')
    assert r['registration']['activePermissionProfile']['id']=='falinks-bridge'
    assert r['steer']['turnId']==r['completed']['id']
    assert [x['arguments']['action'] for x in r['reviews']]==['defer','process']
    assert all(x['accepted'] for x in r['reviews'])
    assert [x['accepted'] for x in r['offers']]==[False,True]
    assert not r['bypass_observations'] and len(r['host_edits'])==1 and r['host_edits'][0]['accepted']
    output='\n'.join(x['item'].get('aggregatedOutput','') for x in r['events'] if x['method']=='item/completed')
    assert 'SHELL_DENIED 1' in output and 'PermissionError: [Errno 1]' in output and 'SyntaxError' not in output
limited=load('runtime-limited-results.json')
assert all(feature in limited['command'] for feature in ['apps','computer_use','browser_use','image_generation','multi_agent'])
protected=load('runtime-protected-results.json');assert protected['controller_state_open_denied'] and protected['scratch_control_writable']
assert any('="deny"' in x and 'filesystem=' in x for x in protected['command'])
r=load('resume-results.json');assert r['model_turns_started']==0 and r['processed_obligation_restored'] and r['synthetic_deferred_obligation_restored']
assert set(r['restored_dynamic_tool_names'])=={'engine_edit_probe','engine_review_probe','engine_offer_probe'}
assert r['native_path_verified'] is False # Explicitly preserve this coverage limitation.
r=load('deferred-resume-results.json');assert r['completed']=='completed' and r['source_unchanged'] and r['ordinary_reads']
assert [x['accepted'] for x in r['offers']]==[False,True] and len(r['reviews'])==1 and r['reviews'][0]['accepted']
n=load('native-results.json');assert n['exit']!=0 and n['source_unchanged'] and n['writable_control_patch_succeeded']
s=load('sandbox-results.json');assert s['source_unchanged'] and len(s['probe']['attempts'])==8 and all(x['denied'] for x in s['probe']['attempts'].values())
w=load('worktree-results.json');assert w['individual_test']['exit']!=0 and w['team_test']['exit']==0 and w['live_draft_preserved'] and w['unchanged_fixture_inode_and_mtime_preserved']
print('PASS: retained capture, reusable-worktree, restricted-runtime, steering, deferred reconnect and bounded writer evidence; no production guarantees inferred.')
