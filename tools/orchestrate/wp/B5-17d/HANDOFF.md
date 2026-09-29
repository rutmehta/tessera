# B5-17d handoff: Quick Mask with a selection, channel highlight on RGB click

Branch `wp/B5-17d`, based on origin/main dd2cf300 (B5-17b and B5-17c merged). Follow-ups 1 and 2 from
`tools/orchestrate/wp/B5-17c/HANDOFF.md`.

## Commits

- 8817f805 test: RED Rust tests, `crates/tessera-ffi/tests/document_quick_mask.rs` (4 tests)
- 8bfe3b01 ffi: `enter_quick_mask` / `exit_quick_mask` in `crates/tessera-ffi/src/document/channels.rs`
- 679b977d mac: backend calls, `QuickMask`, Channels panel click model, Swift tests, bindings
- (this commit) docs: this handoff

## What changed

1. Quick Mask entered with an active selection (Photoshop behaviour)
   - FFI `enter_quick_mask(name) -> ChannelUpdate`: the selection becomes the new alpha channel and the
     selection is dropped, as one `DocOp::Batch[AddChannel, SetSelection(None)]`. That is one history node
     labelled "Quick Mask". With no selection, the channel is all white and no SetSelection is added, so this
     path behaves as before. The channel is made visible (session state).
   - FFI `exit_quick_mask(id) -> DocumentUpdate`: one `Batch[SetSelection(mask), DeleteChannel]`, also
     labelled "Quick Mask". An empty mask deselects. An unknown id fails without creating a node.
   - History: one node to enter and one to exit, with the strokes in between. The old exit path was
     Load Selection + Delete Channel, which made two nodes. Undoing the enter restores the selection and
     removes the channel in one step.
   - `add_channel` now wraps a new `add_channel_with(channel, more_ops, label)`. The other callers are
     unchanged.
   - Swift: `DocumentChannelsBackend` gains `enterQuickMask(name:)` and `exitQuickMask(id:)`, implemented
     on the engine (`EngineDocumentBackend+Channels`) and on the stub (the stub clears its rect selection).
     `QuickMask.enter(_:)` no longer takes `hasSelection:`, because the backend reads the selection itself.
     `DocumentChannels.toggleQuickMask` is updated to match. The rule "the selection limits channel paint"
     is untouched: there is simply no selection while Quick Mask is on.
2. Channel highlight follows the paint target
   - New pure `ChannelsPanelModel.click(row, command:) -> ChannelRowClick?`:
     - Plain click on a saved channel: highlight it and paint into it.
     - Plain click on RGB or a colour row: clear the highlight and paint into the layer.
     - ⌘-click on a saved channel: highlight it and load it as the selection, with paint not redirected.
     - ⌘-click on RGB or a colour row: nothing.
   - `ChannelsPanel.click()` uses it, so `selectedChannel` is set to nil on RGB and colour clicks.
   - `ChannelsSelfTest` step 5 adds a check: "quick mask on drops the selection".

## Tests

- Rust, `document_quick_mask.rs`:
  - `entering_with_a_selection_moves_it_into_the_mask_in_one_node`: one node, selection dropped, mask equals
    the old selection, visible; undo restores it.
  - `painting_white_outside_the_old_selection_grows_it_after_exit`: the stroke is not clipped; exit is one
    node and gives the union; undo and redo work.
  - `entering_without_a_selection_is_unchanged`
  - `exiting_an_unknown_channel_fails_without_a_node`
- Swift:
  - `DocumentChannelsTests.testQuickMaskEnterExitOnTheStub`: updated; entering drops the selection.
  - `DocumentChannelsTests.testEngineChannelsThroughTheAdapter`: updated; one "Quick Mask" node for enter
    and one for exit.
  - `DocumentChannelsTests.testRowClickHighlightMatchesThePaintTarget`: new.
  - `DocumentToolsTests.testQuickMaskWithASelectionGrowsByPainting`: new; engine, Quick Mask target via
    `BrushStrokeTarget.resolve`, a white stroke outside the selection grows it.

## Gates (CARGO_TARGET_DIR=~/.cache/tessera-target/B5-17d)

- `cargo test --locked --release -p tessera-ffi --no-fail-fast`: 46 binaries, 0 failing.
  - One earlier run had a single failure in the lib unit test
    `smart_preview_thumbnail::tests::hdr_saved_offline_recipe_keeps_policy_and_renders_sdr_thumbnail_without_mutation`.
    It is unrelated to this package (smart previews) and passed alone and in the full rerun, so it looks flaky.
- `cargo clippy --locked --release -p tessera-ffi --all-targets -- -D warnings`: clean
- `cargo fmt --all -- --check`: clean
- `apps/mac/build-ffi.sh`: OK. The bindings `TesseraFFI.swift` and `CTesseraFFI.h` gain only the two new
  calls. Re-run at merge if another FFI package lands first.
- `tools/orchestrate/swift-gate.sh`: **SWIFT GATE OK** (777 tests, 3 skipped, 0 failures). The new and
  updated tests were confirmed to run with `--filter`.

## Not done / notes

- Exiting an untouched Quick Mask that was entered without a selection still gives a select-all selection,
  the same as before this package. Photoshop gives no selection in that case. Changing it would mean an
  "all white means deselect" rule in `exit_quick_mask`.
- No self-test run and no screen use in this session.

## On-screen checks (someone with a screen)

1. Make a rectangular marquee on the left half and press Q. The marching ants disappear and the red overlay
   covers the right half. The History panel shows one "Quick Mask" row.
2. Paint white with the brush across the right half. The overlay clears where you paint, and it is not
   clipped at the old marquee edge.
3. Press Q again. The ants show the rectangle plus the painted band. History shows one more "Quick Mask" row.
4. Undo walks back one row at a time: exit, then stroke, then enter (with the original rectangle back).
5. Q with no selection: the overlay is clear. Paint black, press Q, and the painted band is excluded from the
   selection, as before.
6. Channels panel: click an alpha channel row. It highlights and the status reads "Painting into <name>".
   Click RGB, then Red. The alpha row is no longer highlighted and brush strokes go to the layer.
   ⌘-click an alpha row: it highlights and loads as the selection, and brush strokes still go to the layer.
7. `--channels-selftest`: step 5 also logs "quick mask on drops the selection (B5-17d)" as ok.
