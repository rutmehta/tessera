# B5-18 handoff — Camera Raw Filter sheet (steps 420–439)

Branch `wp/B5-18`, linear on origin/main `c15dee24` (merge base; `aec3738e` in earlier revisions was stale). Swift only; no Rust, FFI, bindings, board.json or Cargo.lock change.
`crates/tessera-ffi/src/document/filters.rs` untouched (B5-13 / B5-15 own it).

## What landed
- `apps/mac/Sources/TesseraCore/Document/CameraRaw/CameraRawDraft.swift` — `CameraRawFilter` (id, title,
  target/selection refusal, AI mask kinds), `CameraRawControls` (Basic sliders on `DevelopParameter` paths,
  split points), `CameraRawPanel` (Basic, Curve, HSL, Color Grading, Detail, Effects built from
  `DevelopControls.swift`), `CameraRawDraft` (settings JSON + amount; clamping; Custom WB when Temp/Tint move,
  6500 K/0 start; neutral base turns off DevelopSettings' raw defaults sharpening 40 / colour NR 25 so the neutral
  filter is identity on rendered pixels; re-edit parse keeps settings the sheet does not show; AI mask refusal).
- `apps/mac/Sources/Tessera/Document/CameraRaw/CameraRawSheet.swift` — `DocumentCameraRaw` (open / re-edit / apply
  off-main with busy + Cancel via `cancelFilter`), `CameraRawSheetModel` (canvas preview with 120 ms drag
  coalescing, release immediate; 1:1 detail pane latest-wins via `LatestRequestBuffer`; Show before; Amount;
  Reset), views, `CameraRawMenuItem`, `CameraRawSheets` modifier.
- `CameraRawSelfTest.swift` — `--camera-raw-selftest <dir>` (needs `--new-document` in a background launch).
- Hooks (delimited `B5-18` comments): FilterMenus.swift (menu item after Neural Filters), SmartFilterRows.swift
  (camera_raw rows re-open this sheet), DocumentView.swift (sheet modifier), AppCommands.swift (Develop ▸ Auto
  Edit… keeps ⇧⌘A only outside document mode; Camera Raw Filter… takes ⇧⌘A in documents).
- Acceptance text: `ACCEPTANCE-STEPS.md` (to merge into apps/mac/ACCEPTANCE.md by the integrator).

## Gates
- `tools/orchestrate/swift-gate.sh`: SWIFT GATE OK — 719 XCTest tests, 3 skipped, 0 failures (+5 swift-testing).
- `swift test --filter DocumentCameraRawTests`: 16/16 (draft paths for all 76 controls, clamping, neutral,
  WB, amount, splits, re-edit round trip, refusals, latest-wins; engine: neutral identity and exposure +1 = ×2,
  out-of-domain / unknown outer key rejected with no history, apply = one undoable node, cancel = no history,
  smart object append + re-edit replace (no duplicate) + native save/reopen round trip, selection and AI refusals).
- `Support/make-app.sh debug` + `codesign --verify --deep --strict`: OK.
- Background self-test (`open -g -n … --new-document --camera-raw-selftest <dir> --nonactivating`) on
  sample.dng (5212 × 3468, 16-bit): 0 failures; full-resolution apply 13.8 s. Log: `evidence/camera-raw-selftest.log`.
- cargo gates not run (no Rust touched). The brief's Rust test `crates/tessera-ffi/tests/document_camera_raw_ui.rs`
  was not added; the same behaviours are covered through the real FFI by the Swift engine tests above.

## Not verified
- No screenshots (steps print window rects for `screencapture -R`, but capturing was skipped to stay off the
  user's screen). Visual layout at 1440 pt, both appearances, and keyboard/VoiceOver pass are open.
- Preview latency on a 24 MP layer was not measured (every camera_raw preview is a full-canvas level-0 CPU render;
  expect seconds per frame — see follow-up 1). GPU path unverified from the app.

## Follow-ups
1. **Viewport-sized preview (deferred, filters.rs):** `camera_raw` is an `adapter_id`, so `full_resolution()` forces
   every preview / 1:1 detail / smart-filter validation to level 0 over the whole canvas through `Spec::run`
   (`submit_preview`, `set_nodes`, `filter_detail`). The renderer is level-aware; exclude `camera_raw` from that
   gate after B5-13 and B5-15 merge. The sheet already coalesces drags so it benefits without Swift changes.
2. **History / row label (fixed on the Swift side after A's round-2 review):** `Spec::name()` falls back to the
   raw id because `camera_raw` is not in `filters::registry`, so the engine still labels it "camera_raw".
   `81621d09` (RED test `DocumentCameraRawTests.testHistoryAndSmartFilterRowsShowTheFilterTitle`) and `d583b03f`
   map it to "Camera Raw Filter" where Swift converts engine history items (`DocumentHistoryIDMap.rows`) and smart
   filter records (`SmartFilterRow.init`), via `CameraRawFilter.displayName`. filters.rs untouched; swift-gate OK
   at `d583b03f` (720 XCTest, 3 skipped, 0 failures). Optional later cleanup: name adapter ids in filters.rs with
   follow-up 1, then drop the Swift mapping.
3. **Detail pane while re-editing:** `filter_detail` always appends (`StackEdit::Append`), so re-editing would show
   the saved smart filter twice; the sheet hides the pane then and relies on the canvas preview. An FFI
   `filter_detail` variant with `StackEdit::Replace(index)` would restore it (same bug affects the generic filter
   dialog's re-edit).
4. Detail pane colour: `filter_detail` writes document-linear samples and the Swift image is tagged sRGB (shared
   with `FilterSheetModel.image`); the pane likely renders darker than the canvas (not checked visually). Tag it with the document's linear space.
5. Local masks (brush/gradient/AI) are out of scope; AI masks are refused with a message.
