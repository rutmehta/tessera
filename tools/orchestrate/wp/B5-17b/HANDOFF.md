# B5-17b handoff: persisted alpha channel display

Branch `wp/B5-17b`, based on origin/main aec3738e (main had not moved at push time).

## Commits

- 4aa3296d test(B5-17b): RED tests (Rust `document_channels_ui.rs`, Swift `DocumentChannelsTests.swift`)
- a3b64651 feat(B5-17b): implementation
- (this commit) docs: HANDOFF.md and ACCEPTANCE-STEPS.md (steps 485–490)

## What changed

- `crates/tessera-ffi/src/document/channels.rs`: `set_alpha_channel_display(id, color, opacity, selected_areas)`,
  one "Channel Options" node via `edit_channel`. Writes `ChannelKind::AlphaDisplay`, or plain `Alpha` when the
  values equal the legacy red / 0.5 / masked default. Rejects non-finite or out-of-range values before recording
  anything. Also turns a spot channel back into an alpha channel. The read path (`ChannelRecord.selected_areas`,
  colour, opacity) is M5-32's; its `// M5-32` hunks are unchanged.
- TesseraCore `Channels/`: `SavedChannel.selectedAreas` and `overlayStyle`, derived from the record. Protocol
  `setAlphaChannelDisplay` on the engine and the stub. New `ChannelIndicates` (Masked / Selected / Spot Color),
  `ChannelOptionsForm` (plans at most one display call and ignores colour-well round-trip noise) and
  `ChannelDisplayEdit`. `ChannelsPanelModel.rows` no longer takes a `styles:` map.
- App `Channels/` (DocumentChannels, ChannelSheets, ChannelOverlay, ChannelsSelfTest; ChannelsPanel.swift not touched):
  - The session `styles` map is gone.
  - `applyOptions(id, form)` is a rename plus at most one display edit, both in History.
  - The Channel Options sheet now has one "Color Indicates" radio group with Masked Areas, Selected Areas and Spot
    Color (`document.channels.options.indicates`). The `document.channels.options.kind` segmented control is gone.
  - The overlay reads `record.overlayStyle`.
  - `ChannelsSelfTest` now also checks that an options change is one node, that undo and redo restore it, and that
    the settings survive a .tessera-doc and PSD reopen.
- Stub fix (in scope, `StubDocumentBackend+Channels.swift`): `StubChannelStore` keyed only by `ObjectIdentifier`.
  A new stub document allocated at a freed one's address inherited its channels. The store now keeps a weak owner
  and starts fresh on a mismatch. The new stub test exposed this.
- Regenerated bindings (`CTesseraFFI.h`, `TesseraFFI.swift`). Re-run `./build-ffi.sh` at merge.
- Stub channels remain session state, not stub history, as they were for B5-08. The brief's "stub stores it in stub
  history" is not done; the stub round trip is covered instead.

## Gates (worktree root; CARGO_TARGET_DIR=~/.cache/tessera-target/B5-17b, CARGO_BUILD_JOBS=2)

- `cargo test --locked --release -p tessera-ffi`: all pass. `document_channels_ui` has 17 tests, 6 of them new.
- `cargo clippy --locked --release -p tessera-ffi --all-targets -- -D warnings`: clean
- `cargo fmt --all -- --check`: clean
- `tools/orchestrate/swift-gate.sh`: **SWIFT GATE OK** (707 tests, 3 skipped, 0 failures)
- `xcodebuild -scheme Tessera … -jobs 2 build`: BUILD SUCCEEDED
- Channels self-test, on the debug bundle from `Support/make-app.sh debug`:
  - Command: `TESSERA_CHANNELS_SELFTEST=<dir> Tessera --nonactivating --new-document …`
  - Result: `done, 0 failure(s)`, including all new `alpha options: …` and `reopen <ext> keeps the alpha display` checks.
  - How it was launched: the same way as B5-16's selftest_runner, i.e. the bundle executable run directly, with
    `--nonactivating` and no activation. With `open -g -n --env …` on this build, the document view never appeared
    and the test sat at launch with no output. That was tried twice.

## Remaining on-screen checks (not done: no screen or computer use)

- 485/486: the look of the sheet's radio group and colour well; the overlay tint polarity when switching between
  Masked and Selected Areas; the row chip colour.
- 487: open the saved PSD in Photoshop and check Channel Options (Selected Areas, green, 30 %).
- 488: needs a Photoshop-authored PSD with a non-default alpha display. There is no fixture for this; the engine
  import path is M5-32's.
- Docs to fix at merge: B5-08 IMPLEMENTATION-STATUS limitation 5, and the ACCEPTANCE.md line near l.1488
  ("colour / opacity … are session preferences, not saved"). Both are now stale for alpha overlay settings. Channel
  visibility is still session state.
