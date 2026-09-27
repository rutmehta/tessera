import unittest
import importlib.util
from pathlib import Path
import sys
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location('timing', Path(__file__).with_name('app_timing.py'))
timing = importlib.util.module_from_spec(spec)
spec.loader.exec_module(timing)

class TimingTests(unittest.TestCase):
    def test_no_fabricated_input_latency_or_residency(self):
        result = timing.summarize({'events': [
            {'name': 'input', 'time': 1, 'session': 's', 'input': 1},
            {'name': 'ffi_end', 'durationMs': 3, 'mainThread': True},
            {'name': 'callback_enqueue', 'time': 2, 'session': 's', 'generation': 7, 'level': 2},
            {'name': 'drawable_presented', 'time': 2.1, 'session': 's', 'generation': 7, 'level': 2},
            {'name': 'drawable_presented', 'time': 2.2, 'session': 's', 'generation': 7, 'level': 2},
        ], 'dropped': 0})
        self.assertIsNone(result['input_to_present_p50_ms'])
        self.assertIsNone(result['input_to_present_p95_ms'])
        self.assertEqual(result['main_instrumented_span_p95_ms'], 3)
        self.assertEqual(result['presented_generations'], 1)
        self.assertAlmostEqual(result['callback_to_present_p95_ms'], 100)
        self.assertFalse(result['p01_complete'])

    def test_missing_presentation_is_not_zero(self):
        result = timing.summarize({'events': [], 'dropped': 0})
        self.assertIsNone(result['callback_to_present_p95_ms'])
        self.assertIsNone(result['main_instrumented_span_p95_ms'])

    def test_percentile_is_nearest_rank(self):
        self.assertEqual(timing.percentile(list(range(1, 101)), 0.95), 95)

if __name__ == '__main__':
    unittest.main()
