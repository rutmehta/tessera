"""Structural regression guards for launch safety and deferred renderer setup."""
import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
APP = ROOT / 'apps/mac/Sources/Tessera'

class FoundationTests(unittest.TestCase):
    def test_background_launch_guards_activation_and_front(self):
        source = (APP / 'App/TesseraApp.swift').read_text()
        self.assertIn('if !nonactivating { NSApp.activate() }', source)
        self.assertIn('args.contains("--front"), !nonactivating', source)
        self.assertIn('nonactivating ? .accessory : .regular', source)

    def test_no_eager_renderer_or_source_compile_on_main(self):
        source = (APP / 'Loupe/MetalLoupeView.swift').read_text()
        self.assertNotIn('private let renderer = LoupeRenderer()', source)
        self.assertIn('private var renderer: LoupeRenderer?', source)
        renderer = (APP / 'Loupe/LoupeRenderer.swift').read_text()
        self.assertIn('Task.detached(priority: .userInitiated)', renderer)
        self.assertNotIn('@MainActor\nfinal class LoupeRenderer', renderer)

    def test_layout_test_never_fronts_window(self):
        source = (ROOT / 'apps/mac/Tests/TesseraCoreTests/PeopleLayoutTests.swift').read_text()
        self.assertNotIn('makeKeyAndOrderFront', source)

if __name__ == '__main__':
    unittest.main()
