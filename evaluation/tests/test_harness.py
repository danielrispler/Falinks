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


if __name__ == '__main__':
    unittest.main()
