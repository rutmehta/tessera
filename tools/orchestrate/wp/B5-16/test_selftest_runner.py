#!/usr/bin/env python3
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch
from selftest_runner import validate_log, run_test, enforce_resource_hold, TESTS, main


class SelfTestRunnerTests(unittest.TestCase):
    def test_exact_success(self):
        self.assertEqual(validate_log('document-selftest: check opened ok\ndocument-selftest: done, 0 failure(s)\n', 'document-selftest', 0), [])

    def test_failures_cannot_hide_behind_zero_summary(self):
        for failure in ['check saved FAIL', 'FAIL library did not load', 'Warp did not start']:
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
        self.assertEqual(TESTS['transform'], (1800, ['--transform-selftest=@OUT@']))

    def test_resource_hold_presence_blocks_even_without_valid_json(self):
        with tempfile.TemporaryDirectory() as temp, patch('selftest_runner.Path.home', return_value=Path(temp)):
            enforce_resource_hold()
            hold = Path(temp) / '.local/state/tessera-resource-hold.json'
            hold.parent.mkdir(parents=True)
            for content in ['{"reason":"resource audit"}', '', 'not-json']:
                hold.write_text(content)
                with self.assertRaisesRegex(RuntimeError, 'workload hold is active'):
                    enforce_resource_hold()
                self.assertEqual(hold.read_text(), content)

    def test_resource_hold_blocks_main_before_dispatch_or_process(self):
        with tempfile.TemporaryDirectory() as temp, patch('selftest_runner.Path.home', return_value=Path(temp)):
            root = Path(temp)
            binary = root / 'apps/mac/build/Tessera.app/Contents/MacOS/Tessera'
            binary.parent.mkdir(parents=True); binary.touch()
            fixture = root / 'fixtures/raw/sample.dng'
            fixture.parent.mkdir(parents=True); fixture.touch()
            hold = root / '.local/state/tessera-resource-hold.json'
            hold.parent.mkdir(parents=True); hold.write_text('{}')
            for requested in [[], ['transform']]:
                with self.subTest(requested=requested), patch('selftest_runner.run_test') as run, \
                     patch('selftest_runner.subprocess.Popen') as process, \
                     patch('selftest_runner.subprocess.run') as capture, \
                     patch('selftest_runner.tempfile.mkdtemp') as scratch, patch('sys.stderr') as stderr:
                    with self.assertRaises(SystemExit) as error:
                        main(['--root', str(root), *requested])
                    self.assertEqual(error.exception.code, 2)
                    self.assertIn('workload hold is active', ''.join(str(call.args[0]) for call in stderr.write.call_args_list))
                    run.assert_not_called(); process.assert_not_called()
                    capture.assert_not_called(); scratch.assert_not_called()

    def test_resource_hold_blocks_direct_run_test_before_staging(self):
        with tempfile.TemporaryDirectory() as temp, patch('selftest_runner.Path.home', return_value=Path(temp)):
            root = Path(temp)
            hold = root / '.local/state/tessera-resource-hold.json'
            hold.parent.mkdir(parents=True); hold.write_text('{}')
            with patch('selftest_runner.subprocess.Popen') as process, patch('selftest_runner.subprocess.run') as capture:
                with self.assertRaisesRegex(RuntimeError, 'workload hold is active'):
                    run_test(root / 'unused', 'transform', TESTS['transform'][1], 1800,
                             root / 'scratch', root / 'evidence', [])
                process.assert_not_called(); capture.assert_not_called()
                self.assertFalse((root / 'scratch').exists())
                self.assertFalse((root / 'evidence').exists())

    def test_hold_appearing_during_staging_blocks_child(self):
        with tempfile.TemporaryDirectory() as temp, patch('selftest_runner.Path.home', return_value=Path(temp)):
            root = Path(temp)
            hold = root / '.local/state/tessera-resource-hold.json'
            hold.parent.mkdir(parents=True)
            scratch = root / 'scratch'; scratch.mkdir()
            def raise_hold(*_args):
                hold.write_text('{}')
            with patch('selftest_runner.shutil.copy2', side_effect=raise_hold), \
                 patch('selftest_runner.subprocess.Popen') as process, patch('selftest_runner.subprocess.run') as capture:
                with self.assertRaisesRegex(RuntimeError, 'workload hold is active'):
                    run_test(root / 'unused', 'transform', TESTS['transform'][1], 1800,
                             scratch, root / 'evidence', [root / 'fixture'])
                process.assert_not_called(); capture.assert_not_called()

    def test_hold_between_cases_stops_remaining_dispatch(self):
        with tempfile.TemporaryDirectory() as temp, patch('selftest_runner.Path.home', return_value=Path(temp)):
            root = Path(temp)
            binary = root / 'apps/mac/build/Tessera.app/Contents/MacOS/Tessera'
            binary.parent.mkdir(parents=True); binary.touch()
            fixture = root / 'fixtures/raw/sample.dng'
            fixture.parent.mkdir(parents=True); fixture.touch()
            hold = root / '.local/state/tessera-resource-hold.json'
            hold.parent.mkdir(parents=True)
            def finish_first(*_args):
                hold.write_text('{}')
                return []
            with patch.dict(os.environ, {'SP': str(root / 'scratch'), 'EV': str(root / 'evidence')}), \
                 patch('selftest_runner.run_test', side_effect=finish_first) as run, \
                 patch('selftest_runner.subprocess.Popen') as process, patch('sys.stderr'), patch('builtins.print'):
                with self.assertRaises(SystemExit) as error:
                    main(['--root', str(root), 'document', 'transform'])
                self.assertEqual(error.exception.code, 2)
                self.assertEqual(run.call_count, 1)
                self.assertEqual(run.call_args.args[1], 'document')
                process.assert_not_called()

    def test_default_suite_and_failure_exit_from_another_checkout(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binary = root / 'apps/mac/build/Tessera.app/Contents/MacOS/Tessera'
            binary.parent.mkdir(parents=True); binary.touch()
            fixture = root / 'fixtures/raw/sample.dng'
            fixture.parent.mkdir(parents=True); fixture.touch()
            with patch('selftest_runner.Path.home', return_value=root), \
                 patch.dict(os.environ, {'SP': str(root / 'scratch'), 'EV': str(root / 'evidence')}), \
                 patch('selftest_runner.run_test', return_value=['fixture failure']) as run, patch('builtins.print'):
                self.assertEqual(main(['--root', str(root)]), 1)
                self.assertEqual(run.call_count, len(TESTS))
                self.assertEqual(run.call_args.args[0], binary.resolve())

    def test_real_child_success_crash_and_timeout_preserve_other_process(self):
        with tempfile.TemporaryDirectory() as temp, patch('selftest_runner.Path.home', return_value=Path(temp)):
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
