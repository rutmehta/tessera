"""Guard the synchronous/worker boundary that previously ran L0 Upright on main."""
import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[4]

class PreparationBoundaryTests(unittest.TestCase):
    def test_settings_submission_does_not_prepare_residency(self):
        source = (ROOT / 'crates/tessera-ffi/src/develop.rs').read_text()
        submission = source.split('fn render(self: &Arc<Self>')[1].split('fn interactive_done')[0]
        self.assertNotIn('.can_render_resident(', submission)
        worker = source.split('impl Job for DevelopJob')[1].split('impl Drop for DevelopSession')[0]
        self.assertIn('.can_render_resident(', worker)
        self.assertIn('ctx.cancellation.check()?', worker)

if __name__ == '__main__':
    unittest.main()
