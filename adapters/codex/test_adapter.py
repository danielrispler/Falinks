import tempfile
import unittest
from pathlib import Path

from adapter import Boundary, SafetyError, ControlGate


class BoundaryTests(unittest.TestCase):
    def test_only_registered_runtime_identity_reaches_engine(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            source = root / 'source'
            source.mkdir()
            received = []
            boundary = Boundary(root / 'context.sqlite', source, 'session-a',
                                lambda envelope: received.append(envelope) or {'accepted': True})
            boundary.bind('thread-a', 'agent-a')
            boundary.begin('turn-a')
            call = {'threadId': 'thread-a', 'turnId': 'turn-a', 'callId': 'call-a',
                    'tool': 'falinks_edit', 'arguments': {'workspace': str(source),
                    'request': {'request_id': 'request-a', 'content': 'new'}}}
            self.assertTrue(boundary.dispatch(call)['accepted'])
            self.assertEqual(received[0]['identity'], {
                'agent': 'agent-a', 'session': 'session-a', 'thread': 'thread-a',
                'turn': 'turn-a', 'workspace': str(source)})
            for field, value in [('threadId', 'forged'), ('turnId', 'old-turn'),
                                 ('tool', 'shell')]:
                with self.assertRaises(SafetyError):
                    boundary.dispatch(dict(call, **{field: value}))
            for request in [{'author': 'agent-b'}, {'identity': {'agent': 'agent-b'}}]:
                with self.assertRaises(SafetyError):
                    boundary.dispatch(dict(call, arguments={'workspace': str(source), 'request': request}))
            with self.assertRaises(SafetyError):
                boundary.dispatch(dict(call, arguments={'workspace': str(root), 'request': {}}))
            boundary.end('turn-a')
            with self.assertRaises(SafetyError):
                boundary.dispatch(call)
            self.assertEqual(len(received), 1)
            boundary.close()

    def test_context_survives_presentation_deferral_and_resume(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            engine = lambda envelope: {'accepted': True}
            boundary = Boundary(root / 'context.sqlite', root, 'session-a', engine)
            boundary.bind('thread-a', 'agent-a')
            boundary.enqueue('E1', {'revision': 'R1', 'text': 'reread source'})
            boundary.enqueue('E1', {'revision': 'R1', 'text': 'reread source'})
            boundary.begin('turn-a')
            call = {'threadId': 'thread-a', 'turnId': 'turn-a', 'callId': 'call-a',
                    'tool': 'falinks_review', 'arguments': {'workspace': str(root),
                    'request': {'event': 'E1', 'action': 'acknowledge'}}}
            boundary.dispatch(call)
            self.assertEqual(boundary.pending()[0]['status'], 'pending')
            call['arguments']['request']['action'] = 'defer'
            boundary.dispatch(call)
            boundary.close()
            resumed = Boundary(root / 'context.sqlite', root, 'session-b', engine)
            with self.assertRaises(SafetyError):
                resumed.bind('thread-a', 'agent-b', resume=True)
            resumed.bind('thread-a', 'agent-a', resume=True)
            self.assertEqual(resumed.pending(), [{'event': 'E1',
                'context': {'revision': 'R1', 'text': 'reread source'}, 'status': 'deferred'}])
            resumed.begin('turn-b')
            call['turnId'] = 'turn-b'
            call['arguments']['request']['action'] = 'keep'
            resumed.dispatch(call)
            self.assertEqual(resumed.pending(), [])
            resumed.enqueue('E2', {'revision': 'R2'})
            resumed.enqueue('E1', {'revision': 'R1', 'text': 'reread source'})
            self.assertEqual([event['event'] for event in resumed.pending()], ['E2'])
            with self.assertRaises(SafetyError):
                resumed.enqueue('E2', {'revision': 'R3'})
            resumed.close()

    def test_missing_or_failed_fresh_controls_block_capabilities(self):
        gate = ControlGate()
        with self.assertRaises(SafetyError):
            gate.require_supported()
        for control in ControlGate.REQUIRED:
            gate.record(control, True)
        gate.require_supported()
        gate.record('source_write_denial', False)
        with self.assertRaises(SafetyError):
            gate.require_supported()
        gate.record('source_write_denial', True)
        with self.assertRaises(SafetyError):
            gate.require_supported()
        with self.assertRaises(SafetyError):
            ControlGate().require_supported()

    def test_unknown_change_preserves_source_and_stops_engine_operations(self):
        from check_adapter import ControlledHost
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            (root / 'source').mkdir()
            (root / 'controller').mkdir()
            target = root / 'source/source.txt'
            target.write_text('original\n')
            host = ControlledHost(root)
            boundary = Boundary(root / 'context.sqlite', root / 'source', 'session', host)
            boundary.bind('thread', 'agent')
            boundary.begin('turn')
            target.write_text('external work\n')
            call = {'threadId': 'thread', 'turnId': 'turn', 'callId': 'call',
                    'tool': 'falinks_offer', 'arguments': {'workspace': str(root / 'source'),
                    'request': {'expected_hash': 'anything'}}}
            with self.assertRaises(SafetyError):
                boundary.dispatch(call)
            self.assertEqual(target.read_text(), 'external work\n')
            # Restoring old bytes cannot silently reconcile unknown history.
            target.write_text('original\n')
            with self.assertRaises(SafetyError):
                boundary.dispatch(call)
            host.close()
            boundary.close()

    def test_unknown_directory_changes_stop_the_controlled_host(self):
        from check_adapter import ControlledHost
        for change in ('empty_directory', 'directory_mode', 'root_alias'):
            with self.subTest(change=change), tempfile.TemporaryDirectory() as directory:
                root = Path(directory).resolve()
                source = root / 'source'
                source.mkdir()
                (root / 'controller').mkdir()
                (source / 'source.txt').write_text('original\n')
                host = ControlledHost(root)
                if change == 'empty_directory':
                    (source / 'unexpected').mkdir()
                elif change == 'directory_mode':
                    source.chmod(source.stat().st_mode ^ 0o100)
                else:
                    source.rename(root / 'original-source')
                    source.symlink_to(root / 'original-source', target_is_directory=True)
                with self.assertRaises(SafetyError):
                    host.integrity()
                host.close()


if __name__ == '__main__':
    unittest.main()
