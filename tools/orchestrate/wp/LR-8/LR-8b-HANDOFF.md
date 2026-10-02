# LR-8b: catalog orientation and shared RAW exposure

This follow-up supersedes the orientation, baseline-exposure and private-catalog
validation limitations in the earlier LR-8 handoff. Local branch only; no GUI launch.
Core implementation: `c42dae72`, preceded by five LR-8b test commits.
Final engine follow-up: `57a23888`; cache, white-point, viewport admission and
comparison-thumbnail changes also have tests-first commits.
The branch is rebased, without squashing, onto `04217312` (LR-5 stack), retaining
LR-5 apply-time mask resolution/pins and LR-7 single Import history finalization.

## Rendering contract

Catalog orientation is absolute: AB=1, BA=2, CD=3, DC=4, CB=5, BC=6, AD=7, DA=8.
Import retains it in the recipe. CFA reconstruction and camera calibration stay
in the sensor frame; the common CPU tail orients the active image before local
masks, Upright and crop. Presentation sees orientation 1, preventing double rotation.
Relinking selects the original while retaining the proxy recipe owner and the
same absolute orientation. RGB originals replace, rather than compose, EXIF
orientation. Synthetic non-square coverage exercises all eight transforms,
normalized masks/crop/Upright, relink and export. Catalog-oriented camera sources
use CPU rendering; resident GPU output is explicitly declined.

The scalar CPU path now uses the host mask cache, including imported LR-5 rasters,
for both Native and Adobe processing. Missing imported rasters retain the existing
pending/empty behavior. This does not add unsupported depth, retouch or lens-blur
support to external camera-linear sources. Named external lens profiles have no
viewport resolver. The draw-only admission now leaves that correction undrawn,
retains the full reference in the recipe/XMP, and reports `/lens/profile` through
ignored-settings. Other lens/manual edits remain active. This applies equally to
original and proxy viewports; explicit exports still require the named profile's
calibration rather than silently substituting one.

## Exposure diagnosis

The proxy already applied BaselineExposure; its darker output was not caused by
missing exposure gain, incorrect JXL normalization, or doubled white balance.
The actual consistency bug was the original CFA path: it discarded DNG
BaselineExposure. The decoder now retains it and both source kinds apply the
same `2^EV` gain exactly once in the common camera-profile stage. Native preview
cache generator revision is 3 so old cached prefixes cannot silently survive.
Explicit DCP rendering also retains the gain once.

A later deterministic contact candidate exposed another valid DNG variant:
AsShotNeutral absent, AsShotWhiteXY [0.3457, 0.3585], AnalogBalance [1, 1, 1],
and no CameraCalibration tags. The reader now converts the white xy value to
camera neutral through the same selected ColorMatrix. It rejects simultaneous
neutral/xy tags, invalid chromaticities and nonidentity extra calibration in this
new branch. Synthetic tests cover the conversion, IFD0-to-SubIFD inheritance and header-only projection.
This follows [Adobe DNG chapter 6](https://www.kronometric.org/phot/processing/DNG/dng_spec_1.4.0.0.pdf)
with the existing documented single-matrix limitation.

Private sample tag projection (numeric values only):

- Orientation: 8. The inspected sample actually contains this tag; catalog
  orientation still takes precedence for imported sources.
- BaselineExposure: 0.35. BaselineExposureOffset and DefaultBlackRender: absent.
- ProfileToneCurve: absent. ProfileLookTableDims: [36, 8, 16]; 13824 data values.
  ProfileHueSatMapDims: [90, 30, 1]; Data1 and Data2 each contain 8100 values.
- WhiteLevel: [65535, 65535, 65535]; BlackLevel: [0, 0, 0].
  LinearizationTable: absent. BitsPerSample: [16, 16, 16].
  SampleFormat: absent (unsigned integer default).
- AsShotNeutral: [0.463768, 1, 0.562637].
- ColorMatrix1: [[0.8693, -0.3745, -0.0467], [-0.5349, 1.2601, 0.3098],
  [-0.0367, 0.0752, 0.7041]].
- ColorMatrix2: [[0.8322, -0.3112, -0.1047], [-0.6367, 1.4342, 0.2179],
  [-0.0988, 0.1638, 0.6394]].
- ForwardMatrix1: [[0.6637, 0.2038, 0.0968], [0.2768, 0.8043, -0.0812],
  [0.0259, -0.2161, 1.0153]].
- ForwardMatrix2: [[0.5536, 0.3713, 0.0394], [0.2424, 0.9215, -0.1639],
  [0.0149, -0.1266, 0.9368]].

JXL integer floats are normalized by 65535, restored to code units, then DNG
black/white-normalized. Decoding requests the source encoding with NullCms; it
introduces no display gamma transform. Default Tessera processing does not select
embedded DNG hue/saturation/look tables automatically for either source kind.
No attempt is made to imitate Apple's default RAW look. Calibration provenance
can still differ: LibRaw may choose its camera matrix while the external DNG
reader selects the embedded D65 matrix. The matching private original is not
available, so exact original/proxy pixel equality is not established.

Measured default downsampled RGB means (same 256-by-256 comparison):

| Source | Tessera | Apple |
| --- | --- | --- |
| Private proxy | 46.34 / 54.00 / 65.09 | 62.42 / 74.93 / 96.65 |
| Repository ordinary DNG | 75.94 / 57.70 / 48.77 | 101.55 / 78.78 / 61.25 |

The ordinary fixture has BaselineExposure -0.5; before correction its full render
means were 91.14 / 70.05 / 58.16. The private proxy already included +0.35 EV,
so its expected default appearance is effectively unchanged by moving the gain.
Both comparisons show that the Apple difference is broader than the proxy path.

## Source-safe profiling

The opt-in aggregate harness uses the copied catalog and an explicit smart-preview
bundle override. All resolved originals and proxies are registered read-only;
recipe and XMP destinations are checked to be inside the fresh app directory
before plan/apply. No source permissions are changed. Header-only DNG metadata
projection prevents indexing from decoding every compressed preview.

Phase byte counts are net app-directory growth, not physical filesystem I/O.
Other build/test jobs were running during this measurement; wall times describe
this run rather than an isolated throughput benchmark.
Per-phase RSS is sampled every 100 ms; process peak RSS is also retained.
Comparison pairs are selected every floor(comparable-proxy-count / 12) entries
after sorting by catalog image id. Eligibility requires an existing standard-preview
file; the first selection from all proxies had no reference JPEG, so that
comparison attempt stopped without altering the completed apply measurement. CPU Develop uses the imported recipe through the viewport's
supported-settings admission and imported mask hooks, then reduces to 1024 px.
Largest standard-preview JPEGs are preserved unchanged. The reader supports
legacy `.lrprev`, newer `.lrfprev` / `.lrmprev` containers and split JPEG levels
for the current digest, selected by encoded dimensions. Integral SQLite REAL
image IDs are accepted in the preview index only; fractional or out-of-range
values are rejected. Synthetic regression tests cover both observed cache
compatibility gaps. Luminance MAD uses
64-by-64 images and Rec.709 weights; orientation/aspect is an image-based heuristic
(2 percent aspect tolerance and identity within 0.25 MAD of the best dihedral
transform), not a pixel-parity guarantee.

Only the two approved raw-decode dependency edges change Cargo.lock:
`jxl-oxide` and `zune-jpeg 0.5.15`. No board changes or private artifacts are committed.

The immutable M1/M2 RAW goldens retain their historical zero-baseline calibration.
They are not regenerated. Separate tests verify the ordinary fixture's -0.5 EV
metadata and the exact exposure gain for CFA and external camera-linear sources.
The synthetic import golden is updated because recipes now retain absolute
catalog orientation; the per-code tests independently verify that field.

## Completed catalog apply

21,656 total images; 8 online originals; 19,726 offline proxies; 1,922 offline
without proxies. Imported and indexed: 19,734. Skipped: 1,922. Virtual copies: 0.
Resumed: 0. Cancelled: false.

| Phase | Seconds | Sampled peak RSS MiB | Process peak RSS MiB | Net app bytes written |
| --- | ---: | ---: | ---: | ---: |
| open | 15.124 | 542.48 | 552.59 | 403,576 |
| plan | 57.170 | 606.84 | 606.88 | 0 |
| apply | 673.783 | 796.69 | 796.69 | 7,002,580,988 |

The full source listing matched before and after apply, including every entry
path, size, mtime and mode. Both aggregate snapshots: count 89,557; total size
20,297,801,488 bytes; newest mtime 1790832250649389090 ns. No source file was
created, removed, resized, retimestamped or chmodded.

## Comparison deliverables

Twelve pairs were produced from 14,924 comparable offline proxies, every 1,243rd
entry in catalog-id order. All 24 private files exist; all Tessera images have a
1024-pixel long edge and preserve their rendered aspect. Eight recipes reference
unavailable named lens profiles, retained but undrawn as described above.
Eleven pairs pass the orientation/aspect heuristic. The last pair has an aspect
mismatch against the cached reference; this comparison is not claimed as visual
parity. All eight orientation transforms pass the independent synthetic tests.

| Pair | Luminance MAD, 8-bit | Orientation/aspect matches |
| --- | ---: | --- |
| 01 | 31.8893 | true |
| 02 | 10.9958 | true |
| 03 | 31.3023 | true |
| 04 | 48.1911 | true |
| 05 | 12.3985 | true |
| 06 | 35.6865 | true |
| 07 | 21.1284 | true |
| 08 | 40.2302 | true |
| 09 | 39.1257 | true |
| 10 | 32.5822 | true |
| 11 | 47.4895 | true |
| 12 | 47.8939 | false |

The source listing was rechecked after the final comparison render: the same
count, size, newest mtime and exact per-entry listing remained unchanged.

## Delivery gates

Environment: the lane's external Cargo target, four Cargo jobs and four Rayon
threads; MACOSX_DEPLOYMENT_TARGET=15.0.

- `cargo test --release --locked --no-fail-fast -p libraw-ffi -p raw-decode
  -p import-lrcat -p sidecar -p pipeline-cpu -p pipeline-adobe -p image-core
  -p pipeline-gpu -p export -p merge -p tessera-ffi -p tessera-mcp -p library
  -p cull`: passed, 1,831 tests, zero failures, 64 suite-declared ignored tests.
  No command-level exclusions. The ignored private profile was separately run
  successfully, including the completed apply and final 12-pair render.
- Workspace `cargo clippy --workspace --locked --all-targets -- -D warnings`: passed.
- `cargo fmt --all --check`: passed.
- `apps/mac/build-ffi.sh`: passed; UniFFI bindings regenerated.
- `swift build -c release --product Tessera -Xswiftc -strict-concurrency=complete
  -Xswiftc -warnings-as-errors`: passed.
- `tools/orchestrate/swift-gate.sh`: `SWIFT GATE OK`, 920 XCTest cases with
  three skipped and zero failures, plus five Swift Testing cases passed.

The toolchain emits existing native LibRaw deprecation warnings and a linker
warning about a BLAKE3 object built for a newer deployment target. These did not
fail the requested gates. No foreground GUI was launched.
