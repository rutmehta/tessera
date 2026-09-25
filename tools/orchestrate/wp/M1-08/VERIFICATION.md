# M1-08 handoff

Implemented `crates/export`, registered it in the workspace, and updated Cargo.lock. No commits were made.

## Verification

Executed the exact required chain, exit 0:

    cargo test -p export --release && cargo clippy -p export --all-targets -- -D warnings && cargo fmt --check

Evidence: `verification.log`. The vendored LibRaw build emits existing C++ warnings, but Rust Clippy with `-D warnings` passes. `git diff --check` also passes.

Executed the ignored benchmark separately, exit 0:

    cargo test -q -p export --release --test batch -- --ignored --nocapture

Evidence: `benchmark.log`. Five synthetic 3000×2000 decoded RGB fixtures, full-size JPEG q90: 1624.8 ms/image (batch wall time divided by five) in the recorded run. This is not a 45 MP raw performance measurement. A previous run was faster; no regression threshold is asserted on this shared host.

## Coverage

- JPEG decoded dimensions and ICC; all four output profiles parsed from all formats.
- TIFF8/TIFF16 decoded types and pixels; TIFF XMP tag 700; PNG iCCP/iTXt.
- All/CopyrightOnly/None privacy behavior, embedded packet and sidecar consistency.
- Rating/label interoperability through the sidecar crate.
- Naming tokens and traversal rejection; malformed quality/bit depth/resize rejection.
- Lanczos constant preservation, anti-aliasing and independent reference comparison.
- All resize modes and distinct sharpening presets.
- Five fixtures at long edge 1024, exactly five progress events.
- Cancellation after first publication, no partial files, and successful resume of remaining items.
- Cancellation during codec writes, pre-cancel, duplicate naming and no-clobber behavior.
- Oversized APP1 failure leaves no temporary/final output.

## Integration decisions and limits

`render_full` in `src/lib.rs` is the one renderer adapter to replace when M1-06 lands. Its downstream contract is floating-point display-referred sRGB. The current `pipeline_cpu::render` is sRGB8, so TIFF16 cannot recover upstream precision or out-of-sRGB gamut. CPU rendering itself is currently non-interruptible; cancellation is observed immediately after it returns and per filter/ICC row or codec I/O thereafter.

`ExportImage` borrows decoded RenderSource plus stable name/sequence/date context and optional source XMP. `ExportItem` adds the recipe. Batch callbacks execute on the caller thread; completed results and individual failures are returned in input order. Resume resubmits `BatchReport::remaining()` items with their original sequence/date and a fresh token. Existing output is never silently overwritten or assumed correct.

CopyrightOnly strips selection and all other non-copyright properties. None emits no XMP or sidecar. All preserves supplied source XMP with updated selection. No opaque EXIF copying or Extended XMP support is claimed.

See `crates/export/README.md` for API semantics, memory admission bounds, privacy, publication and resume details. Publication is cancellation-safe but not a crash-atomic two-file transaction.

## Environment

CARGO_TARGET_DIR remained `/Users/rutmehta/.cache/tessera-target/M1-08`. A transient disk-full error interrupted a patch without changing its targets. After removing only this package's generated Cargo artifacts with `cargo clean -p export`, the patch and full verification succeeded. Only Cargo.toml, Cargo.lock, crates/export/** and this WP's evidence paths changed.
