"""Public CLI checks; no private helper assertions or model calls."""
import json
import hashlib
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
CLI = ROOT / 'run.py'


def cli(*args):
    return subprocess.run([sys.executable, str(CLI), *map(str, args)], text=True,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE)


class HarnessTest(unittest.TestCase):
    def test_frozen_oracles_reject_initial_and_accept_reference(self):
        result = cli('verify')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        evidence = json.loads(result.stdout)
        self.assertEqual(set(evidence['fixtures']), {'rust-errors', 'go-page', 'go-relationships'})
        for fixture in evidence['fixtures'].values():
            self.assertFalse(fixture['initial']['passed'])
            self.assertTrue(fixture['reference']['passed'])
            self.assertTrue(fixture['initial_visible']['passed'])
            self.assertTrue(fixture['reference_visible']['passed'])


    def test_oracles_reject_compiling_behavioral_regressions(self):
        mutants = {
            'rust-errors': ('src/consumer.rs', 'Err(LookupError::Unavailable { retryable: true })'),
            'go-page': ('handler.go', 'wrong-filter'),
            'go-relationships': ('producer.go', 'wrong-ids'),
        }
        with tempfile.TemporaryDirectory(prefix='falinks-oracle-test-') as tmp:
            for name, (file, defect) in mutants.items():
                with self.subTest(fixture=name):
                    base=Path(tmp)/name
                    base.mkdir()
                    repo=base/'repo'
                    prepared=cli('prepare',name,'--output',repo)
                    self.assertEqual(prepared.returncode,0,prepared.stderr)
                    reference=json.loads((ROOT/'protected'/f'{name}.json').read_text())['reference']
                    for path, text in reference.items():
                        (repo/path).write_text(text)
                    if name=='rust-errors':
                        text=(repo/file).read_text().replace(
                            'keys.iter().try_fold(0, |sum, key| lookup(key).map(|value| sum + value))', defect)
                    elif name=='go-page':
                        text=(repo/file).read_text().replace('kind := r.URL.Query().Get("kind")','kind := ""')
                    else:
                        text=(repo/file).read_text().replace('ID:i+1','ID:len(out)+1').replace('for i, line := range lines','for _, line := range lines')
                    (repo/file).write_text(text)
                    for args in [('add','.'),('commit','-m','Intentional behavioral defect')]:
                        subprocess.run(['git','-c','user.name=Test','-c','user.email=test@example.invalid',*args],cwd=repo,check=True,stdout=subprocess.DEVNULL)
                    evidence=base/'evidence'
                    result=cli('oracle',name,'--repo',repo,'--evidence',evidence)
                    self.assertEqual(result.returncode,1,result.stdout+result.stderr)
                    checks=json.loads((evidence/'checks.json').read_text())
                    self.assertTrue(checks['visible']['passed'],checks)
                    self.assertFalse(checks['oracle']['passed'])

    def test_oracle_blocks_prior_arm_reads_and_checks_export_ignored_files(self):
        with tempfile.TemporaryDirectory(prefix='falinks-isolation-test-') as tmp:
            base=Path(tmp)
            repo=base/'repo'
            prior=base/'prior-arm'
            prior.mkdir()
            secret=prior/'solution.go'
            secret.write_text('protected prior-arm solution')
            result=cli('prepare','go-page','--output',repo)
            self.assertEqual(result.returncode,0,result.stderr)
            reference=json.loads((ROOT/'protected/go-page.json').read_text())['reference']
            (repo/'handler.go').write_text(reference['handler.go'])
            denied_path=json.dumps(str(secret))
            (repo/'isolation_test.go').write_text(
                'package fixture_test\nimport("os";"testing")\n'
                'func TestPriorArmUnreadable(t *testing.T) { if _,err:=os.ReadFile('+denied_path+'); err==nil { t.Fatal("prior-arm solution readable") } }\n')
            def commit():
                for argv in [('add','.'),('commit','-m','Isolation candidate')]:
                    subprocess.run(['git','-c','user.name=Test','-c','user.email=test@example.invalid',*argv],cwd=repo,check=True,stdout=subprocess.DEVNULL)
            commit()
            result=cli('oracle','go-page','--repo',repo,'--evidence',base/'denied-check','--deny-root',prior)
            self.assertEqual(result.returncode,0,result.stdout+result.stderr)
            # Git archive would silently omit this failing focused test. Exact tree checks must see it.
            (repo/'isolation_test.go').write_text('package fixture_test\nimport "testing"\nfunc TestMustNotBeIgnored(t *testing.T) { t.Fatal("intentional focused failure") }\n')
            (repo/'.gitattributes').write_text('isolation_test.go export-ignore\n')
            commit()
            result=cli('oracle','go-page','--repo',repo,'--evidence',base/'exact-check','--deny-root',prior)
            self.assertEqual(result.returncode,1,result.stdout+result.stderr)
            checks=json.loads((base/'exact-check/checks.json').read_text())
            self.assertFalse(checks['visible']['passed'])
            self.assertTrue(checks['oracle']['passed'])

    def test_codex_bridge_waits_for_peer_events_and_retains_all_usage(self):
        with tempfile.TemporaryDirectory(prefix='falinks-codex-bridge-test-') as tmp:
            base=Path(tmp)
            reference=json.loads((ROOT/'protected/go-relationships.json').read_text())['reference']
            runtime=base/'fake-codex'
            runtime.write_text('#!/usr/bin/env python3\nREFERENCE='+repr(reference)+'\n'+FAKE_CODEX)
            runtime.chmod(0o700)
            config=json.loads((ROOT/'config.json').read_text())
            config.update(worker_command=[sys.executable,'{worker}'],worker_script=str(ROOT/'codex_worker.py'),
                          model='scripted',model_version='test-v1',reasoning='none',
                          runtime_version='scripted-v1',runtime_sha256=hashlib.sha256(runtime.read_bytes()).hexdigest(),runtime_binary=str(runtime),
                          auth_file=None,denied_roots=[],timeout_seconds=60)
            config_path=base/'config.json'
            config_path.write_text(json.dumps(config))
            run=base/'run'
            result=cli('baseline','go-relationships','--config',config_path,'--output',run)
            self.assertEqual(result.returncode,0,result.stdout+result.stderr+
                             (run/'controller/result.json').read_text())
            evidence=json.loads((run/'controller/result.json').read_text())
            self.assertEqual(evidence['usage']['status'],'available')
            for agent in ('A','B'):
                reports=evidence['usage']['agents'][agent]
                turns=list((run/'agents'/agent/'scratch').glob('turn-*.jsonl'))
                self.assertGreater(len(turns),2)
                self.assertEqual(len(reports),len(turns))
                self.assertEqual([r['turn'] for r in reports],list(range(1,len(turns)+1)))

    def test_baseline_retains_resolved_same_handler_merge_for_final_check(self):
        with tempfile.TemporaryDirectory(prefix='falinks-conflict-test-') as tmp:
            base=Path(tmp)
            reference=json.loads((ROOT/'protected/go-page.json').read_text())['reference']
            worker=base/'worker.py'
            worker.write_text('REFERENCE='+repr(reference)+'\n'+CONFLICT_WORKER)
            config=json.loads((ROOT/'config.json').read_text())
            config.update(worker_command=[sys.executable,'{worker}'],worker_script=str(worker),
                          model='scripted',model_version='test-v1',reasoning='none',
                          runtime_version='scripted-v1',runtime_sha256=hashlib.sha256(Path(sys.executable).resolve().read_bytes()).hexdigest(),runtime_binary=sys.executable,
                          auth_file=None,denied_roots=[],timeout_seconds=60)
            config_path=base/'config.json'
            config_path.write_text(json.dumps(config))
            run=base/'run'
            result=cli('baseline','go-page','--config',config_path,'--output',run)
            self.assertEqual(result.returncode,0,result.stdout+result.stderr+
                             (run/'controller/result.json').read_text())
            evidence=json.loads((run/'controller/result.json').read_text())
            attempts=evidence['validation_attempts']
            self.assertEqual(len(attempts),3)
            self.assertTrue(attempts[0]['conflict'])
            self.assertTrue(attempts[1]['passed'])
            self.assertTrue(attempts[2]['passed'])
            self.assertEqual(evidence['final_snapshot'],attempts[1]['candidate'])

    def test_baseline_exchanges_unfinished_work_and_freezes_exact_candidate(self):
        with tempfile.TemporaryDirectory(prefix='falinks-baseline-test-') as tmp:
            base=Path(tmp)
            # Scripted clients test orchestration, not model performance. Reference bytes are
            # embedded by the trusted test host; real workers never receive reference materials.
            reference=json.loads((ROOT/'protected/go-relationships.json').read_text())['reference']
            worker=base/'worker.py'
            worker.write_text("REFERENCE="+repr(reference)+"\n"+SCRIPTED_WORKER)
            config=json.loads((ROOT/'config.json').read_text())
            config.update(worker_command=[sys.executable,'{worker}'],worker_script=str(worker),
                          model='scripted',model_version='test-v1',reasoning='none',
                          runtime_version='scripted-v1',runtime_sha256=hashlib.sha256(Path(sys.executable).resolve().read_bytes()).hexdigest(),runtime_binary=sys.executable,
                          auth_file=None,denied_roots=[],timeout_seconds=60)
            config_path=base/'config.json'
            config_path.write_text(json.dumps(config))
            run=base/'run'
            result=cli('baseline','go-relationships','--config',config_path,'--output',run)
            self.assertEqual(result.returncode,0,result.stdout+result.stderr+
                             (run/'controller/result.json').read_text())
            evidence=json.loads((run/'controller/result.json').read_text())
            self.assertEqual(evidence['outcome'],'success')
            self.assertEqual(evidence['usage']['status'],'unavailable')
            self.assertFalse(evidence['scored'])
            self.assertEqual(len(evidence['final_snapshot']),40)
            events=[json.loads(line) for line in (run/'controller/events.jsonl').read_text().splitlines()]
            self.assertEqual([e['milestone']['id'] for e in events if e['kind']=='development'],
                             ['record-contract','follow-ups'])
            self.assertEqual({e['agent'] for e in events if e['kind']=='draft_shared'},{'A','B'})
            # Sandbox protection must be an observed denial, not a written instruction.
            self.assertEqual({e['agent'] for e in events if e['kind']=='worker_request' and
                              e['request'].get('text')=='protected-read-denied'},{'A','B'})
            repeat=cli('baseline','go-relationships','--config',config_path,'--output',run)
            self.assertNotEqual(repeat.returncode,0)


SCRIPTED_WORKER = r"""
import json
from pathlib import Path
import sys
import threading

def send(**value):
    print(json.dumps(value),flush=True)
start=json.loads(sys.stdin.readline())
agent=start['agent']
file='producer.go' if agent=='A' else 'consumer.go'
try:
    # Resolve controller relative to the public worktree; protected by sandbox-exec.
    (Path(start['workspace']).parent.parent/'controller/events.jsonl').read_text()
except PermissionError:
    send(action='message',text='protected-read-denied')
else:
    raise AssertionError('controller readable')
Path(file).write_text(Path(file).read_text()+'\n// first unfinished draft '+agent+'\n')
send(action='share')
last=None
final=False
pending=None
for line in sys.stdin:
    event=json.loads(line)
    if event['type']=='development':
        pending=event
        send(action='ack',milestone=event['id'])
    elif event['type']=='ok' and event.get('action')=='ack':
        if pending['id']=='record-contract':
            Path(file).write_text(REFERENCE[file]+'\n// unfinished contract draft\n')
        else:
            Path(file).write_text(REFERENCE[file])
            final=True
        send(action='share')
    elif event['type']=='shared':
        last=event['commit']
        if final:
            send(action='ready',commit=last)
    elif event['type']=='ok' and event.get('action')=='ready':
        threading.Event().wait()
"""


FAKE_CODEX = r"""
import json
from pathlib import Path
import sys

argv=sys.argv[1:]
assert argv[0]=='exec'
assert argv[argv.index('-m')+1]=='scripted'
if 'resume' in argv:
    assert argv[-2]=='fixture-thread'
prompt=sys.stdin.read()
events=json.loads(prompt.rsplit('\n',1)[1])
# A telemetry acknowledgment alone is not new peer/task context.
assert any(e.get('type')!='ok' or e.get('action')!='usage' for e in events), 'telemetry wait spin'
state_file=Path('scratch/fake-state.json')
state=json.loads(state_file.read_text()) if state_file.exists() else {'phase':'start'}
response=dict(action='wait',text='',milestone='',commit='',resolved='')
start=next((e for e in events if e['type']=='start'),None)
if start:
    state['agent']=start['agent']
    state['file']='producer.go' if start['agent']=='A' else 'consumer.go'
    response.update(action='message',text='Plan: implement '+state['file'])
    state['phase']='plan'
elif state['phase']=='plan' and any(e['type']=='ok' and e.get('action')=='message' for e in events):
    file=Path(state['file'])
    file.write_text(file.read_text()+'\n// unfinished draft\n')
    response['action']='share'
    state['phase']='initial'
else:
    development=next((e for e in events if e['type']=='development'),None)
    if development:
        response.update(action='ack',milestone=development['id'])
        state['phase']='ack-'+development['id']
    elif state['phase'].startswith('ack-') and any(e['type']=='ok' and e.get('action')=='ack' for e in events):
        final=state['phase']=='ack-follow-ups'
        Path(state['file']).write_text(REFERENCE[state['file']]+('' if final else '\n// contract draft\n'))
        response['action']='share'
        state['phase']='final' if final else 'contract'
    elif state['phase']=='final':
        shared=next((e for e in events if e['type']=='shared'),None)
        if shared:
            response.update(action='ready',commit=shared['commit'])
            state['phase']='ready'
state_file.write_text(json.dumps(state))
Path(argv[argv.index('-o')+1]).write_text(json.dumps(response))
print(json.dumps(dict(type='thread.started',thread_id='fixture-thread')))
print(json.dumps(dict(type='turn.completed',usage=dict(input_tokens=2,cached_input_tokens=0,output_tokens=3))))
"""


CONFLICT_WORKER = r"""
import json
from pathlib import Path
import subprocess
import sys
import threading

def send(**value):
    print(json.dumps(value),flush=True)
start=json.loads(sys.stdin.readline())
agent=start['agent']
Path('handler.go').write_text(Path('handler.go').read_text()+'\n// initial '+agent+'\n')
send(action='share')
final=False
own_final=None
peer_final=False
checking=False
for line in sys.stdin:
    event=json.loads(line)
    if event['type']=='development':
        send(action='ack',milestone=event['id'])
    elif event['type']=='ok' and event.get('action')=='ack':
        Path('handler.go').write_text(REFERENCE['handler.go']+'\n// final '+agent+'\n')
        final=True
        send(action='share')
    elif event['type']=='shared' and final:
        own_final=event['commit']
        if agent=='B':
            send(action='ready',commit=own_final)
    elif event['type']=='peer_diff' and event['sender']=='B' and '+// final B' in event['diff']:
        peer_final=True
    elif event['type']=='check_result':
        if event['conflict']:
            integration=Path(start['integration_workspace'])
            (integration/'handler.go').write_text(REFERENCE['handler.go']+'\n// resolved ordinary merge\n')
            for argv in [('add','handler.go'),('commit','-m','Resolve both contributions')]:
                subprocess.run(['git','-c','core.hooksPath=/dev/null','-c','user.name=Test','-c','user.email=test@example.invalid',*argv],cwd=integration,check=True,stdout=sys.stderr)
            commit=subprocess.check_output(['git','rev-parse','HEAD'],cwd=integration,text=True).strip()
            send(action='check',resolved=commit)
        elif event['passed']:
            send(action='ready',commit=own_final)
        else:
            raise AssertionError('resolved candidate failed')
    elif event['type']=='ok' and event.get('action')=='ready':
        threading.Event().wait()
    if agent=='A' and own_final and peer_final and not checking:
        checking=True
        send(action='check')
"""


if __name__ == '__main__':
    unittest.main()
