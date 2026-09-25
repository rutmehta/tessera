# Export (M1-08)

`export_one(&ExportImage, &Recipe, &ExportSettings) -> EngineResult<PathBuf>`

`export_batch(&[ExportItem], &ExportSettings, impl Fn(Progress), &CancellationToken) -> EngineResult<BatchReport>`

Inputs borrow already-decoded `pipeline_cpu::RenderSource` (RGB or CFA plus raw metadata), a source basename, a stable sequence number, capture-date text, and optional source XMP. No source files or source sidecars are changed. Decode and catalog lookup belong to the caller.

Defaults: JPEG q90, sRGB, no resize, no sharpening, all supplied XMP metadata, `{name}-{seq}`, current directory. JPEG quality is 1–100; TIFF bits must be 8 or 16. Resize supports long edge, aspect-preserving fit, and percentage (100 = original size). Enlargement is allowed. Invalid/nonfinite scales, zero dimensions and outputs/intermediate resize buffers above 100 megapixels are rejected. Naming expands `{name}`, `{seq}`, `{date}` once, then rejects separators, controls, colon, unexpanded braces and empty/dot basenames. Sequence/date come from the caller, not wall-clock time, so retries retain identical names.

## Render and colour

The renderer integration is `render_full` in `src/lib.rs`. It calls `pipeline_cpu::render_managed_scaled` at full resolution with the selected export ICC profile. Scene-linear Rec.2020 is tone-mapped and converted directly to destination-encoded float RGB, without an sRGB8 intermediate. Proof state is explicitly cleared because monitor proof simulation must not be baked into a document. Unsupported recipe operators remain errors.

Processing order: float render including shared `color_mgmt::Transform` (relative colorimetric, BPC enabled) → row-parallel separable Lanczos-3 (anti-alias support expands when reducing) → Gaussian unsharp mask → final quantization and encode. Filters operate on destination-encoded floats. Screen/matte/glossy use (sigma, amount) of (.6,.5), (1.2,1), (.8,.7). `color_mgmt::Registry` supplies all built-in ICC profiles; this crate no longer constructs profiles or invokes lcms2 transforms itself. The codec receives already-converted pixels and must not convert them a second time. Float samples are clamped/rounded to u16 before 8/16-bit encoding; destination ICC bytes are embedded in JPEG APP2, PNG iCCP and TIFF tag 34675. Direct lcms2 remains only as a dev dependency for independent ICC parsing tests.

The TIFF16 integration regression checks exact equality to the managed float render followed by final quantization, including a saturated wide-gamut sample and distinguishable sub-8-bit differences. ExportSettings still has no custom ICC/intent fields and does not claim printer/CMYK or HDR export. Shared transforms are built per render; cross-export registry/transform caching is not implemented.

## Metadata and publication

All preserves the supplied XMP packet (including foreign properties) and replaces selection metadata using `sidecar`'s standard rating/flag/Lightroom-label mapping. CopyrightOnly rebuilds a clean packet containing only copyright, not ratings, marks, GPS or other foreign properties. None writes neither embedded XMP nor an XMP sidecar. Privacy policy applies equally to the exported image and its sidecar. This API consumes XMP, not opaque EXIF/IPTC blobs.

JPEG uses standard APP1 XMP, TIFF uses tag 700, and PNG uses `XML:com.adobe.xmp` iTXt. The standard sidecar path comes from `Sidecar::paths` (`photo.jpg.xmp`, for example). Oversized JPEG APP1 packets fail rather than silently truncate; Extended XMP is not implemented.

Each output is encoded to a same-directory temporary file and synced before publication with tempfile's no-clobber persistence. The optional sidecar is published first and the image last; if image publication fails, the newly published sidecar is rolled back. Existing files are never overwritten. Cancellation is checked before publication, not between the two short commit operations. This provides cancellation/error safety, not crash-atomic two-file transactions.

## Batch and resume

Run the synchronous batch function on an Export-priority background job, not the UI thread. A local Rayon pool is capped by available cores, item count and a conservative 512 MiB scratch estimate (64 bytes per largest source/output/intermediate pixel), with a minimum of one image. The estimate is not a hard RSS cap and excludes borrowed decoded inputs. Admission occurs in bounded waves so nested row work cannot admit an unbounded number of full-image buffers. A bounded channel carries prepared temporary files, not pixel buffers, to the coordinator.

Publication and progress callbacks run serially on the calling thread. The callback has no Send/Sync requirement. Each non-cancelled result produces one progress event; `completed` counts successes, `index` addresses the input slice, and `total` is this invocation's length. Cancellation does not discard completed results: the report is in input order, with `Err(Cancelled)` for unfinished items. Resubmit items indexed by `report.remaining()` with a fresh token and their original sequence/date context. The report also exposes individual errors for retry decisions. A preflight naming/settings failure returns an outer error before launching workers. Case-folded duplicate names are rejected conservatively for macOS filesystems.

Cancellation is polled per resize/sharpen/quantization row and on codec writes/seeks. The current synchronous CPU renderer itself has no cancellation API, so an in-flight render must finish before observing cancellation. No new image is published after a callback cancels the token.

## Verification

- `cargo test -p export --release`
- `cargo clippy -p export --all-targets -- -D warnings`
- `cargo fmt --check`
- `cargo test -p export --release --test batch -- --ignored --nocapture`

Tests use deterministic generated RGB fixtures (no network or raw-fixture skips). They cover format/profile/privacy combinations, parsed ICC/XMP, TIFF depth, selection mapping, sizing/sharpening, Lanczos numerical invariants/reference comparison, progress, cancellation during encoding, cancel/resume, duplicate names, no-clobber behavior and cleanup after encoder failure. The ignored benchmark uses five synthetic 3000×2000 RGB fixtures, full-size JPEG q90, and prints completion times plus amortized ms/image. It is a CPU-export smoke benchmark, not a 45 MP raw performance claim.

Keep `CARGO_TARGET_DIR` pointing outside the checkout on this host.
