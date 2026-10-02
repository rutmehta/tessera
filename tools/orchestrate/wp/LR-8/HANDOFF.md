# LR-8: stopped at the required decode prerequisite

Branch: `wp/LR-8-smart-preview-proxies`. Local only.

## Outcome

Offline proxy import is **not implemented**. Requirement 2 explicitly requires
stopping if this LibRaw build cannot decode lossy linear DNG, without adding
dependencies. The opt-in real-format test reproduced that condition.

Real-sample result: **decodes: no; dimensions: 2560 x 1707**.

No private sample, pixels, content hashes, EXIF, sample filename, or catalog
strings were committed. The sample was referenced in place, read-only. No GUI
was launched and no Lightroom-managed directory was written.

## Missing capability and evidence for Machine A

`crates/libraw-ffi/build.rs` does not define `USE_JPEG`, provide JPEG headers,
or link a JPEG library. The vendored LibRaw `internal/defines.h` consequently
defines `NO_JPEG`. Its `src/decoders/dng.cpp` compiles an empty
`lossy_dng_load_raw` implementation under that flag; its identification code
also marks that decoder unsupported when JPEG support is absent.

The real test successfully opens the container with `RawFile::open`, then fails
at `RawFile::unpack`. The missing build capability is LibRaw's JPEG-backed
lossy-DNG decoder. The vendored configure script's supported setup checks for
`jpeglib.h` and `jpeg_mem_src`, defines `USE_JPEG` and `USE_JPEG8`, and links
`libjpeg`. This lane did not install or enable those dependencies.

After Machine A resolves that prerequisite, RGB admission still needs work:
`RawSource` exposes CFA decode, and the existing bounded `linear_dng` reader
supports uncompressed LinearRaw strips, not Lightroom's JPEG-compressed DNG.
Successful LibRaw unpack alone will not establish CPU/GPU Develop support.
The test is deliberately a prerequisite probe, not an end-to-end rendering test.

## Changes requiring Machine A review

- `crates/raw-decode/tests/smart_preview.rs`: additive opt-in integration test
  invoking existing LibRaw open/unpack APIs. No production engine files changed.

RED commit: `887563d0` (`test(LR-8): expose lossy smart-preview LibRaw decode blocker`).
There is no `feat(LR-8):` commit because the required stop condition was reached.

## Verification

Using the requested PATH, external LR-8 target directory, four Cargo jobs and
four Rayon threads:

- `TESSERA_SMART_PREVIEW_SAMPLE=<private local DNG> cargo test --release --locked -p raw-decode --test smart_preview -- --nocapture`:
  **RED**, `lightroom_smart_preview_libraw_decode` fails at unpack.
- Same targeted command without the environment variable: passes with an
  explicit opt-in skip message; this does **not** establish decode support.
- `cargo fmt --all -- --check`: passes.
- Vendored C++ compilation emitted existing deprecation, macro-redefinition,
  sign-comparison and unused-parameter warnings.
- Full multi-crate release tests, workspace clippy, FFI build, Swift gate and
  strict release Swift build were not run after the explicit decode stop.
- CPU/GPU rendering, import plan/apply, protected proxy sidecars, copy option,
  relinking, badges/export note and normalized geometry remain unimplemented
  and unverified for this feature.

No Cargo.lock, board.json, dependency, index-schema or app changes.

## Resume

Machine A must first supply a build with lossy-DNG decode support, then rerun
the opt-in test. Continue the requested synthetic-fixture RED tests and proxy
implementation only after that prerequisite passes. Do not substitute baked
standard previews as editable sources.
