# B5-17c handoff: paint into alpha channels and Quick Mask

Branch `wp/B5-17c`, based on origin/main 5778da58. Does not include B5-17b and does not need its code.

## Commits

- 6a8d85a1 test: RED Rust tests, `crates/tessera-ffi/tests/document_channel_paint.rs` (9 tests)
- bbb19f58 ffi: the approved `// B5-17c` hunk in `crates/tessera-ffi/src/document/tools.rs`
- dfb7f26e mac: Swift target, precedence, live refresh, Channels click hook, self-test, Swift tests, bindings
- (this commit) docs: HANDOFF.md and ACCEPTANCE-STEPS.md (steps 491–499)

## What changed

- `tools.rs` (only inside `// B5-17c begin/end` markers):
  - New variant `StrokeTarget::Channel { id: u64 }`.
  - `begin_stroke` now does the layer lookup and the pixel/all lock check only for Pixels and Mask, through a
    `paintable` closure. The Channel arm does neither: it resolves the base raster through
    `PaintTarget::Channel(ChannelId(id)).raster(..)`, the way tessera-mcp does at `documents/mod.rs:794`, and
    passes `lock_alpha = false`. Clone Stamp and Healing Brush are refused on channels ("… cannot paint into a
    channel"), and nothing is recorded. The `layer` argument is ignored for channel targets.
  - Nothing else changed. `stroke_points` already returns the brush's canvas dirty rect, not the op's damage, so
    channel frames report the painted area even though the engine op returns an empty RGB damage rect. The
    `DocOp::PaintTiles` that `end_stroke` writes, and its history labels, are the same code path as before.
- TesseraCore:
  - `BrushStrokeTarget` is now `Hashable` with `case channel(UInt64)`. Its String raw value and `CaseIterable` are
    gone; nothing used them.
  - New `BrushStrokeTarget.resolve(quickMask:channel:layerKind:hasMask:paintMask:)`, the precedence rule, which
    is unit-tested.
  - `.ffi` in EngineDocumentBackend+Tools is now an exhaustive switch.
  - The stub still throws "needs the engine" for every stroke, channel strokes included. It does not record
    strokes. A test pins this.
- `DocumentTools.swift`: one `// B5-17 begin/end` block, plus three one-line `// B5-17` hooks (begin stroke,
  frame refresh, stroke end) and the self-test start in `attach`.
  - Stroke precedence: Quick Mask on, then a channel targeted by a plain panel click that is still
    highlighted, then the layer as before.
  - Status text "Painting into Quick Mask" or "Painting into <name>".
  - During a channel stroke, `DocumentChannels.reload(doc, force: true)` runs at most every 150 ms (this refreshes
    the thumbnail and overlay), and once more at the end of the stroke.
- `ChannelsPanel.swift`: one line in `click()`. A plain click (without ⌘) on a row calls
  `DocumentTools.targetChannel(row.channelID, in:)`. Clicking RGB or a colour row passes nil and returns painting
  to the layer. Painting is **not** redirected by highlights that code sets, such as Save Selection / New
  Channel setting `selectedChannel`. Otherwise saving a selection would silently redirect the next brush stroke.
- `ChannelPaintSelfTest.swift` (new): `--channel-paint-selftest=<dir>`, which runs steps 491–499 (see
  ACCEPTANCE-STEPS.md).
- Bindings regenerated (`TesseraFFI.swift`: +15 lines, the Channel case only; no header change). Re-run
  `./build-ffi.sh` at merge.

## Tests (A's list, all in `document_channel_paint.rs` unless noted)

- Exactly one undoable node per stroke; undo and redo are exact; live before the node:
  `channel_stroke_changes_only_the_channel_in_one_undoable_node`
- The selection limits the paint: `selection_limits_channel_paint`
- An unknown channel id fails with no node and no open stroke: `unknown_channel_fails_without_a_history_node`
- RGB composite unchanged (byte-identical PNG export), layer pixels unchanged, no mask created: the first test,
  plus both round trips
- Save/reopen round trips for .tessera-doc and PSD, including a flat-export composite check and painting again
  after reopen: `painted_channel_round_trips_as_{tessera_doc,psd}`
- Also covered: brush paints luminance, eraser goes toward 0; a locked layer and a bogus layer id do not block
  channel paint (while pixel paint is still refused); Clone/Heal are refused; Quick Mask enter → paint → exit
  gives the painted selection.
- Swift (`DocumentToolsTests`):
  - `testStrokeTargetPrecedence`: the precedence table plus `.ffi` mapping
  - `testEngineChannelStrokeThroughTheAdapter`: a real session, one node, revision moves, load gives the stroke,
    unknown id and clone refused
  - The stub assertion

## Gates (worktree root; CARGO_TARGET_DIR=~/.cache/tessera-target/B5-17c, CARGO_BUILD_JOBS=2)

- `cargo test --locked --release -p tessera-ffi`: all pass (38 test binaries, 0 failed)
- `cargo clippy --locked --release -p tessera-ffi --all-targets -- -D warnings`: clean
- `cargo fmt --all -- --check`: clean
- `tools/orchestrate/swift-gate.sh`: **SWIFT GATE OK** (705 tests, 3 skipped, 0 failures). The new tests were
  confirmed to run and pass with `--filter DocumentToolsTests` (17 tests).
- `xcodebuild -scheme Tessera -configuration Debug … -jobs 2 build`: BUILD SUCCEEDED
- `Support/make-app.sh debug`: built and verified at dfb7f26e

## Self-test: NOT run to completion

- `open -g -n build/Tessera.app --args --nonactivating --app-dir … --new-document --channel-paint-selftest=<dir>`
  never reached document mode in 3 minutes (no self-test output at all). This matches what B5-17b saw with
  `open -g -n`.
- Running the bundle executable directly with `--nonactivating --app-dir … --folder <empty> --new-document`, as
  B5-16's selftest_runner does, also produced no output in 6.5 minutes.
- In this session the **existing** `--vector-selftest` behaves the same way under identical launch arguments. So
  this looks like the environment (document view never appearing, perhaps a locked or asleep display), not
  B5-17c.
- To do on a machine where the other document self-tests run: launch with `--new-document
  --channel-paint-selftest=<dir>` (plus `--nonactivating --app-dir … --folder …`). Pre-create `<dir>/ack-01…30`
  or capture with `screencapture -l <window-id>` per step. Expect `done, 0 failure(s)`.

## Open issues / notes for the integrator

1. **Quick Mask entered with an active selection:** the engine selection stays active, so Quick Mask strokes are
   clipped to it and white cannot grow the selection beyond it. Photoshop drops the marching ants on entering
   Quick Mask. The fix belongs in `QuickMask.enter` / `DocumentChannels.toggleQuickMask` (Channels owner /
   B5-17b's files), for example by deselecting as part of entering. It is not in the stroke path, and A's rule
   "the selection limits channel paint" should stay. Quick Mask entered *without* a selection works, and is
   tested.
2. Clicking RGB clears the paint target but leaves the alpha row highlighted, because `selectedChannel` is
   unchanged; I kept ChannelsPanel to a single line. Consider clearing `selectedChannel` on composite and
   component clicks when B5-17b's Channels changes merge.
3. Merge: B5-17b edits DocumentChannels.swift and DocumentChannelsBackend.swift. B5-17c only reads
   `quickMask`, `records`, `selectedChannel`, `rows(_:)`, `reload(_:force:)`, `backend(_:)`, `newChannel()`,
   `toggleVisible(_:)`, and `toggleQuickMask()`, all of which exist on both sides. `TesseraFFI.swift`: take either
   side and re-run `./build-ffi.sh`.

## Remaining on-screen checks (no screen or computer use here)

- 491: the Quick Mask overlay while painting black/white, and the marching ants after Q-off.
- 492/497: the channel overlay and row thumbnail updating mid-drag, and the status text.
- 493–496: eraser, undo/redo and clipping, visible on the overlay; RGB click returning to layer paint; the mask
  auto-create on an adjustment layer.
- 498: reopen .tessera-doc and PSD and look at the channel. Photoshop opens the PSD's alpha channel with the
  painted strokes.
- 499: the composite looks unchanged throughout.
