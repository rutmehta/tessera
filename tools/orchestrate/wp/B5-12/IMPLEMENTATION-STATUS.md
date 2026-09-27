# B5-12 implementation status: Warp, Perspective Warp, Puppet Warp, Content-Aware Scale

Branch `wp/B5-12` (base `wp/B5-10b`). Apple M4 Max, Xcode 26.3 / Swift 6.2.4, release Rust.

## FFI (crates/tessera-ffi/src/document/transform.rs, module `advanced_transform`)

Free functions: `warp_preset_names()`, `warp_preset(width, height, preset, bend) -> WarpMesh JSON`,
`warp_split(mesh_json, x, y, split_u, split_v)` (exact de Casteljau through a child point), `warp_subdivide(mesh_json,
columns, rows)`.

`DocumentSession`:

| Call | Contract |
| --- | --- |
| `transform_stages(layer)`, `transform_stage(layer, index)` | Committed `transform` stages: kind, `TransformOp` JSON (protect mask stripped, `has_protection`), enabled, blend, opacity |
| `begin_advanced_transform(layer, index?, kind) -> AdvancedTransformInfo` | Token session; locks and kinds checked; child size, `child_to_document` (row-major `[a,b,c,d,e,f]`), content bounds, `needs_conversion`, draft level, other stages, limitations. Ends an older session (its preview dropped), commits other pending drags |
| `preview_advanced_transform(token, transform_json, draft) -> {update, deformed_json?}` | Strict serde `TransformOp`, validated (+ puppet ARAP solve, deformed vertices returned); scratch only, no history; invalid geometry fails with the previous preview intact |
| `content_aware_scale_from_channel(token, w, h, amount, channel?, draft)` | CAS preview; protection = a saved **alpha** channel sampled Rust-side onto the evaluation grid (proxy grid for drafts, child grid otherwise), through the placement; no skin detector |
| `commit_advanced_transform(token, convert_to_smart_object)` | ONE node labelled Warp / Perspective Warp / Puppet Warp / Content-Aware Scale: `AddTransform`, `SetTransform` (re-edit keeps enabled/blend/index), or `Batch[RemoveLayer, AddLayer(wrapper), AddTransform]` for non-smart layers, only with consent (otherwise error, session stays). Nothing previewed: ends with no node |
| `cancel_advanced_transform(token)` | No history change; never creates a smart object |
| `puppet_mesh_from_layer(layer, density, expansion)` | Pin-less `PuppetWarp` from the source alpha; honest errors (density name, expansion > 64); coarsened level + note to stay ≤ 16,384 vertices |
| `save_psd_rasterizing_transforms(path)` | Explicit PSD/PSB copy with smart objects that have enabled stages rasterized; the session is unchanged |

Stale tokens (replaced/ended session, another document) and layers changed since `begin` (undo, other edits: layer
revision / kind) are rejected with no change. Previews live only in the scratch document (never in `pending`), so an
unrelated edit, save or undo drops the preview instead of flushing it into history (the session continues from the
new base; tested). Large children (> 1 MP) get a draft proxy: the child rendered at level L into a proxy smart object
(one cache key per session) with the geometry scaled by 2^-L; the editor sends drafts while a proxy exists and the
exact stage is rendered once on Apply.

## How reserved transform stages survive filter edits (document/filters.rs, `// B5-12` blocks)

* `Node::of` recognises `name == "transform"` and keeps the stage verbatim (`raw: Arc<Value>`): never fed to the
  Spec parser / baker; `store()` writes the original params back with the node's enabled / blend / opacity, so
  appending, editing, disabling or deleting another filter rewrites the stack with the transform untouched
  (tested: `smart_filter` edits leave the `SmartFilter` byte-identical).
* `set_smart_filter(Params)` on a transform row is refused (edit it with Edit ▸ Transform); enable / blending edits
  work. Whole-stack edits (`set_nodes_checked`) reject any change to transform stages (index, enabled, blend,
  geometry) under a position / all lock (engine rule); colour filters keep their existing lock semantics.
* Records name stages by operation (Warp, …) with the protect mask stripped; bake keys use a digest (stripped JSON
  + protect length + strided samples) so large masks are not serialized per frame.
* Routing: a smart object whose enabled stages are all geometric (Free/Warp/Perspective/Puppet/Displacement) is not
  baked: the session renderer evaluates it (resident Metal route of M5-23 on this Mac, CPU compositor otherwise).
  Stacks with content-aware scale or mixed with menu filters bake on the filter worker (off the session lock)
  through the compositor (`native_stack`, menu filters bridged by `NativeFilterEvaluator`), at the view level.
* `smart_wrapper` is the one wrapper for Convert for Smart Filters and B5-12 conversions: id, name, props, styles,
  raster AND vector mask exactly once on the wrapper (previously the vector mask stayed inside the child).

## Measured latency (20 MP)

* Rust `bench_warp_drag_20mp` (ignored; 5472 × 3648 smart object, Arc, bend 0.30…0.335 in 8 steps, preview call →
  presented level read back through the resident Metal renderer): `begin` 17 ms (draft level 3); draft previews
  median 104.6 ms (range 93.9–135.6) read at level 2, 100.9 ms (77.8–116.5) at level 3; exact full-resolution
  previews median 4615 ms (4426–5106) — CPU inverse-map preparation (NEEDS 2); commit + level-2 read 21.5 ms
  (geometry already prepared by the last exact preview).
* App self-test (`transform-latency` log, preview call → frame presented in the viewport, 3 drags of 30 steps on the
  20 MP smart object at Fit): n 27, median 218.4 ms, p95 343.7 ms, max 435.2 ms; begin 301 ms; Apply → exact frame
  1457 ms. On the 1600 × 1000 card (no proxy, full-resolution previews): median 300 ms, p95 408 ms.
* No number above is a cached GPU-only timing.

## Tests

* Rust `crates/tessera-ffi/tests/document_transform_ui.rs`: 17 cases + 1 ignored bench (consent/cancel, wrapper
  keeps source/masks/styles once, presets/splits, linked quads + rejected geometry, puppet pins/rotation/limits,
  CAS amount + channel protection, placement mapping + draft protect on the proxy mip, one node + undo/redo, stale
  tokens / changed layers / other documents, re-edit keeping neighbours/order/blend/enabled + mixed-stack bake,
  locks, live text wrapper, native reopen + PSD refusal + rasterized copy, all five kernels, real Metal vs CPU for
  warp/perspective/puppet stacks at levels 0 and 1 (≤ 1/255), unrelated edits drop previews, draft proxy vs exact).
* Swift `DocumentTransformTests`: 16 cases (schema, Bézier evaluation, anchor drags, engine splits/presets,
  perspective JSON/round trip/split without jump/layout vs warp/convexity, puppet pins/rebase, CAS box, kernels,
  child mapping, real engine session through `EngineDocumentBackend`, puppet mesh + protection channels).
* App self-test `--transform-selftest=<dir>`: 44 checks, 0 failures (`evidence/transform-selftest.log`).

Gate (`CARGO_TARGET_DIR=$HOME/.cache/tessera-target/B5-12`, `CARGO_BUILD_JOBS=2`):

* `cargo test --locked --release -p transform -p compositor -p psd -p tessera-ffi`: exit 0, 587 passed, 0 failed,
  21 ignored.
* `cargo clippy --locked --release -p tessera-ffi --all-targets -- -D warnings`: clean. `cargo fmt --all -- --check`:
  clean. `git diff --check` (excluding generated bindings): clean.
* `./build-ffi.sh && swift build --jobs 2 && swift test --jobs 2`: Executed 311 tests, with 0 failures.
* `xcodebuild -scheme Tessera -configuration Debug … build`: `** BUILD SUCCEEDED **`; `Support/make-app.sh debug`: built.

## Evidence (`evidence/`, `screencapture -x -o -l <window>` of this PID's window, app launched `open -g -n`)

381 warp preview on a pixel layer and the conversion alert; 382 anchor/handle drag; 383 split; 384 Flag bend; 385
linked planes; 386 rejected crossing; 387 pins + rotation ring; 388 rigid sparse; 389 CAS amount 0 / 1; 390
protection channel (rings keep their shape); 391 200 % zoom; 396 live text preview; 397 native reopen; 399 20 MP
drag + applied; 399 1440-pt panels. (The toolbar overlapping the options bar in window captures of a background
window is pre-existing, see B5-10's evidence.)

## Needs on-screen verification

* Real mouse / trackpad feel of Bézier anchor and tangent dragging, perspective vertex dragging, pin dragging and
  ⌥-drag pin rotation (the self-test synthesizes events into the viewport, without an active window).
* The Apply conversion alert and the Save Rasterized PSD Copy panel as sheets on an active window.
* Esc / Return / ⌫ through the real key routing on a focused canvas (tested through `DocumentTools.handleKey`).
* B5-07 styles and B5-09 Remove regressions were covered by their unit tests in `swift test`, not re-run through
  their app self-tests here.

## Limitations (explicit in the UI's limitations glyph and status bar)

* Output clipped to the fixed child canvas; the warp net spans the whole child canvas (engine source domain at the
  origin); CAS output anchored top-left.
* Nonlinear stages are native-only: PSD save refuses them; the rasterized copy is explicit.
* CAS is CPU seam carving (slow for large changes on large images); protection needs a saved alpha channel, capped at
  4 MP children at full resolution (inline mask storage); no automatic skin detection.
* Puppet mesh from the child source (not the output of stages below it); coarsened for large opaque layers.
* Perspective editing covers lattice-shaped plane sets (what this editor creates); other quad sets are listed but
  not re-editable here. Free / Displacement stages are listed, not edited.
* Layer and composite thumbnails show the source without stages (render.rs, NEEDS 9).
* The first exact render after Apply on large images blocks the session while it prepares geometry (NEEDS 2); the
  editor reloads rows only after that frame so the main thread does not wait on it.

## Deviations

* `commit_advanced_transform(token, convert_to_smart_object)` takes the consent flag; `preview_advanced_transform`
  and `content_aware_scale_from_channel` take `draft`; `content_aware_scale_from_channel` is session-scoped (token,
  not layer) so the protect mask never crosses the bridge; previews return `{update, deformed_json}`.
* Extra calls: `transform_stages`, `warp_preset_names`, `warp_subdivide`, `save_psd_rasterizing_transforms`.
* Previews are scratch-only instead of pending ops, so unrelated edits drop (not flush) a transform preview.
* `native_stack` warms the stack from the calling thread before the parallel tile render to avoid the compositor
  lock deadlock (NEEDS 1).
* The self-test starts from AppCommands (`--transform-selftest=<dir>` must run before any document exists) and
  exits the process after its log if a save prompt holds termination.
* Swift adoption lives in `TesseraCore/Document/Transforms/DocumentTransformsBackend.swift` (as B5-10 did);
  StubDocumentBackend does not adopt it (the editor reports that transforms need the engine backend).
