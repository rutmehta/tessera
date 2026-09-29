# B5-20 handoff — Adaptive Wide Angle as an editable smart filter (steps 460–479)

Branch `wp/B5-20`, rebased onto origin/main `8d3996f7`. Vanishing Point is out of scope. Scope as approved by Machine A:
CASE A of the fit analysis — the recipe is stored as the params of a new smart-filter id, rendered on the CPU.

## Commits
- `51cbbb10` filters: adaptive_wide_angle smart filter (engine; tests `crates/filters/tests/adaptive_smart_filter.rs`,
  RED-checked: 4/6 fail without the branch, the 2 passing are the degradation invariants)
- `2870c329` tessera-ffi: Adaptive Wide Angle workspace (`document/adaptive.rs`, tests `document_adaptive_ui.rs`)
- `52fee8d7` mac: Filter ▸ Adaptive Wide Angle workspace (Swift, bindings, ACCEPTANCE-STEPS.md)
- `0cbe79ed` mac: self-test on a synthetic fisheye grid
- `d5a19f76` mac: register the self-test with SelfTestHost (landed on main during the package)
- this HANDOFF

## Gates (after the rebase)
- `cargo test --no-fail-fast -p tessera-ffi -p filters -p transform`: 701 passed, 31 ignored, 1 failed —
  `document_liquify_ui::brush_latency_on_a_20_megapixel_layer` (p95 600 ms under full-suite load; passes alone).
  Also seen under parallel load only: `tessera-ffi --lib smart_preview_thumbnail::tests::hdr_saved_offline_recipe…`
  ("close the active Smart Preview editor", a shared admission gate between parallel tests) fails ~1 in 2 full
  `--lib` runs, passes alone and with `--test-threads=1` (197/197). Neither touches B5-20 code.
  B5-20 suites re-run after the rebase: filters 6/6, document_adaptive_ui 8/8 (+1 ignored timing), lib 4/4.
- `cargo clippy -p tessera-ffi -p filters -p transform --all-targets -- -D warnings`: clean. `cargo fmt --check`: clean.
- `apps/mac/build-ffi.sh`: OK (bindings regenerated and committed).
- `tools/orchestrate/swift-gate.sh`: **SWIFT GATE OK** — 846 XCTest, 3 skipped, 0 failures (+5 swift-testing);
  `DocumentAdaptiveWideAngleTests` 9/9.
- Background app self-test (`run-background-selftest.sh`, needs `make-app.sh` and a runner case for the new flag)
  was not run.

## Engine-crate changes (for A's review)
- `crates/filters/Cargo.toml`: `transform = { path = "../transform" }` (internal workspace crate, already in the
  graph through compositor; no cycle, no crates.io/C dependency). **Cargo.lock changes by exactly one line**
  (`"transform"` added to the `filters` package's dependency list).
- `crates/filters/src/compositor_adapter.rs` (+79, inside `// B5-20 begin/end` plus a 3-line dispatch before the
  `camera_raw` branch): `adaptive_wide_angle` stage =
  1. `serde_json` → `transform::adaptive::Adaptive`; strict at every depth: the params' keys must be a subset of the
     typed re-serialization (`unknown_key`, 12 lines — the transform crate has no `deny_unknown_fields` and was not
     touched);
  2. lattice cap checked first with a user-facing message ("supports layers up to 4095 × 4095 pixels (16,777,216
     mesh vertices); this layer is W × H") — zero engine work;
  3. recipe source and output must equal the stage input extent (smart-filter stages keep their extent);
  4. `solve()` → `TransformOp { version 1, Displacement, Kernel::Automatic }.apply(level 0)` on premultiplied planes,
     un-premultiplied back into an F32 raster — the same conversion as compositor's private `evaluate_transform`.
  No cancellation (the evaluator trait has none; same as `liquify`). `resident_supports` is unchanged: the id falls
  through `parse_filter` → `Unsupported` → `false`, so the resident renderer uses the CPU fallback.
- No change to compositor, transform, document/transform.rs, the .tessera-doc format (FORMAT_VERSION stays 1), PSD
  code, or board.json.

## FFI (Machine B owned)
- `crates/tessera-ffi/src/document/adaptive.rs` (new): `begin_adaptive_wide_angle(layer, stage_index)`,
  `preview_adaptive_wide_angle(token, recipe_json?)` (proxy ≤ 768 px long side, uniformly scaled recipe incl.
  line tolerance ÷ factor), `commit_adaptive_wide_angle(token, recipe_json)` (one node: pixel layer destructive via
  `blended_tiles` + `PaintTiles` + `edit_layer_checked` [revision + cancel checked under the lock], smart object via
  `set_adapter_smart_filter` [append or replace in place, validation render before the write]),
  `cancel_adaptive_wide_angle`, and the free function `adaptive_wide_angle_curve(recipe, from, to)` (source-pixel
  image of the straight edge between two clicks under the camera model, by damped-Newton inversion of
  `Adaptive::project`; used by the sheet to trace constraint lines like Photoshop's Constraint tool).
  EXIF: `FocalLengthIn35mmFilm` from the catalog for documents opened from the library (`source_image_id`), one
  SQL read; otherwise `None` and the default is 24 mm (35 mm eq.). Manual camera only (Rectilinear / Equidistant);
  no lens profiles.
- `document.rs`: module registration + re-exports (5 lines in `B5-20 begin/end`). `document/filters.rs`: one line,
  `"adaptive_wide_angle"` in `adapter_id()`.
- Timing (release, 4000 × 3000, fisheye + one traced vertical): proxy preview 667 × 500 ≈ 45–60 ms; full apply
  ≈ 1.1 s (`document_adaptive_ui::timing_on_a_12_megapixel_layer`, ignored by default).

## Swift
- `apps/mac/Sources/TesseraCore/Document/AdaptiveWideAngle/` — `AdaptiveWideAngleDraft` (recipe parse/encode that
  keeps the engine fields the sheet does not edit; Perspective/Fisheye; focal mm ↔ px; scale; lines with
  orientation, hit testing; profile recipes refused), the backend protocol, engine + stub conformances.
- `apps/mac/Sources/Tessera/Document/AdaptiveWideAngle/` — workspace model (latest-wins previews, traced lines,
  re-trace on camera change, OK through `RetouchJobs`, cancel), sheet + canvas (drag draws, ⇧ = H/V, click selects,
  Delete removes, P toggles Preview, pan/zoom), `AdaptiveWideAngleMenuItem` (⌥⇧⌘A — free in document mode),
  `AdaptiveWideAngleSheets`, `AdaptiveWideAngleSelfTest` (`--adaptive-wide-angle-selftest <dir>`; started from the
  sheets modifier as Camera Raw's; there is no `SelfTestHost` on main).
- Hooks (one line each, `B5-20` comments): FilterMenus.swift (after Camera Raw Filter…), DocumentView.swift (sheet
  modifier), SmartFilterRows.swift (double-click / Edit Smart Filter… re-opens the workspace),
  EngineDocumentBackend+Filters.swift (row name "Adaptive Wide Angle" instead of the id). Regenerated bindings.

## Remaining checks (on-screen, not done here)
- Background self-test: `open -g -n --stderr <log> Tessera.app --args --nonactivating --app-dir <d> --folder
  <folder with sample.dng> --adaptive-wide-angle-selftest <dir>` (run-background-selftest.sh has no `awa` case yet;
  add `adaptive-wide-angle` to its `camera-raw|…` line). Expect `done, 0 failure(s)`.
- Visual pass of the sheet at 1440 × 900 in both appearances; the constraint overlay colours; VoiceOver labels.
- Drawing constraints with the real mouse (the self-test drives the model's pointer calls, not NSEvents).
- ⌥⇧⌘A from the menu bar in document mode; library mode unaffected.
- Real fisheye photo end to end: every RAW fixture is over the 4095 × 4095 cap, so the positive path was exercised
  on synthetic images (Rust tests, Swift tests, the self-test's 1200 × 900 fisheye grid).

## Known limits / follow-ups
1. **Lattice cap**: layers over 4095 × 4095 (most camera originals: sample.dng is 5212 × 3468) are refused with a
   clear message. The coarse-lattice render (fit doc §4) is the follow-up that lifts it without format change.
2. Constraint curves follow the camera model only; there is no mid-point drag to bend a line to an off-model edge
   (Photoshop allows it). Users match curves to edges with Focal Length.
3. Preview and apply solve serially on one core (transform::adaptive); no cancellation inside a solve.
4. Constraints are drawn on the original (Preview off); the corrected preview has no overlay (the field is inverse
   only).
