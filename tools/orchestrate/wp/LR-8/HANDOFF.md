# LR-8: editable Lightroom Smart Preview sources

See [the LR-8b follow-up](LR-8b-HANDOFF.md) for catalog orientation, shared
BaselineExposure, current preview-cache decoding and real-catalog validation.

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

## LR-8e hotfix (B1, B2)

Machine B; branch `wp/LR-8-smart-preview-proxies`, based on `afe97d4c`.
Binding scope: Machine A's `A-LR8-REVIEW.md`, B1/B2, M9, and the small M1
APP14 guard. No B3/B4 or other rendering/import changes are included.

- **B1:** the reader identifies full-resolution LinearRaw from its inline
  PhotometricInterpretation before interpreting other IFD fields. Errors before
  identification return `None` silently; errors after identification propagate.
  Both existing source-open call sites therefore retain their LibRaw fallback
  without call-site changes. The generated CFA regression first proves LibRaw
  can unpack an ordinary DNG containing an exotic optional TIFF field, then
  verifies that the lossy reader declines it.
- **B2:** field types/ranges and retained numeric values, layout, crop,
  calibration, opcodes, all tile ranges/counts, and decoded-byte accounting are
  checked before pixel allocation. Tile dimensions cannot exceed the image
  rounded up to a 16-pixel tile boundary; strips cannot exceed image dimensions.
  The sum of padded tile outputs (three f64 channels) is capped at 512 MiB.
  Every codec header is checked before allocating the full image buffer.
  Metadata projection still avoids compressed payload reads.
- JPEG receives zune's maximum width/height before header/decode. JXL receives a
  tile-derived allocation tracker (128 bytes per tile pixel plus 16 MiB,
  capped at 512 MiB) before initialization. Because jxl-oxide 0.12.6 has no
  dimension-limit builder option, staged initialization checks dimensions,
  bit depth, sample format, orientation, and channel count before feeding frame
  data; it does not use the eager `builder.read` path. Header consumption is
  capped at 64 KiB. Both codec paths reject oversized header-only inputs.
- **M9:** tile-count and aggregate decoded-byte multiplication use checked
  arithmetic. Huge dimensions and overflow-sized tile arrays are rejected.
- **M1:** APP14 stripping is reachable only for a selected LinearRaw IFD and
  removes only Adobe transform 0. Explicit YCbCr/YCCK markers are preserved.
  Synthetic tests pin YCbCr marker preservation and non-LinearRaw fallback.

All new inputs are built in tests (including the CFA pixels and both malformed
codec headers). No private fixtures, GUI work, dependency/lock changes, generated
bindings, or board edits. Production changes are confined to
`crates/raw-decode/src/lossy_dng.rs`; tests remain in raw-decode.

RED commit: `6417ec4f` (`test(LR-8e): expose LinearRaw admission and allocation
regressions`). Its release run recorded five expected failures and five passes.
Additional coverage includes malformed field order, crop/calibration/NaN values,
empty and short LinearizationTables, cyclic IFDs, truncated JPEG/JXL, and a small
byte-mutation/truncation corpus. The existing truncation test now distinguishes
pre-identification fallback from post-identification errors, as B1 requires.

Verification uses `$HOME/.cache/tessera-target/LR-8e`, four Cargo build jobs,
`$HOME/.cargo/bin` on PATH, and `--locked`.

Implementation commit: `3256ed56cff0ad4ba6fe8ff4b142113a5155763d`
(`fix(LR-8e): gate LinearRaw admission and bound tile decoding`). Cherry-pick
`6417ec4f` first, then this fix, then the documentation commit containing this
section. All three commits carry the requested Claude Opus 5.5 co-author trailer.

Final verification:

- Release gates **PASS** for raw-decode, image-core, pipeline-cpu, import-lrcat,
  previews, export, and tessera-ffi. Completed final-source per-crate runs total
  **1,492 passed, zero failed, 56 existing ignores**. The first combined release
  run also passed with those totals.
- **15 new safety tests pass:** 14 integration tests plus the APP14 unit test.
  After the test-only Clippy helper cleanup, `cargo test --locked --release
  -p raw-decode --test lr8e_safety` passed all 14 again.
- `cargo clippy --locked --release --workspace --all-targets -- -D warnings`:
  **PASS**.
- `cargo fmt --all -- --check`: **PASS**.
- `git diff --check`: **PASS**. Cargo.lock and board.json are unchanged.

Timing reruns are recorded explicitly: a final-source combined run hit the
existing previews 3-second timing assertion at 3.979 s (render output checks
passed). The unchanged isolated Cargo test passed at 2.252 s; the same original
executable also passed at 2.487 s with four Rayon threads. The previews and
raw-decode full release suites passed in a remaining-crates run using
`RAYON_NUM_THREADS=4` and `--test-threads=4`. That run then hit the existing FFI
export/slider p90 timing assertion at 24.4 ms against 16 ms. The complete FFI
release suite passed when rerun with `--test-threads=1` and the normal Rayon
configuration, including that latency assertion and the 20k-image streaming
memory gate. No timing thresholds, production code, or CI skip flags were
changed to obtain these rerun results.

Local evidence: `/tmp/lr8e-red.log`, `/tmp/lr8e-release.log`,
`/tmp/lr8e-release-final.log`, `/tmp/lr8e-preview-retry.log`,
`/tmp/lr8e-release-remaining.log`, `/tmp/lr8e-ffi-serial.log`,
`/tmp/lr8e-safety-final.log`, `/tmp/lr8e-clippy.log`, and `/tmp/lr8e-fmt.log`.

### LR-8f review response (on top of 85958b35)

1. **B1 fixed.** Admission now requires the selected full-resolution IFD's
   inline SHORT PhotometricInterpretation=34892, Compression=34892 or 52546,
   and SamplesPerPixel=3. Other LinearRaw layouts fall through to LibRaw,
   including compression 1/7/8 and one-channel images; tests cover both readers
   with valid and malformed unrelated fields. Pre-identification errors still
   return None. A current recursive inventory of `fixtures/raw` found one DNG:
   its full-resolution IFD has PI=32803, compression=1, channels=1 (CFA), while
   its thumbnail has PI=2. No compression-7/8 LinearRaw or ProRAW-like fixture
   exists there, so the conditional origin/main real-fixture comparison is N/A.
2. **B2 fixed.** Checked sum of compressed tile byte counts must be no larger
   than the file and no larger than 128 MiB. Tile count is capped at 65,536;
   the existing per-tile 32 MiB limit remains. Compressed tiles are retained
   during header validation and consumed from that bounded cache: each tile
   is read from the file once. Metadata projection reads no tile payloads.
   Tests reject alias amplification and excessive tiny tiles and count actual
   payload bytes read. Existing field/layout/header validation precedes the
   full pixel allocation. JPEG maximum dimensions and JXL AllocTracker remain.
3. **M1 fixed.** Only Adobe transform 0 selects raw component interleaving.
   Transform 1 and JFIF without APP14 request RGB output from zune, converting
   YCbCr. Tests check actual decoded pixels through PI=34892 for all three
   cases (first camera pixel approximately 48/69/84 versus converted 0/100/0),
   plus existing RGB component-ID coverage. The synthetic camera-channel JPEG
   and its DNG wrapper now explicitly carry Adobe transform 0; the generator
   and fixture documentation describe that correction.
4. **JXL verified.** Header input grows geometrically from 64 bytes to a 64 KiB
   ceiling, replacing byte-at-a-time reparsing. AllocTracker is installed before
   initialization; bounded initialization can buffer a frame prefix, but image
   geometry is checked before feeding remaining data or rendering pixels.
   A valid 16x16 header with zero frames is independently parsed in the test,
   then the DNG reader must return `JXL frame missing`, never call render_frame(0).
   The supplied private 16-bit lossy JXL sample was decoded and rendered at
   `afe97d4c` and at the LR-8f implementation, using identical default settings
   and scale=1. Numeric comparison only:

   | Output | Dimensions | Compared values | Max absolute difference | Bit-identical |
   | --- | --- | ---: | ---: | --- |
   | Decoded camera f32 | 2560x1707 | 13,109,760 | 0 | yes |
   | Linear render f32 | 2560x1707 | 13,109,760 | 0 | yes |
   | RGB8 render | 2560x1707 | 13,109,760 | 0 | yes |

   Both private runs passed. Source was opened read-only; temporary pixel
   outputs and the detached baseline worktree were deleted after comparison.
   No private image, metadata, identifier, or derivative is committed. No GUI
   or install was performed.
5. **Seeded mutation test added.** Deterministic xorshift seed
   `0x8f5eed1234567890`; 128 bit-flip/truncation cases per full JPEG/16-bit JXL
   seed, each passed through read and read_metadata (512 invocations). Both
   seeds decode successfully first, include calibration, crop, ActiveArea,
   all three opcode lists, and full LinearizationTables (256/65,536 entries).
   The IFD precedes payloads so truncation exercises identified-container
   validation. Bit flips sample IFDs, payload tails, and the complete container.
   Each invocation asserts no panic, cumulative allocations below 768 MiB
   (also bounding peak tracked allocation), and elapsed time below 2 seconds.
   Per-thread allocator instrumentation includes codec allocations in this
   non-Rayon decoder configuration. Final focused run: maximum 2,455,363 bytes,
   maximum 1,171 microseconds; entire seven-test binary finished in 0.04 s.

**Minors:** removed the f64 tile copy. JPEG stores u8; JXL stores f32 and promotes
one sample at a time for normalization, preserving prior f64 arithmetic and
bit-identical private output. Conservative 24-byte/padded-pixel admission remains
at 512 MiB. The cyclic-IFD regression asserts the specific cyclic/excessive-graph
error. Production changes are confined to raw-decode's lossy_dng.rs.

**Attempt ledger:**

- Synthetic JXL generator: first rustc invocation selected cached zune-core 0.4
  and failed a type/version mismatch; selecting already-locked 0.5.3 succeeded.
  No dependency or lock edits.
- `/tmp/lr8f-red.log`: five intended failures (admission, aliased reads, duplicate
  reads, tile count, JPEG pixels); header-only and mutation tests passed.
  RED commit `ec2f312a` precedes implementation.
- `/tmp/lr8f-green-1.log`: 28 focused tests passed; mutation max 2,455,331 bytes,
  710 microseconds. `/tmp/lr8f-green-2.log`: seven passed after strengthening the
  frame-less JXL assertion; max 2,455,331 bytes, 741 microseconds.
  `/tmp/lr8f-green-3.log`: seven passed after repacking mutation seeds with IFDs
  before payloads; bounds reported above. No thresholds were weakened.
- `/tmp/lr8f-private-before.log` and `/tmp/lr8f-private-after.log`: one successful
  decode plus linear/RGB render each; test execution 1.59/1.56 seconds.
- `/tmp/lr8f-clean.log`: requested plain clean removed zero files (debug profile).
  First release gate launch `/tmp/lr8f-release-1.log` was deliberately interrupted
  during compilation, before tests, to clean the actual release artifacts.
  `/tmp/lr8f-clean-release.log`: 190 files / 91.0 MiB removed with
  `cargo clean --release -p raw-decode`. Complete gate restarted serially in
  `/tmp/lr8f-release-2.log`.
- `/tmp/lr8f-release-2.log`: **PASS**, all seven requested release crates,
  **1,499 passed, zero failed, 56 existing ignores**, including streaming-memory
  and latency assertions. Build took 6m 11s. No timing retry or threshold change
  was needed after the deliberate clean-build restart. Command:
  `cargo test --locked --release -p raw-decode -p image-core -p pipeline-cpu
  -p import-lrcat -p previews -p export -p tessera-ffi -- --test-threads=1`.
- `/tmp/lr8f-clippy-1.log`: `cargo clippy --locked --release --workspace
  --all-targets -- -D warnings` **PASS** on its first attempt (33.87 s).
- `/tmp/lr8f-fmt.log`: `cargo fmt --all -- --check` **PASS**;
  `git diff --check` **PASS**.

All runs use the requested PATH, external `$HOME/.cache/tessera-target/LR-8e`,
and `CARGO_BUILD_JOBS=4`. Release tests run with one test thread because other
lanes were building/testing on the machine. No test skips were added. Cargo.lock
and every board.json are unchanged from 85958b35.

Cherry-pick order: `ec2f312ac4bfc8717f7a103c23b000541cca025d` (RED),
`46965d1cfca6a95b1901e102586ea3685aaa2bdd` (fix), then the documentation commit
containing this LR-8f section. Each ends with the requested Claude Opus 5.5
co-author trailer.
