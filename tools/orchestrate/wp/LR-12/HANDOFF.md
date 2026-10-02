# LR-13 (was LR-12): Smart Preview proxy thumbnails, analysis and renderability

Machine B, branch `local/rut-build`, pushed to `wp/INT-1-rut-build`. Commits sit
on top of `6682ca82`; nothing was rebased, amended or force-pushed. This file
stays at the historical LR-12 path by coordination request.

The lane was started by a Codex worker (commits up to `d986db0d` plus a large
uncommitted diff) and finished by Claude Opus 5.5. Commits that mainly carry the
Codex worker's uncommitted work say so in their body.

## Review map for Machine A

### LR-13a — minimal, contiguous, gated and pushed first (`a6b8ab39..df72fcb7`)

Thumbnails and analysis through the Smart Preview decoder at thumbnail size, and
the proxy CPU-reference rejection fix. 20 files, +867 / -63 for `bd4c71c7..df72fcb7`;
no Swift, no FFI surface change, no Cargo.lock change.

| Commit | What | Files |
| --- | --- | --- |
| `a6b8ab39` test | Synthetic JPEG XL LinearRaw fixture; reproduces LibRaw `-2` in analysis | `raw-decode/tests/fixtures/{README.md,linear-gradient-jxl.dng}`, `tessera-ffi/tests/lrcat.rs` |
| `a1c269df` fix | Analysis and assist share the imported preview route | `tessera-ffi/src/{assist.rs,preview.rs}` |
| `d986db0d` test | RED: culling, Adobe preview parity, proxy export warning | `export/tests/smart_preview_admission.rs`, `tessera-ffi/tests/lrcat.rs` |
| `7907804b` fix | **Proxy CPU-reference rejection fix**: `CameraLinearProxy::render_plan`, applied at the pipeline-cpu, pipeline-adobe and image-core camera-linear entry points; embedded DCP for the Adobe renderer; no second Native tone curve on Adobe display output | `pipeline-cpu/src/{render.rs,smart_preview.rs,smart_preview_codec.rs}`, `pipeline-adobe/src/render.rs`, `image-core/src/{render.rs,smart_preview_render.rs,source.rs}`, `image-core/tests/lrcat_linear.rs` |
| `9fb8b532` fix | Export from an external proxy uses the same plan, with a quality warning (required by the committed RED test in `d986db0d`) | `export/src/lib.rs`, `export/tests/smart_preview_admission.rs` |
| `e4fadad9` fix | **Thumbnail, loupe, analysis and culling routing** through `catalog::open_image` and the viewport settings filter | `tessera-ffi/src/{preview.rs,assist.rs,session.rs,export.rs,masks.rs}`, `cull/src/{lib.rs,grouping.rs}`, `tessera-ffi/tests/lrcat.rs` |
| `2aaf1283` test | RED: thumbnail request returns the level-0 frame (M3) | `tessera-ffi/src/preview.rs` |
| `df72fcb7` fix | **Thumbnail size** (M3): render the coarsest level that covers `max_px` | `tessera-ffi/src/preview.rs` |

The four RED tests of `07337d42` (`pipeline-cpu/tests/lrcat_dng.rs`, `lr12_*`)
turn green at `7907804b`.

### Broader — after LR-13a, each independently droppable (`df72fcb7..HEAD`)

| Commit | What | Files |
| --- | --- | --- |
| `ab924590` feat | Identity-oriented, Native-process external LinearRaw may use the resident GPU tail. **Changes an existing admission assertion** in `export/tests/lrcat_jxl.rs` (was: every external DNG declines the resident tail) | `pipeline-cpu/src/smart_preview.rs`, `image-core/src/{render.rs,resident_render.rs,smart_preview_render.rs}`, `pipeline-gpu/tests/smart_preview.rs`, `export/tests/lrcat_jxl.rs` |
| `8c2b8e10` fix | Merge input adapter reads JPEG XL LinearRaw | `tessera-ffi/src/merge.rs` |
| `87e70eec` fix | Import-time mask extent from the LinearRaw header, LibRaw fallback kept | `tessera-ffi/src/lrcat.rs` |
| `cca99b5b` feat | `DevelopSession.render_notices` FFI + Mac status-line notice + regenerated bindings | `tessera-ffi/src/{develop.rs,masks.rs}`, `tessera-ffi/tests/lrcat.rs` (one assertion), `apps/mac/**` (13 files) |
| `c5132de6` test | Opt-in `#[ignore]` measurement harness | `tessera-ffi/src/lrcat_profile.rs` |
| this commit | HANDOFF | `tools/orchestrate/wp/LR-12/HANDOFF.md` |

Broader commits that precede LR-13a in history (already on the branch before
this continuation): `c5f6ae92` + `b47a482b` (per-level Lightroom preview index
in the import sheet), the recipe-audit half of `07337d42`
(`tessera-ffi/src/lrcat_profile.rs`; its `pipeline-cpu/tests/lrcat_dng.rs` half
is the LR-13a RED set), `bd4c71c7` (docs).

### Deliberately not carried over from the Codex worker's diff

`tessera-ffi/src/catalog.rs` reordered `EmbeddedMetadata::read` so the lossy-DNG
header reader ran before LibRaw for every `.dng` and propagated its I/O errors.
That conflicts with ruling B1 (ordinary DNGs must fall through to LibRaw
silently) and drops capture time, camera and lens for any DNG LibRaw can open.
No test needs it, so it was left at `HEAD`.

## What was broken at each entry point

| App entry point | State at `6682ca82` | Fix |
| --- | --- | --- |
| `Engine.embedded_preview` (grid thumbnail, loupe) | Decoded correctly, then passed the full recipe to the CPU/Adobe renderer at scale 1: rejected for 19,655 of 19,727 proxies ("non-default operator not implemented by CPU reference renderer"), so the grid was blank | `e4fadad9` route + filter, `7907804b` plan, `df72fcb7` size |
| `Engine.analyze_image`, assist, quality, faces, embeddings (`analysis_rgb`) | Ordinary LibRaw preview decode: `LibRaw error -2` for every JPEG XL LinearRaw proxy | `a1c269df` |
| Cull session near-duplicate grouping | LibRaw embedded-preview hash: error per proxy | `e4fadad9` (host preview provider) |
| `Engine.open_develop_session`, refresh, attached surface | Smart Preview decoder was already used; 702 proxies failed admission on one optional setting | `7907804b` |
| `DevelopSession.get_histogram` | Reads the completed Develop frame, so it had nothing to read when the frame was rejected | `7907804b` |
| `Engine.export_batch`, print render | Decoder already used; 19,655 rejected by the settings validator; no proxy warning; Adobe recipes exported through the Native tone path | `9fb8b532`, `e4fadad9` (embedded DCP, no segmenter load) |
| Merge input | LibRaw `-2` | `8c2b8e10` (broader) |
| Import mask extent | LibRaw could not size the proxy | `87e70eec` (broader) |

## M3 answer (thumbnails)

- Settings: `render_imported` renders `develop::session_renderable(...)`, the
  filter the Develop viewport draws, not the full recipe.
- Size: it renders the coarsest engine level whose long edge still covers the
  request. 256 px on a 2560 px proxy is level 3 (320 px); the 2048 px loupe tier
  is level 0. Culling hashes the 256 px tier.
- Residual, stated plainly: on the scalar camera-linear route the engine still
  develops the 2560 px proxy once on the CPU and then reduces, which is the
  Develop coarse-level contract (it keeps imported mask rasters and Adobe parity
  identical to the viewport). Tiles, display encoding and stitching are
  thumbnail-sized. Pre-reducing the camera-linear pixels would be faster but
  changes mask-raster extents and pixel-radius operators; that belongs in LR-8.
- Not done: imported previews are not looked up in the preview store before
  rendering, so analysis re-renders a proxy even when the grid already has it.
- Test: `preview::tests::lr13_imported_proxy_thumbnail_renders_at_thumbnail_level`.
  Its first version (`2aaf1283`) also compared 8-bit means of the level-0 frame
  and the thumbnail within 3 codes. That oracle was wrong for a steep gradient
  because the viewport reduces in linear light before the display transform; the
  fix commit replaces it with a per-sample check that each thumbnail value lies
  inside its source bin, and keeps the dimension assertions unchanged.

## Render plan (proxy CPU-reference rejection fix)

`CameraLinearProxy::render_plan(settings, mask_hooks)` returns a render-only
copy of the settings and the list of omitted field names. It is the identity
for generated (non-external) Smart Previews, so their immutable prefix contract
and the original-required export rule are unchanged. For external LinearRaw:

| Field | Plan |
| --- | --- |
| `/decode`, `/linearize`, `/demosaic`, `/denoise` | ignored: no mosaic |
| `/white_balance/mode` = Auto | As Shot |
| `/camera_profile/look` | default look |
| `/output/hdr`, `hdr_headroom_stops` | SDR |
| `/lens/profile` = database profile | none (kept when the caller supplies a profile or database) |
| `/effects/lens_blur`, `/locals/retouch` | omitted |
| `/locals/adjustments` groups needing depth or unavailable AI rasters | that group disabled |

The saved recipe is never modified; export writes metadata from the saved recipe.

## Measurement (aggregate only; nothing private is committed)

Source: the scratch catalog copy and read-only access to the Smart Previews
bundle. Offline is forced for every image that has a Smart Preview. Originals
that resolve now: 21,643 catalog masters (count only).

Admission audit, `lr13_proxy_admission_from_env`, 19,727 proxies:

| App route | Before | After |
| --- | ---: | ---: |
| Thumbnail / loupe preview | 72 | 19,727 |
| Develop (mask hooks, CPU fallback) | 19,025 | 19,727 |
| Export from proxy | 72 | 19,727 |

"After" was re-run on the final code in this continuation. "Before" is the
Codex worker's recorded run of the same audit against the `bd4c71c7` validators
(same code as `6682ca82` for these paths); it was not re-run here. The preview
"before" counts validator admission only: analysis additionally failed for all
19,727 at `6682ca82` in the LibRaw decode, which a validator cannot see and the
synthetic regression reproduces.

Residual failures after: none (0 in each route). Settings degraded with an info
note, by field (counts overlap, they are not failing images):

| Field | Proxies |
| --- | ---: |
| `output/hdr` (flag or headroom; headroom alone 16,658, flag 366) | 16,658 |
| `camera_profile/look` | 15,543 |
| `lens/profile` | 11,978 |
| `locals/adjustments` | 625 |
| `effects/lens_blur` | 417 |
| `locals/retouch` | 285 |
| `white_balance/mode` | 52 |

The audit is an admission count: it reads real proxy and embedded-profile
headers, supplies synthetic 2x2 pixels and calls renderer admission.

Full renders, `lr13_app_develop_sample_from_env`, final code, four workers,
1,535 s: a deterministic every-98th sample of 200 proxies plus the 12
reference pairs (211 distinct photos) through the real FFI calls.

| Route | Rendered | Failed |
| --- | ---: | ---: |
| Develop session frame to an IOSurface + nonempty histogram | 211 | 0 |
| Grid thumbnail (256) | 200 | 0 |
| Loupe (2048) | 200 | 0 |
| Analysis (quality) | 200 | 0 |
| PNG export from proxy, at proxy resolution, with warnings file | 200 | 0 |

The validators predicted success for all 211 and all 211 rendered. Outputs are
only in the scratch `lr13-out/contact` directory; the scratch app directory was
removed by the harness. The live Tessera library, application-support directory
and the live catalog were never opened; nothing was written under
`~/Pictures/Lightroom`; no GUI was launched.

## Gates

LR-13a at `df72fcb7` (release artifacts of the touched crates cleaned first;
5 Cargo jobs, 5 Rayon threads, LR-8 target directory):

| Gate | Result |
| --- | --- |
| `cargo test --release` raw-decode, pipeline-adobe, pipeline-cpu, pipeline-gpu, image-core, import-lrcat, engine-api, previews, export, cull, tessera-ffi, tessera-mcp | PASS: 1,993 passed, 0 failed, 70 ignored, one attempt, no exclusions |
| Workspace clippy, all targets, `-D warnings` | PASS |
| `cargo fmt --all --check` | PASS |
| `apps/mac/build-ffi.sh` | PASS; bindings differed from the tracked file only by 10 trailing-whitespace lines, restored |
| `tools/orchestrate/swift-gate.sh` | SWIFT GATE OK: 920 tests, 3 skipped, 0 failures |
| Strict release `Tessera` build | PASS |

Final gates at `c5132de6` (all code; this docs commit follows; release and dev
artifacts of the touched crates cleaned first):

| Gate | Result |
| --- | --- |
| `cargo test --release`, same twelve crates | PASS: 1,995 passed, 0 failed, 72 ignored, one attempt, no exclusions |
| Workspace clippy, all targets, `-D warnings` | PASS |
| `cargo fmt --all --check` | PASS |
| `apps/mac/build-ffi.sh` | PASS; tracked bindings unchanged afterwards (worktree clean) |
| `tools/orchestrate/swift-gate.sh` | SWIFT GATE OK: 921 tests, 3 skipped, 0 failures |
| Strict release `Tessera` build | PASS |

No wall-clock test needed a rerun in either gate run.

Cargo.lock, dependency manifests, board.json and existing goldens are unchanged.
No private pixel, path, file name or catalog-derived string is committed.
