# LR-11 — per-mask local adjustments

Branch `wp/LR-11-local-adjustments`, based on `46b1bf540dd1061438725d3cbdad652ee79f2938`
(the LR-9/9b predecessor). Local only. No dependency, lockfile or board changes.
All fixtures are invented Lua/XMP and generated catalogs; no real catalog rows,
identifiers, paths or value strings are committed.

## Implementation

- Local point curves: optional `params.curves` and `params.curves_extended`, both
  use global `ToneCurves`. Decode Main/Red/Green/Blue and their Extended spellings
  with the global normalization by 255. Extended channels inherit ordinary
  channels when not supplied. The CPU global spline is reused inside the mask.
  Curve amount interpolates/extrapolates the scene-linear curve delta.
- Local Point Color: optional `params.point_colors`, sharing LR-1's 19-number and
  SDK-resource decoder (including indexed Lua arrays) and HSL operator. With B&W,
  only local Point Color moves before monochrome; later local processing removes
  it to avoid double application. Tone cache keys include those points and mask
  geometry. Other local operators retain their post-global position, so overlay
  color survives B&W. Absent local Point Color adds no image-copy pass.
- Local Color tint: existing `params.color_overlay` now renders. A unit-value hue
  is scaled to the original Rec.2020 luminance, then mixed by saturation and
  group amount. Mask alpha limits the resulting delta. Hue 0..360 and saturation
  0..100 are accepted; zero foreign saturation stays absent.
- Local defringe: existing `params.defringe` now reuses the global edge-selective
  purple/green operator. Local 0..100 maps to global 0..20; negative or oversized
  values are retained with a named warning. Existing global hue bands and edge
  threshold remain unchanged. Only the masked delta is applied.
- Individual object instances: optional `adobe_ai.instance_hint` retains typed
  numeric IDs/bounds as informational JSON. The existing object/subject mask seam
  regenerates the selection. The info diagnostic explicitly says **per-instance
  segmentation is unavailable**; no model interface was added. Non-object,
  malformed and unknown hint structures still fail closed; nested range masks
  preserve the object seed and its hint.
- Radial conflicts: retain the entire unsupported parent and emit
  `radial mask inversion flags conflict`. No precedence was found in the reviewed
  [Adobe masking documentation](https://helpx.adobe.com/lightroom-classic/desktop/process-and-develop-photos/masking.html)
  or [Radial Filter documentation](https://helpx.adobe.com/lightroom-classic/desktop/process-and-develop-photos/lightroom-radial-filter.html).
  They describe UI inversion, not precedence between serialized `Flipped` and
  `MaskInverted`. No heuristic precedence was introduced.

These are Tessera rendering approximations, not Adobe pixel-parity claims.
All newly populated fields keep exact source and use
`import_lrcat::diagnostics::push_approximate`, with matrix-matching recipe paths.
The shared finish still owns the single Import history entry. No history writer
was added.

The three new local optional fields and the instance hint omit absent values.
Each has a schema-4 predicate and bumped-only-when-present test. Newly renderable
nonzero defringe and present overlay also require schema 4. Schema predicates
inspect history base and disabled groups as well as live settings. The existing
recipe schema constant remains 3. No old fixture golden was re-pinned.

GPU admission rejects CPU-only local fields before resident dispatch. The
compositor regression requires exact equality (absolute tolerance 0) with CPU,
and has a no-feature GPU-admission control. The raw/RGB engine paths explicitly
select CPU, and RGB memo keys isolate CPU arithmetic from earlier GPU frames.
FFI preview no longer clears overlay/defringe. MCP schema mirrors include the
new curve/Point Color types. No exported UniFFI signature or Swift UI changed,
so binding/UI and Swift build gates are not required for this lane.

## Synthetic evidence

- `crates/import-lrcat/tests/data/lr11/`: six matched Lua/XMP fixtures, including
  the retained radial conflict. Both codecs map to identical local settings;
  exact source, info diagnostics, recipe round-trip and one Import entry checked.
- `crates/tessera-ffi/tests/lr11_local.rs`: generated catalog and XMP imports to
  analytic masked reference pixels, absolute scene-linear tolerance **2e-6**.
  Includes disabled/zero-amount/outside-mask identity, Point Color before B&W,
  tone-cache invalidation, and deterministic existing host-mask seam for instances.
- `crates/pipeline-cpu/tests/lr11_local.rs`: default-byte omission and curve amount
  scaling, including 200% without producing invalid scaled control knots.
- `crates/sidecar/tests/lr11_local.rs`: native XMP round-trip of all new fields.
- Schema predicates and matrix guard cover every new field/spelling.
- Malformed curves/Point Color/overlay/defringe/hints retain the parent atomically.
  Empty controls produce no LR-11 diagnostic. Native and prior absent-field bytes
  stay unchanged. Four LR-9b tests that expected these features to be unsupported
  now assert populated fields and explicit approximation; unrelated goldens stay.

Test-first commits: `484ea506` (RED: two failures, radial named-note control passed)
and `2617ee61` (expanded fixtures/pixels/fallback coverage). Further test corrections
are included in implementation commit `4d9a8cf2`.
Initial test setup corrections: a nonexistent schema helper method was changed
into the existing free function before recording RED; global sharpening/NR were
explicitly neutralized in the pixel fixtures; one Lua literal had an extra brace;
XMP round-trip setup now uses Recipe::edit so its history matches its settings.
The analytic references and tolerances were not weakened. Preflight Clippy found
small syntax/style issues that were corrected before the clean gate.

## Aggregate measurements

The final read-only aggregate audit confirms Develop warnings **715 → 645** and
mask-warning images **71 → 1**: **70 of 71** newly translate with zero mask warnings.
Per-item counts overlap where an image has multiple local controls:

| Item | Before | After |
|---|---:|---:|
| Local tone curve | 25 | 0 |
| Local Point Color | 39 | 0 |
| Local color overlay | 11 | 0 |
| Local defringe | 2 | 0 |
| Individual AI object instance | 17 | 0 |
| Conflicting radial inversion flags | 1 | 1 |

The exclusive baseline partition is Point Color only 17; curve+Point Color 22;
curve only 2; curve+overlay 1; overlay only 9; overlay+defringe 1; defringe only 1;
AI instance 17; radial conflict 1. All classes except the last are now warning-free.
These are translation dispositions, including explicitly diagnosed approximations.
The AI result does not imply per-instance isolation.

The final isolated FFI profile passed: 21,656 images, 21,615 edited, all 21,656
originals intentionally resolved as missing, 0 imported, 21,656 skipped, and
0 fidelity samples. It confirms 645 Develop-settings warnings. The fresh
`lr11-appdir` was removed in a finally block and verified absent. No real-image
fidelity claim is made. Only curated aggregate counts are committed; raw profile
output remains outside the repository.

## Final gates — passed

Environment: PATH includes `$HOME/.cargo/bin`,
`CARGO_TARGET_DIR=$HOME/.cache/tessera-target/LR-4-parametric-masks`,
`CARGO_BUILD_JOBS=4`, `RAYON_NUM_THREADS=4`.
The explicit touched-crate release clean removed **4,781 files / 7.1 GiB**.

All eleven user-requested packages passed: **2,241 passed, 0 failed, 71 ignored**
across 357 test binaries/doc-test suites. There were no command-level exclusions;
ignored tests retain their declared status. Both real-catalog opt-in tests were
run separately and passed. Matrix guard, schema predicates, masked reference
pixels (2e-6), and GPU-selected fallback equality (0) passed in this clean gate.

```sh
cargo test --release --locked --no-fail-fast -p import-lrcat -p engine-api -p pipeline-cpu -p pipeline-gpu -p compositor -p filters -p sidecar -p image-core -p merge -p tessera-ffi -p tessera-mcp
cargo clippy --release --locked --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

All three exit codes are 0. See `gate-tests.log`, `gate-test-counts.json`,
`gate-clippy.log`, `gate-fmt.log` and their exit files. Vendor LibRaw C++ build
warnings are present; Rust workspace Clippy with warnings denied passed.
No bindings/UI changed, so Swift gates are not applicable. No GUI was opened;
no writes were made under Pictures. Dependency manifests, Cargo.lock and board.json
are unchanged. All commits are local, with the requested co-author trailer.

## Commit sequence

- `484ea506`: initial test-first RED.
- `2617ee61`: expanded synthetic fixtures, pixels and fallback tests.
- `4d9a8cf2`: complete implementation and test setup corrections.
- The final `docs(LR-11)` commit contains this handoff, translation matrix and
  final gate/aggregate evidence; its hash is reported in the delivery message.

