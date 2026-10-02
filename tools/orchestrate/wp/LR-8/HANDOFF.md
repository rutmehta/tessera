# LR-8: classic JPEG decoder groundwork; actual sample requires JPEG XL

Branch: `wp/LR-8-smart-preview-proxies`, local only, on top of `f5cce8ff`.

## Outcome and corrected blocker

**Offline proxy import is not implemented or ready for integration.** The
supplied real-format prerequisite still fails:

**decodes: no, dimensions: 2560 x 1707**

The earlier handoff's diagnosis was incomplete. `NO_JPEG` is true for this
LibRaw build, but the full-resolution LinearRaw IFD uses JPEG XL compression
(code 52546), not classic lossy JPEG compression (34892). The primary evidence
is the container's structural codec selection and the vendored decoder mapping:

- `crates/libraw-ffi/vendor/LibRaw/LibRaw-0.22.2/src/metadata/identify.cpp`,
  cases 52546 and 34892: the former selects `jxl_dng_load_raw_placeholder`,
  the latter `lossy_dng_load_raw`.
- `src/integration/dngsdk_glue.cpp`, `valid_for_dngsdk`: JPEG XL requires
  `USE_DNGSDK`, `qDNGSupportJXL`, and DNG SDK >= 1.7.

`zune-jpeg` decodes classic JPEG, not JPEG XL. Enabling `USE_JPEG` or adding
libjpeg would not resolve this sample. Neither was changed.

The coordinator's current exception permits **only** the `zune-jpeg` dependency
edge. A pending clarification requests the additional
`jxl-oxide.workspace = true` dependency in raw-decode. `jxl-oxide = "0.12"` is
already in the root workspace and version 0.12.6 is already locked. The proposed
additional lock change would be only `"jxl-oxide",` in raw-decode's dependency
list. That exception has **not** been granted or applied.

No private sample, pixels, content hashes, camera metadata, sample name, or
catalog-derived strings were committed. No GUI was launched. No Lightroom
managed directory was written. No CPU PNG was produced because decoding the
required sample remains blocked.

## Implemented and tested groundwork

- `raw_decode::lossy_dng::read`: additive bounded classic TIFF reader, both byte
  orders, cycle/IFD/tag/pixel budgets, root/SubIFD traversal, full-image selection,
  tiled or stripped compression-34892 LinearRaw with three JPEG components.
- It applies a 256-entry LinearizationTable, scalar/per-channel black and white
  levels, ActiveArea and integral DefaultCropOrigin/Size. It returns camera RGB
  samples and retains Orientation for the eventual source boundary.
- Both ColorMatrix and ForwardMatrix tags, calibration illuminants,
  AsShotNeutral, BaselineExposure, make/model, and opcode payloads are retained.
- Explicit Unsupported error for JPEG XL, rather than misidentifying its payload
  as classic JPEG or accidentally rendering its standard preview.
- `import_lrcat::smart_previews::SmartPreviewIndex`: pure validated UUID path
  derivation and `is_file` lookup using **AgLibraryFile.id_global**. It never
  enumerates, copies, writes or opens a Lightroom database.
- Synthetic 16x16 gradient JPEG generated with already-locked jpeg-encoder 0.6.1;
  committed generator source and instructions. TIFF wrappers are hand-written
  in memory. Tests cover both byte orders, SubIFDs, tiles/strips, crop and
  orientation metadata, calibration, malformed ranges, explicit JPEG XL
  rejection, and Adobe marker / RGB component-ID invariance.
- Gradient tolerance: 2.1 code values / 254 normalized black-to-white interval.

### JPEG colour handling

The DNG LinearRaw interpretation is authoritative. A private in-memory copy of
each JPEG omits Adobe APP14 because zune maps Adobe transform 0 to CMYK even for
three-component payloads. SOF/SOS component IDs are retained. After header
parsing, output ColorSpace is explicitly set equal to input ColorSpace:
YCbCr-to-YCbCr or RGB-to-RGB. This selects component interleaving without a
YCbCr-to-RGB colour transform. Synthetic tests require exactly equal decoded
samples with/without APP14 and with numeric/RGB component IDs.

### Deliberate limits; not end-to-end completion

The new decoder is an additive raw-decode API only. It is **not wired into
RawImage/Develop**, so no CPU/GPU renderer support is claimed. Orientation is
retained, not consumed. A selected ColorMatrix is exposed in RawMetadata;
dual-illuminant interpolation, ForwardMatrix-based rendering, BaselineExposure
application and LibRaw metadata parity are still pending integration. Matrix2
is selected when its calibration illuminant is D65, otherwise Matrix1 is the
fallback; this is not a completed DNG colour pipeline.

OpcodeList1/2/3 are preserved, **not applied** by this decoder. Embedded profile
tone curves and profile lookup tables are **unsupported**. The existing pipeline
has embedded-opcode stages, but they have not been connected to these camera-RGB
samples. Spatial BlackLevelRepeatDim, BlackLevelDelta, fractional crop geometry,
non-8-bit classic JPEG codes, planar JPEG, and non-three-channel data are
rejected. No rendered colour correctness claim is made.

Requirement 1 has only the read-only lookup and its tests; the catalog UUID
extraction and fixture bundle integration remain pending. Requirements 3–7
(import outcomes/options/counts, protected proxy edits, copy option, persisted
proxy/original mapping, relink, UI badge/export note, normalized geometry test)
remain unimplemented. Baked standard previews were not admitted as editable
sources. Index schema remains v9.

## Exact manifest / lock exception for Machine A approval

The only manifest delta is:

```diff
 [dependencies]
+zune-jpeg.workspace = true
```

Verified `git diff Cargo.lock` delta:

```diff
diff --git a/Cargo.lock b/Cargo.lock
index 88e72768..a0245aab 100644
--- a/Cargo.lock
+++ b/Cargo.lock
@@ -4107,6 +4107,7 @@ dependencies = [
  "libc",
  "libraw-ffi",
  "tempfile",
+ "zune-jpeg 0.5.15",
 ]

 [[package]]
```

No package additions or version changes. No other manifest or board.json changes.

## Machine A review: every changed engine file

- `crates/raw-decode/Cargo.toml`
- `crates/raw-decode/src/lib.rs`
- `crates/raw-decode/src/lossy_dng.rs`
- `crates/raw-decode/tests/smart_preview.rs`
- `crates/raw-decode/tests/lossy_dng.rs`
- `crates/raw-decode/tests/support/mod.rs`
- `crates/raw-decode/tests/fixtures/linear-gradient.jpg` (synthetic only)
- `crates/raw-decode/tests/fixtures/generate-gradient.rs`
- `crates/raw-decode/tests/fixtures/README.md`
- `Cargo.lock` (exact dependency edge above)

Other source changes:

- `crates/import-lrcat/src/lib.rs`
- `crates/import-lrcat/src/smart_previews.rs`
- `crates/import-lrcat/tests/smart_previews.rs`

No library, index, image-core, pipeline, FFI, or Swift source changes.

## Verification

Requested external LR-8 Cargo target directory; four Cargo jobs and four Rayon
threads. Default tests use synthetic/temp paths only, never ~/Pictures.

- RED: `cargo test --release -p raw-decode --test lossy_dng` failed because the
  new API did not exist. Commit `28be3ef4`.
- RED: `cargo test --release --locked -p import-lrcat --test smart_previews`
  failed because the new module did not exist. Commit `e8493bef` also adds the
  JPEG XL rejection regression and privacy-safe prerequisite result handling.
- `cargo test --release --locked -p import-lrcat -p raw-decode`: PASS (210 passed, 4 ignored), including
  the 20k synthetic catalog scale test. Private sample absent in this run, so
  its opt-in test explicitly skips; this is not sample-decoding evidence.
- Final targeted lossy-DNG tests (4) and smart-preview-index test (1): PASS.
- `cargo clippy --locked -p raw-decode -p import-lrcat --all-targets -- -D warnings`:
  PASS. Existing vendored C++ warnings still appear.
- `cargo fmt --all -- --check`: PASS.
- Opt-in private-sample test: FAIL; result stated above, no other photo data.
- Full requested five-crate gate, workspace-wide clippy, FFI/Swift gate and
  strict release Swift build: **not run** after identifying the new codec
  prerequisite. CPU/GPU rendering and the requested sample PNG are **not verified**.

## Resume decision

Authorize the already-locked jxl-oxide dependency edge (or provide a genuinely
compression-34892 prerequisite fixture). Implement JPEG XL tile decoding while
preserving camera-linear semantics, then make the actual sample decode and CPU
colour-check render pass before integrating proxy import, source admission,
protected edits, automatic relink and app presentation. Run all original gates
once that integration exists. The current commits are groundwork, not a claim
that Rut can edit the offline catalog yet.
