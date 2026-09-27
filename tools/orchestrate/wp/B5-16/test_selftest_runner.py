#!/usr/bin/env python3
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch
from selftest_runner import validate_log, run_test, capture_requests, TESTS, main


class SelfTestRunnerTests(unittest.TestCase):
    def test_exact_success(self):
        self.assertEqual(validate_log('document-selftest: check opened ok\ndocument-selftest: done, 0 failure(s)\n', 'document-selftest', 0), [])

    def test_failures_cannot_hide_behind_zero_summary(self):
        for failure in ['check saved FAIL', 'FAIL library did not load', 'Warp did not start', 'shot example (no watcher)']:
            with self.subTest(failure=failure):
                self.assertTrue(validate_log(f'document-selftest: {failure}\ndocument-selftest: done, 0 failure(s)\n', 'document-selftest', 0))

    def test_missing_duplicate_nonzero_and_wrong_prefix(self):
        for text in ['', 'document-selftest: done, 1 failure(s)',
                     'tools-selftest: done, 0 failure(s)',
                     'document-selftest: done, 0 failure(s)\ndocument-selftest: done, 0 failure(s)']:
            self.assertTrue(validate_log(text, 'document-selftest', 0))

    def test_process_failure_or_timeout_cannot_pass(self):
        done = 'document-selftest: done, 0 failure(s)'
        self.assertTrue(validate_log(done, 'document-selftest', 9))
        self.assertTrue(validate_log(done, 'document-selftest', 0, timed_out=True))

    def test_transform_is_in_default_suite(self):
        self.assertIn('transform', TESTS)

    def test_requested_window_capture_acknowledges_only_success(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            output = root / 'output'; output.mkdir()
            evidence = root / 'evidence'; evidence.mkdir()
            (output / '381-warp.req').write_text('123')
            seen = set()
            def capture(command, **kwargs):
                self.assertEqual(command[:5], ['screencapture', '-x', '-o', '-l', '123'])
                Path(command[-1]).write_bytes(b'captured window')
                return subprocess.CompletedProcess(command, 0, '', '')
            with patch('selftest_runner.subprocess.run', side_effect=capture) as run:
                self.assertEqual(capture_requests(output, evidence, seen), [])
                self.assertEqual((output / '381-warp.png').read_bytes(), b'captured window')
                self.assertEqual(capture_requests(output, evidence, seen), [])
                self.assertEqual(run.call_count, 1)
            (output / '382-failed.req').write_text('123')
            with patch('selftest_runner.subprocess.run', return_value=subprocess.CompletedProcess([], 1, '', 'capture failed')):
                self.assertTrue(capture_requests(output, evidence, seen))
                self.assertFalse((output / '382-failed.png').exists())
            (output / '383-invalid.req').write_text('not a window')
            with patch('selftest_runner.subprocess.run') as run:
                self.assertTrue(capture_requests(output, evidence, seen))
                run.assert_not_called()

    def test_default_suite_and_failure_exit_from_another_checkout(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binary = root / 'apps/mac/build/Tessera.app/Contents/MacOS/Tessera'
            binary.parent.mkdir(parents=True); binary.touch()
            fixture = root / 'fixtures/raw/sample.dng'
            fixture.parent.mkdir(parents=True); fixture.touch()
            with patch.dict(os.environ, {'SP': str(root / 'scratch'), 'EV': str(root / 'evidence')}), \
                 patch('selftest_runner.run_test', return_value=['fixture failure']) as run, patch('builtins.print'):
                self.assertEqual(main(['--root', str(root)]), 1)
                self.assertEqual(run.call_count, len(TESTS))
                self.assertEqual(run.call_args.args[0], binary.resolve())

    def test_real_child_success_crash_and_timeout_preserve_other_process(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            unrelated = subprocess.Popen(['sleep', '15'])
            try:
                for index, (code, timeout, failed) in enumerate([
                    ('import sys; print("document-selftest: done, 0 failure(s)", file=sys.stderr)', 3, False),
                    ('import sys; print("document-selftest: done, 0 failure(s)", file=sys.stderr); sys.exit(7)', 3, True),
                    ('import time; time.sleep(15)', 0.2, True),
                    ('pass', 3, True),
                ]):
                    binary = root / f'fake-{index}'
                    binary.write_text('#!/usr/bin/env python3\n' + code + '\n')
                    binary.chmod(0o755)
                    scratch = root / f'scratch-{index}'; scratch.mkdir()
                    errors = run_test(binary, 'document', [], timeout, scratch, root / f'evidence-{index}', [])
                    self.assertEqual(bool(errors), failed, errors)
                    self.assertIsNone(unrelated.poll(), 'runner must not terminate another process')
            finally:
                unrelated.terminate(); unrelated.wait()


if __name__ == '__main__':
    unittest.main()
