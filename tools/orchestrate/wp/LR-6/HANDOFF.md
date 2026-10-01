# LR-6 — blocked at RED, not merge-ready

Branch: `wp/LR-6-lens-blur`, Machine B, local only.

Base: `e0f9141df07b87849d3fb2a00894de06c2a332f6`.
RED: `a258521f4fea60240f7fa22af447d9660cf09b63`
(`test(LR-6): expose missing Adobe lens blur and depth bookkeeping`).
GREEN/feature hash: none. No production implementation or recipe fields added.
The docs commit containing this file is the handoff commit; obtain its exact hash
with `git log -1 --format=%H -- tools/orchestrate/wp/LR-6/HANDOFF.md`.

## Observed RED

`cargo test -p import-lrcat --test lr6_lens_blur`: exit 101, 0 passed,
3 failed, 0 ignored. Evidence: `evidence/red.log`.

- `lr6_lua_lens_blur_translates_without_opaque_source`: active structured Adobe
  LensBlur does not populate `settings.effects.lens_blur`.
- `lr6_xmp_lens_blur_translates_without_opaque_source`: same missing behavior
  through XMP resource attributes.
- `lr6_missing_adobe_depth_records_regeneration_in_recipe`: no persistent
  `regenerated depth` bookkeeping for unavailable depth.

Fixtures are invented in Rust strings. No real catalog, helper, original image,
or `~/Pictures` was accessed. Tests deliberately remain failing and enabled.
They are initial acceptance probes, not a complete fidelity suite: round-trip
assertions follow translation but cannot yet execute; there is no new field or
CPU render test. No numerical focal-range/bokeh mapping is asserted without a
verified source contract. The third probe checks a regeneration marker, not a
claim that inference has already succeeded.

## Blockers and verified architecture

The binding requirement is full fidelity. The source information located here
is insufficient to implement that honestly:

1. `sidecar/src/structures.rs` explicitly rejects Adobe FocalRange/BokehShape
   because four-value range and numeric shape semantics are not established.
   `engine-api/src/recipe/settings.rs::LensBlur` has only two focus endpoints,
   a string shape, amount, and model provenance. Adobe also has shape detail,
   aspect/rotation, highlight threshold, cat-eye scale, spherical aberration,
   and refinements that need rendering semantics, not just stored metadata.
2. `import-lrcat` only has a Previews.lrdata reader; no discovered Adobe depth
   table/resource decoder connects DepthMapInfo to an imported raster. A table
   identifier must not be invented as a filesystem path, or a synthetic PNG
   helper format presented as Adobe's format. The helper layout, payload
   encoding, image/resource association, depth calibration/direction and
   refinement semantics need an authoritative contract or approved synthetic
   conformance fixture.
3. `pipeline-cpu/LENS_BLUR_M3.md` documents the existing operator as a reference
   approximation: depth bins, mean radii, fixed shape profiles, and boost above
   linear luminance 1. These controls alone do not establish Adobe equivalence.

Public primary material checked:

- [ExifTool XMP source](https://raw.githubusercontent.com/exiftool/exiftool/master/lib/Image/ExifTool/XMP.pm)
  and [tag table](https://www.exiftool.org/TagNames/XMP.html): field names/types,
  but no sufficient numeric-bokeh/rendering or helper-payload contract.
- [Adobe Lens Blur documentation](https://helpx.adobe.com/lightroom-classic/desktop/process-and-develop-photos/lens-blur.html):
  public feature description, not a binary depth-table specification.

This is **not a missing ml-depth dependency blocker**. `image-core/Cargo.toml`
already depends on ml-depth; `image-core/src/depth.rs::DepthProvider` offers
`from_support` (cached model inference) and `from_map` (synthetic/camera-depth
injection). `Renderer::apply_depth_effects` reaches the CPU lens-blur operator.
`export/src/depth.rs` already uses that provider through image-core. The importer
and sidecar lack ml-depth, but can potentially emit a deferred regeneration
marker while inference remains in image-core. No dependency change is proposed.

The user was asked for an approved source contract or synthetic helper fixture;
none had arrived when writing this handoff. Next work is to establish the
contract, extend the recipe additively with absent fields having no effect, and
add per-field round-trip/render coverage plus imported-depth and deferred
regeneration bookkeeping tests. Do not weaken the existing RED probes to claim
the lane complete.

## Environment and gates

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/LR-6-lens-blur"
export CARGO_BUILD_JOBS=3
export RAYON_NUM_THREADS=3
```

Gates on the RED source commit:

| Command | Result | Evidence |
| --- | --- | --- |
| `cargo test -p import-lrcat -p engine-api --no-fail-fast` | exit 101; 161 passed, 3 intended RED failures, 1 ignored | `evidence/test.log` |
| `cargo clippy -p import-lrcat -p engine-api --all-targets -- -D warnings` | exit 0 | `evidence/clippy.log` |
| `cargo fmt --all -- --check` | exit 0 | `evidence/fmt.log` (empty output) |

The ignored test is `streaming_import_memory_is_flat_and_time_bounded`, whose
existing annotation restricts timing/allocation checks to release builds.
No latency failures occurred. No Swift gates or app run.
RAW-fixture rendering suites are excluded; only import-lrcat and engine-api
are selected (including synthetic pinned-RAW metadata tests, which read no RAW).

## 29c compatibility and scope

Production Rust and recipe serialization are unchanged from the base. Therefore
untranslated recipe bytes and unconditional exact-source retention are unchanged
by construction. Existing source-retention tests are included in the gate run.
No key has been removed from `lrcat_develop_source`; neither matrix row is
promoted to translated. DepthMapInfo ownership was corrected from LR-2 to LR-6
under the binding user assignment. No mailbox, board.json, Cargo.lock,
dependency, Swift, app, or remote changes.
