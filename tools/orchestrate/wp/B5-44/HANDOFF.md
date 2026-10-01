# B5-44 — Keyboard shortcut integrity

Branch: `wp/B5-44`. Local commits only; no board or lockfile changes.

## Change

Added `ShortcutIntegrityTests` and the standard-library-only Python source audit it invokes.
The audit walks `AppCommands` and recursively referenced SwiftUI menu views, enumerates
KeyRouter and document tool/fallback bindings, checks collisions by mode/context, and
compares the generated reference against `docs/shortcuts.md`. Unsupported menu shortcut
syntax fails rather than being silently skipped. The test also exercises the real tool
cycle groups and sends every reserved macOS chord through KeyRouter in Library and
Document modes and through both core key maps.

The menu audit counts registered key equivalents, including disabled items and diagnostic
commands. Disabled items are not treated as having relinquished their key equivalent.
Native macOS menu commands retain their system bindings; reservations prohibit custom
commands from taking them. This uses the requested source-enumeration option, not a hosted
SwiftUI menu walk. No live menu behavior or keyboard layout beyond the source key map is claimed.

Intentional reuse is documented: Library/Document modes; Compare, Review and Photo Edit
contexts; tool cycle groups; contextual Return/Escape/Delete; Shift-J Remove before Healing;
and Library Command-E's menu/router alias. The generated reference includes delegated
routing guards so conditional/consumed bindings cannot silently become undocumented.

## RED and separate fixes

The first test commit was run against the unchanged production sources and reported:

```text
Enumerated 63 menu bindings; 11 routing sources; 16 reserved chords
Duplicate Library ⇧⌘B: AppCommands / Run Grid Scroll Benchmark <> ImageMenu / color
Duplicate Library ⇧⌘N: AppCommands / Load 20,000 Stub Items <> LayerMenu / Layer
Undocumented or stale shortcuts: regenerate docs/shortcuts.md from the enumeration
```

1. New Layer now uses `.shortcut(doc != nil, ...)`: relinquish Shift-Command-N in Library so the diagnostic loader is its sole owner.
2. Auto Color now uses `.shortcut(doc != nil, ...)`: relinquish Shift-Command-B in Library so the scroll benchmark is its sole owner.

Each production fix is one line in its own commit. Both existing command chords are retained
in their intended modes. No custom reserved-system collision was found.

## Verification

- `python3 tools/orchestrate/shortcut-audit.py`: PASS; 63 menu bindings, 395 routed entries
  (including modifier aliases), 11 routing sources, 16 reserved chords.
- Temporary-source mutation checks: PASS for injected Command-Q, Library X/menu overlap,
  and Document Quick Mask Q/menu overlap; all 16 reserved combinations rejected.
- `apps/mac/build-ffi.sh` with `CARGO_TARGET_DIR=$HOME/.cache/tessera-target/B5-44`: PASS.
- Focused release `ShortcutIntegrityTests`: PASS, 3 tests, 0 failures (exit 0).
- Required `tools/orchestrate/swift-gate.sh`: **SWIFT GATE OK** (exit 0).
  894 XCTest tests, 3 skipped, 0 failures; 5 Swift Testing tests in 2 suites passed.
  No window-capture failure or locked-screen exception was needed.
- Rust source unchanged; crate test/clippy/fmt gates do not apply.

Regenerate documentation with `python3 tools/orchestrate/shortcut-audit.py --write-doc`.
The generator refuses to write while shortcut conflicts remain. Normal checks never write.
