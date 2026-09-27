import unittest
from app_timing import summarize

class CausalTimingTests(unittest.TestCase):
    def test_join_uses_input_identity_not_generation(self):
        base = dict(session='s', generation=12, level=2, input=101, residency='resident')
        events = [dict(name='input', session='s', input=101, time=1.0),
                  dict(base, name='job_dequeue', time=1.01),
                  dict(base, name='callback_enqueue', time=1.02),
                  dict(base, name='drawable_presented', time=1.04)]
        result = summarize(dict(events=events, dropped=0))
        self.assertAlmostEqual(result['input_to_present_p50_ms'], 40)
        self.assertAlmostEqual(result['input_to_present_p95_ms'], 40)
        self.assertFalse(result['p01_complete'])  # fewer than 100 inputs

    def test_missing_presentation_stays_unavailable(self):
        result = summarize(dict(events=[], dropped=0))
        self.assertIsNone(result['input_to_present_p95_ms'])
        self.assertFalse(result['p01_complete'])

if __name__ == '__main__':
    unittest.main()
