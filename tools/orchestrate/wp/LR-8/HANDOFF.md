# LR-8: editable Lightroom Smart Preview sources

Branch: `wp/LR-8-smart-preview-proxies`, local only. Continuation above `33093751`.
Implementation commit: `03ea43ee`; preceded by five RED regression commits.

## Outcome

Implemented JPEG XL and classic JPEG LinearRaw DNG admission beside the existing
CFA decoder. Lightroom offline proxy imports keep the recipe and image identity,
use the DNG as the Develop/export source while the original is missing, and select
the recorded relocated original on the next source open when it exists.

Private opt-in prerequisite:

**decodes: yes, 2560 × 1707**

The CPU scratch render is outside the repository at
`/private/tmp/lr8-smart-preview-render.png` (direct pipeline RGB, before
presentation orientation). No private pixels, sample names,
content hashes, EXIF, or catalog-derived strings are in committed fixtures.
The private render is for coordinator inspection; this is not a claim of measured
pixel parity with Lightroom. No foreground GUI was launched. The full private
catalog has not been imported by this lane; import/copy/relink and UI integration
are verified by synthetic engine tests and Swift build/test gates, not live GUI
interaction.

## Import, storage, and relink

- `SmartPreviewIndex` derives validated UUID paths without enumeration. Import
  retains **AgLibraryFile.id_global**, not Adobe_images.id_global.
- Plan exposes online originals / offline with Smart Preview / offline without
  Smart Preview. `OfflineProxy` is a distinct internal per-image outcome.
  `import_smart_previews` defaults on; `copy_proxies` defaults off.
- Apply references the preview in place. Existing B5-38 protected paths route
  recipes/XMP to app support by content hash plus path alias. Copy mode puts DNG
  bytes into app support, with a bounded input and atomic publication.
- `recipe.unknown.lightroom_smart_preview` records origin, proxy path, and the
  original path after relocation. `library.json` unknown metadata records proxy
  membership so a normal editable folder queue includes externally stored proxies.
- The index remains v9. No migration or new table is introduced. Stable indexed
  proxy identity owns edits; fresh source resolution selects the original without
  moving or rewriting the recipe. Relinked pixels have a separate render identity
  to avoid mixing proxy/original tile caches. Source selection refreshes when
  opening Develop/export/thumbnail or refreshing/reopening the library.
- Originals without proxies retain existing skip/report behavior. Standard
  `.lrprev` JPEG previews remain reference-only and are never editing sources.
- Swift grid/loupe show Smart Preview with accessibility labels/identifiers;
  import has counts plus import/copy toggles; export warns about proxy resolution.

## Decoder semantics and limits

`jxl-oxide 0.12.6` returns f32 samples. Integer samples are scaled by
`1 / (2^bits_per_sample - 1)`; floating samples are floats. The decoder checks
codestream bit depth and float/integer type against TIFF BitsPerSample and
SampleFormat, and multiplies integer output back into DNG code units before
LinearizationTable, BlackLevel and WhiteLevel normalization. Floating DNG defaults
to WhiteLevel 1. The same DNG calibration and AsShotNeutral are used by the common
camera-profile/WB render stages. BaselineExposure is applied before that tail.

The requested JXL output encoding is **the codestream's own encoding** (enum or
original ICC), with `NullCms`; no display-sRGB or linear-sRGB target is requested.
This recovers source channel values, whose meaning is supplied by DNG, not by a
JXL display colour label. XYB inversion is necessary codec reconstruction for
lossy tiles; raw XYB planes are not camera RGB. We retain the same-encoding
reconstruction and do not reinterpret XYB as camera channels. The synthetic
16-bit lossless fixture deliberately has an sRGB codestream label and asserts
camera code values numerically, detecting a spurious gamma/display conversion.

TIFF bounds, IFD/tag/pixel limits, per-tile byte limits, and a JXL allocation cap
are enforced. Tiles/strips, both TIFF byte orders, scalar/per-channel black/white,
complete integer linearization tables, integral DefaultCrop/ActiveArea, and EXIF
orientation are supported. Orientation is retained at the source boundary and
applied by presentation/export exactly as for camera RAW.

Required MapPolynomial (opcode 8) lists are executed in order, before crop:
List1 operates on native sample units; Lists2/3 on normalized samples. Consumed
lists are removed to prevent replay. Area, planes, pitch, degree, coefficients,
version, flags, and framing are bounded. Mixed polynomial/other-opcode lists fail
closed rather than reorder operations. Camera-source admission also rejects any
remaining opcode list instead of silently dropping unconsumed corrections.

Remaining format limits are explicit: planar/non-three-channel tiles, differing
channel bit depths, spatial black deltas/repeats, fractional DefaultCrop, unsupported
required opcodes and mixed polynomial lists are not silently approximated.
ColorMatrix1/2 and ForwardMatrix1/2 remain retained by the decoder; the current
RawMetadata pipeline selects the D65 matrix when present, otherwise Matrix1.
This does not implement new dual-illuminant interpolation or embedded DNG profile
look tables/tone curves. Existing Adobe profile/operator approximations still
apply. Native generated Smart Preview restrictions remain unchanged.

## Develop, GPU, and export

External DNG camera samples are distinguished from Tessera-generated cached RAW
prefixes. They enter both Native and Adobe camera-linear Develop, retaining
camera WB/profile handling and normalized geometry. Native generated previews
still require Native2 and still cannot be used as full-quality exports.

External DNG sources use an **explicit CPU fallback**, including under a GPU
renderer: their resident tail plan is None. Synthetic tests exercise a GPU-backed
renderer and proxy-resolution export. Export batch memory estimates use proxy
pixel dimensions. Mosaic denoise, AI/depth masks, lens blur and retouch remain
subject to the existing camera-linear source admission limits; they are not
claimed as newly supported by LR-8.

## Exact dependency/lock delta for Machine A

`crates/raw-decode/Cargo.toml` adds `jxl-oxide.workspace = true` alongside the
previously authorized `zune-jpeg.workspace = true`.

The root workspace JXL declaration is changed to
`{ version = "0.12", default-features = false }`. This is necessary to avoid
introducing Rayon/Rayon-core edges under jxl-threadpool from the workspace's
default feature. No package addition or version change is involved.

```diff
diff --git a/Cargo.lock b/Cargo.lock
index 88e72768..c2431b3f 100644
--- a/Cargo.lock
+++ b/Cargo.lock
@@ -4106,0 +4107 @@ dependencies = [
+ "jxl-oxide",
@@ -4109,0 +4111 @@ dependencies = [
+ "zune-jpeg 0.5.15",
```

## Machine A review: changed engine files

- `crates/cull/src/lib.rs`
- `crates/export/src/batch.rs`
- `crates/export/src/lib.rs`
- `crates/export/tests/lrcat_jxl.rs`
- `crates/image-core/src/smart_preview_render.rs`
- `crates/image-core/src/source.rs`
- `crates/image-core/tests/lrcat_linear.rs`
- `crates/pipeline-adobe/src/render.rs`
- `crates/pipeline-cpu/src/render.rs`
- `crates/pipeline-cpu/src/smart_preview.rs`
- `crates/pipeline-cpu/src/smart_preview_codec.rs`
- `crates/pipeline-cpu/tests/lrcat_dng.rs`
- `crates/raw-decode/Cargo.toml`
- `crates/raw-decode/src/lib.rs`
- `crates/raw-decode/src/lossy_dng.rs`
- `crates/raw-decode/tests/fixtures/README.md`
- `crates/raw-decode/tests/fixtures/generate-gradient.rs`
- `crates/raw-decode/tests/fixtures/linear-gradient.jpg`
- `crates/raw-decode/tests/lossy_dng.rs`
- `crates/raw-decode/tests/smart_preview.rs`
- `crates/raw-decode/tests/support/mod.rs`
- `crates/raw-decode/tests/fixtures/linear-gradient.dng` (synthetic only)
- `Cargo.toml`
- `Cargo.lock`

Other changes are import-lrcat, tessera-ffi, Swift app/core and tests, generated
UniFFI bindings, and this handoff. No board.json change.

## Verification

- RED synthetic JXL regression: old decoder rejects compression 52546.
- RED camera Develop regression: old RawImage path reaches LibRaw error -2.
- RED offline import contract: missing fixture/options/outcomes and source state.
- RED polynomial regression: old samples do not apply the required polynomial.
- RED residual-opcode regression: old camera-source admission ignores the list.
- Focused synthetic JXL, classic camera Develop, normalized crop/radial/Upright,
  GPU-or-explicit-CPU-fallback and export tests: PASS.
- Opt-in private decode and CPU scratch PNG render: PASS, including after
  enforcing rejection of unconsumed correction lists.
- Final residual-opcode camera-source regression: PASS.
- Full release gate (`--locked`): PASS for import-lrcat, raw-decode, library,
  image-core, tessera-ffi, export, cull, pipeline-cpu, and pipeline-adobe, including
  the 20,000-image FFI streaming memory gate.
- Final FFI regression: PASS for both reference and copy mode, Develop recipe
  persistence, proxy PNG export, unchanged Lightroom bundle, unchanged recipe
  after automatic relink, and reopening a copied proxy after the catalog moves.
- Workspace Clippy `--locked --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- UniFFI regeneration: PASS.
- `tools/orchestrate/swift-gate.sh`: **SWIFT GATE OK**. 919 XCTest tests, three
  skipped, zero failures; five Swift Testing tests passed. Initial theme lint
  failure fixed by using existing spacing/radius tokens for the loupe badge.
- `swift build -c release --product Tessera -Xswiftc
  -strict-concurrency=complete -Xswiftc -warnings-as-errors`: PASS (142.48 s).
  The linker still warns that `blake3_neon.o` in the Rust archive targets macOS
  26.2 while the app links for 15.0. This lane has not verified execution on macOS
  15; the Swift compiler gate passes. An earlier concurrent build correctly
  aborted when the theme fix changed its input, and the final stable-source
  rerun above completed successfully.
- Final diff whitespace check: PASS. Generated UniFFI changes were regenerated;
  only generator trailing whitespace on changed lines was normalized.

All Rust checks use the requested external LR-8 target, four build jobs, and four
Rayon threads. Committed test inputs are synthetic or existing public fixtures;
new tests do not access ~/Pictures.
