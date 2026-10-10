import concurrent.futures
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import lore_record


class RecordingCheckpointTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix='lore-record-test-')
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.event = {
            'hook_event_name': 'Stop',
            'session_id': 'session-a',
            'turn_id': 'turn-a',
            'cwd': str(self.root),
            'stop_hook_active': False,
            'last_assistant_message': 'A private message not intended for hook output.',
        }
        self.addCleanup(patch.stopall)
        patch('lore_record.home', return_value=self.root / 'home').start()
        patch('lore_record.git_root', return_value=self.root).start()
        self.run = patch('lore_record.run', return_value={'workspace': {}}).start()

    def test_requests_once_per_turn_and_allows_later_turns(self):
        result = lore_record.handle(self.event)
        self.assertEqual(result['decision'], 'block')
        self.assertNotIn(self.event['last_assistant_message'], result['reason'])
        self.assertEqual(lore_record.handle(self.event), {})
        self.assertEqual(lore_record.handle({**self.event, 'turn_id': 'turn-b'})['decision'], 'block')

    def test_reentry_never_requests_a_checkpoint(self):
        self.assertEqual(lore_record.handle({**self.event, 'stop_hook_active': True}), {})
        self.run.assert_not_called()

    def test_concurrent_duplicate_events_emit_one_request(self):
        with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
            results = list(pool.map(lore_record.handle, [self.event] * 8))
        self.assertEqual(sum(result.get('decision') == 'block' for result in results), 1)

    def test_sessions_are_independent(self):
        lore_record.handle(self.event)
        result = lore_record.handle({**self.event, 'session_id': 'session-b'})
        self.assertEqual(result['decision'], 'block')

    def test_uninitialized_workspace_does_not_trigger_setup(self):
        self.run.side_effect = RuntimeError('not initialized')
        self.assertEqual(lore_record.handle(self.event), {})
        self.assertEqual(self.run.call_args.args, (self.root, 'status'))
        self.assertFalse((self.root / 'home').exists())

    def test_missing_cli_and_timeout_do_not_request_continuation(self):
        for failure in (FileNotFoundError(), subprocess.TimeoutExpired('lore', 2)):
            with self.subTest(failure=failure):
                self.run.side_effect = failure
                self.assertEqual(lore_record.handle(self.event), {})

    def test_events_without_stable_identity_or_checkout_are_skipped(self):
        for key in ('session_id', 'turn_id', 'cwd'):
            for value in (None, '', 42):
                with self.subTest(key=key, value=value):
                    self.assertEqual(lore_record.handle({**self.event, key: value}), {})
        self.assertEqual(lore_record.handle({**self.event, 'hook_event_name': 'PostToolUse'}), {})
        self.run.assert_not_called()

    def test_no_git_checkout_is_skipped(self):
        with patch('lore_record.git_root', return_value=None):
            self.assertEqual(lore_record.handle(self.event), {})
        self.run.assert_not_called()

    def test_recording_policy_is_replaceable(self):
        skill = self.root / 'custom policy.md'
        skill.write_text('Record according to the local policy.\n')
        result = lore_record.handle(self.event, skill)
        self.assertIn(json.dumps(str(skill)), result['reason'])

    def test_missing_policy_does_not_claim_a_checkpoint(self):
        with self.assertRaises(ValueError):
            lore_record.handle(self.event, self.root / 'missing.md')
        self.assertEqual(lore_record.handle(self.event)['decision'], 'block')

    def test_malformed_input_keeps_stop_available(self):
        for payload in ('not json', '[]', 'x' * (1024 * 1024 + 1)):
            with self.subTest(size=len(payload)):
                result = subprocess.run(
                    [sys.executable, str(Path(lore_record.__file__))],
                    input=payload, capture_output=True, text=True,
                    env={**os.environ, 'LORE_HOME': str(self.root / 'home')},
                )
                self.assertEqual(result.returncode, 0)
                self.assertEqual(json.loads(result.stdout), {})
                self.assertFalse((self.root / 'home').exists())


if __name__ == '__main__':
    unittest.main()
