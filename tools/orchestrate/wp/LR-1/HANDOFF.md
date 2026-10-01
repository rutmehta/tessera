# LR-1 Point Color — implemented with fidelity and gate limitations

## Status and commits

Implementation committed locally; all 15 lane tests, all-target clippy and fmt
pass. The broad six-crate command finished with **993 passed, 2 failed, 34
ignored** (exit 101). Both failures in existing non-LR-1 FFI tests passed serial
retries using the same binaries, without source or threshold changes. The
original broad command is not relabeled green; see details below.
**Exact Adobe pixel fidelity remains open; this is a documented CPU reference
approximation.** No push, Git mailbox, Swift gate or app launch.

| Commit | Exact hash |
| --- | --- |
| Starting worktree (brief-only child of e6c3e5da) | `87ff1ff173e4d6d2053a534b7cfd9343aef906e1` |
| RED tests | `358833394103b2cca3d810508ebad6b5303e1145` |
| Implementation | `ec40d49ca2e505fc2c40b1797e51898a6074d6ac` |

Branch: `wp/LR-1-point-color`. Every lane commit has the requested Claude Fable
5.1 co-author trailer. The final docs commit contains this handoff and logs;
its hash is reported in the agent's final response.

## What landed

- The shared sidecar codec decodes complete SDK-shaped resources and the
  19-number XMP swatch sequence. Lua positional and contiguous explicitly
  indexed arrays flow through that same codec. Unknown fields, range/list
  extensions, malformed numbers, partial samples/ranges and invalid entries
  reject the entire point list and retain the exact source.
- Existing `/settings/color/point_colors` owns the shifts and range. One
  optional `selection` field stores source HSL plus the twelve independent
  feather boundaries. `source_lch` cannot represent an HSL source, and the
  existing scalar width cannot represent asymmetric H/S/L feather ranges.
  Absent `selection` is omitted from JSON, preserving old native point bytes.
  Recipe/native version contracts are unchanged.
- The CPU color operator renders imported points before mixer/grading; the
  existing `pipeline-adobe` final-stage delegation uses the same path. Native
  OkLCh points also render. Invalid controls reject before mutation; excluded
  and zero-shift points return the original floats exactly.
- A successful, single, nonempty translated property leaves the pending
  `lrcat_develop_source` map. Duplicate properties conservatively keep source.
  Existing empty/nil/legacy native-extension retention behavior is preserved.
- Synthetic end-to-end test: create the repository's synthetic SQLite catalog,
  replace a develop row with the synthetic Point Color Lua table, import the
  catalog, validate the populated recipe, assert no pending-source entry,
  and verify both the CPU color stage and full CPU renderer against expected
  pixels. Existing tessera-ffi dependencies were reused; no dependencies added.

Implementation references:
- `crates/sidecar/src/point_colors.rs`
- `crates/import-lrcat/src/lua_point_colors.rs`
- `crates/engine-api/src/recipe/settings.rs` (`PointColorSelection`)
- `crates/pipeline-cpu/src/point_color.rs`
- `crates/tessera-ffi/tests/lr1_point_color.rs`
- `crates/pipeline-cpu/POINT_COLOR.md` (formulas, source links, conventions)

## RED / GREEN evidence

`red.log`, committed with the RED hash, records six expected failures:
three mapping assertions got zero points instead of one; three CPU tests
received the old “Point Color and LUT are not implemented” error. The two
preexisting-behavior assertions in that run passed. The synthetic end-to-end
test was authored in the RED commit; its execution is included in GREEN.

The implementation was additionally checked against failing explicit Lua
index/list-extension tests (`red-edge-cases.log`) before those fixes. That log
describes an intermediate working-tree state, not a standalone RED commit.
The resource-wrapper regression passes in GREEN. Its initial failure was due
to the stale baseline artifacts described below (`stale-cache-diagnostic.log`),
so that failure is explicitly not counted as valid wrapper-specific RED proof.
No RED assertion was weakened; later changes to initial tests were equivalent
clippy style fixes or extra assertions. The old test rejecting every native
point was updated to reject a nonfinite point, since valid points now render.

`gates-lane.log`: **15 passed, 0 failed** (7 importer, 6 CPU, 1 byte-compatibility,
1 catalog-to-full-render test). Per-channel absolute tolerance: **2e-6 linear
working RGB**. Measured max error across the three principal hand-computed
swatches: **0.0**. Additional cases cover independent saturation/luminance
shifts, half feathers, hue wrap, narrower range, signed native output, malformed
source retention, and native JSON/XMP roundtrips.

Selected lane command wall time: **35.93 s**, including incremental compilation;
the synthetic catalog/full-render test itself took **3.54 s**. These are timings
under shared-machine load, not throughput benchmarks.

## Gates

All Rust commands used:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR=$HOME/.cache/tessera-target/LR-1-point-color
export CARGO_BUILD_JOBS=3
export RAYON_NUM_THREADS=3
```

- Full test gate — **exit 101: 993 passed, 2 failed, 34 ignored; 2338.49 s wall** (`gates-test.log`):
  `cargo test --locked -p import-lrcat -p engine-api -p sidecar -p pipeline-cpu -p pipeline-adobe -p tessera-ffi --no-fail-fast -- --test-threads=3`
- Selected lane — **PASS**, exit 0 (`gates-lane.log`):
  `cargo test --locked -p import-lrcat -p pipeline-cpu -p tessera-ffi --test point_colors --test lr1_point_color --test lr1_compat -- --nocapture --test-threads=3`
- Clippy — **PASS**, exit 0, 15.29 s incremental recheck (`gates-clippy.log`):
  `cargo clippy --locked -p import-lrcat -p engine-api -p sidecar -p pipeline-cpu -p pipeline-adobe -p tessera-ffi --all-targets -- -D warnings`
- Formatting — **PASS**, exit 0: `cargo fmt --all -- --check`.
- Whitespace — **PASS**: `git diff --check`.

The broad-run failures and serial retries were:

| Test | Broad run | Serial retry of the same binary |
| --- | --- | --- |
| `document_liquify_ui::brush_latency_on_a_20_megapixel_layer` | p95 **268.2 ms**, required <250 ms | **PASS**, p95 **231.0 ms**, median 170.4 ms; 35.58 s wall (`retry-liquify.log`) |
| `document_viewport::viewport_frames_composite_only_the_viewport_plus_halo` | empty dispatched rectangle, expected aligned region `[672,480,4560,2688]` | **PASS**, 6.70 s wall (`retry-viewport.log`) |

Only tessera-ffi reported failed tests; all other selected crate suites passed.
The count above uses each top-level harness's final summary, excluding nested
child-harness summaries. The serial retries isolate these tests within this
lane, not the other workers sharing the Mac. They show the failures did not
reproduce in that run; they do not prove a root cause or fix a production bug.
No unrelated renderer code or test thresholds were changed. For strict
single-command-green acceptance, the coordinator should rerun the broad gate
in a controlled workload window.

Retry commands, from `crates/tessera-ffi`, with the same environment above:

```sh
"$CARGO_TARGET_DIR/debug/deps/document_liquify_ui-65e85a775963951e" brush_latency_on_a_20_megapixel_layer --exact --nocapture --test-threads=1
"$CARGO_TARGET_DIR/debug/deps/document_viewport-c64dd3d8c4eb136d" viewport_frames_composite_only_the_viewport_plus_halo --exact --nocapture --test-threads=1
```

Existing LibRaw C++ build-script warnings remain in logs; Rust clippy reports
no warnings under `-D warnings`. Existing CPU RAW fixture goldens passed in
**276.47 s**. Ignored benchmark tests are not exercised by the normal gate.
No Swift or GUI acceptance is claimed; the coordinator owns those gates.

## 29c compatibility

Ten synthetic inputs were executed against the exact starting commit in a
temporary detached worktree, then against the implementation after a clean
rebuild: **123,044 serialized recipe bytes**, with identical BLAKE3 digests
and byte lengths in all ten cases. Coverage includes ordinary/global settings,
the existing incomplete structures fixture, legacy settings, future unknown
keys, nil/empty/opaque PointColors, other pending structures and opaque XMP.
The permanent regression is `tests/lr1_compat.rs`; baseline values are in
`tests/data/point-color-compat.txt`. `compat-base.log` records baseline execution.

The temporary worktree was removed. Its use of the mandated shared target
exposed stale cross-worktree build artifacts; engine-api, sidecar and import-lrcat
artifacts were cleaned before the final current-tree comparison and gates.
No baseline artifacts are used as proof of current GREEN. After compilation
and clippy completed, this lane's disposable incremental compiler cache
(7.4 GiB) was removed to relieve shared-disk pressure; test binaries and logs
were preserved.

Shared-file edits are additive: `lua_develop.rs` has a small guarded PointColors
array-normalization arm and retention exception; `xmp.rs` has a success check,
retention exception and explicit approximation warning. No unrelated mapping
or parser rewrite. Normal positional arrays are borrowed, not cloned; explicit
indices allocate only for the newly supported shape. Other source keys are
retained as before. Diagnostic wording for rejected PointColors may differ;
serialized recipe bytes for the ten untranslated cases do not.

No Cargo manifest, Cargo.lock or board.json changes. The coordinator-supplied
`LR-RULINGS-FROM-A.md` remains untracked and untouched by this lane.

## Limitations / unrepresentable cases

1. Adobe's working color space, hue/scale curves, range-amount curve and overlap
   behavior are proprietary and no Adobe render oracle was available. SDK
   source bounds and a public converter establish the data layout, not the
   pixel algorithm. The HSL transform, 60-degree shift convention and range
   width rule are explicitly documented reference conventions. Do not mark
   exact Adobe Point Color fidelity complete based on the synthetic tests.
2. Imported points currently operate on SDR linear-working RGB HSL. Signed/HDR
   and achromatic pixels are left unchanged. The import warning explicitly
   identifies approximation and unchanged signed/HDR pixels.
3. Missing feather tables, unknown fields (including Variance), all--1
   placeholders, noncontiguous/mixed Lua arrays, future XMP layouts and malformed
   lists remain unsupported and retain source. Unknown defaults are not guessed.
4. GPU Point Color and mask-local Point Color are outside this implementation.
   Native export uses Tessera RDF extensions, not newly generated Adobe strings;
   unchanged original XMP remains preserved by the existing exporter.
5. Swift/UI behavior and Adobe-side visual parity remain unverified by design
   under the Machine B lane overrides.
