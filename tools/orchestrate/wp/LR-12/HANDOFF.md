# LR-13 (was LR-12): Smart Preview proxy thumbnails, analysis and renderability

Machine B, branch `local/rut-build`, pushed to `wp/INT-1-rut-build`. Commits sit
on top of `6682ca82`; nothing was rebased or force-pushed and no pushed commit
was changed. This file stays at the historical LR-12 path by coordination request.

**Combined tip for install** = integration `6682ca82` + LR-13 + hotfix LR-8e..8h
+ the Machine A approval conditions. See "Hotfix LR-8e..8h integrated" and
"Machine A approval conditions" below for what changed after `95cb645c`.

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

Moved into LR-13a by Machine A's ruling that the visible notice must ship with
it. These are not contiguous with the range above (they follow the first three
broader commits in history) but depend only on `7907804b`:

| Commit | What | Files |
| --- | --- | --- |
| `cca99b5b` feat | **Visible notice**: `ignored_settings` lists the omitted proxy fields (drives the existing Develop status line); `DevelopSession.render_notices` FFI; Mac status-line sentence per omitted setting; regenerated bindings | `tessera-ffi/src/{develop.rs,masks.rs}`, `tessera-ffi/tests/lrcat.rs`, `apps/mac/**` (13 files) |
| `e2438c72` refactor | Notice sentences in one function, no text change | `tessera-ffi/src/develop.rs` |
| `7df6f558` test | RED: lens blur / retouch wording; HDR note fired on headroom alone | `tessera-ffi/src/develop.rs`, `pipeline-cpu/tests/lrcat_dng.rs` |
| `348b74f7` fix | Wording fixed; HDR note only when HDR output is on | `tessera-ffi/src/develop.rs`, `pipeline-cpu/src/smart_preview.rs` |
| `e07fe9bc` test | Minimum notice covered through the FFI Develop session | `tessera-ffi/tests/lrcat.rs` |

The Rust-only minimum (if the Swift half of `cca99b5b` were dropped) is its
`develop.rs` + `masks.rs` hunks: the existing status line then reads
"Develop: N imported setting(s) are kept but not rendered yet".

The four RED tests of `07337d42` (`pipeline-cpu/tests/lrcat_dng.rs`, `lr12_*`)
turn green at `7907804b`.

### Broader — each independently droppable

| Commit | What | Files |
| --- | --- | --- |
| `ab924590` feat | Identity-oriented, Native-process external LinearRaw may use the resident GPU tail. **Changes an existing admission assertion** in `export/tests/lrcat_jxl.rs` (was: every external DNG declines the resident tail) | `pipeline-cpu/src/smart_preview.rs`, `image-core/src/{render.rs,resident_render.rs,smart_preview_render.rs}`, `pipeline-gpu/tests/smart_preview.rs`, `export/tests/lrcat_jxl.rs` |
| `8c2b8e10` fix | Merge input adapter reads JPEG XL LinearRaw | `tessera-ffi/src/merge.rs` |
| `70921d37` fix | Merge adapter offers only `.dng` files to the LinearRaw reader (hotfix integration) | `tessera-ffi/src/merge.rs` |
| `87e70eec` fix | Import-time mask extent from the LinearRaw header, LibRaw fallback kept | `tessera-ffi/src/lrcat.rs` |
| `c5132de6` test | Opt-in `#[ignore]` measurement harness | `tessera-ffi/src/lrcat_profile.rs` |
| `95cb645c`, `5d5be036`, final commit | HANDOFF and fixture README | `tools/orchestrate/wp/LR-12/HANDOFF.md`, `raw-decode/tests/fixtures/README.md` |
| `1f64ff6f` + `33e56d7d` | A RED Swift test for a persistent loupe notice and its revert; net zero (see "Deferred to LR-13b") | `apps/mac/Tests/**` |

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

## Measurement before the hotfix (aggregate only; nothing private is committed)

First measurement, at `c5132de6`. The before → after comparison stands; the
"after" figures and the note table are superseded by "Re-measurement on the
merged decoder" below (same admission counts, HDR note 16,658 → 366).

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

## Hotfix LR-8e..8h integrated

`git cherry-pick afe97d4c..38817f56` onto `95cb645c`: twelve separate commits
(`e9925602..e4372184`), original subjects, bodies and trailers byte-identical,
no `-x`.

- Conflict: one, in `crates/raw-decode/tests/fixtures/README.md` while applying
  `46965d1c` (LR-8f fix). Both texts kept: the LR-13 `linear-gradient-jxl.dng`
  paragraph, then the LR-8f APP14 / 16-bit JPEG XL paragraphs. No code conflict.
- `lossy_dng.rs`, `lr8e_safety.rs`, `lr8f_safety.rs`, `lossy_dng.rs` tests, the
  LR-8 HANDOFF and every hotfix fixture are byte-identical to `38817f56`
  (verified per blob). The LR-8f regenerated `linear-gradient.jpg` /
  `linear-gradient.dng` (Adobe APP14 transform 0) landed. The only differences
  in the hotfix's file set are the LR-13 fixture and README paragraphs.
- The hotfix's admission and validation rules are untouched. The two approved
  call sites (`image-core/src/source.rs`, `tessera-ffi/src/export.rs`) have no
  net change in the hotfix range and keep their LR-13 form.

Audit of every LR-13 use of the LinearRaw readers under the hotfix semantics
(None when not claimed, error when claimed but invalid):

| Caller | Reached by | Behaviour |
| --- | --- | --- |
| `image-core/src/source.rs` (`.dng` gate, approved site) | thumbnail, loupe, analysis, assist, culling, Develop via `catalog::open_image` | proxy claimed and decoded; ordinary DNG returns None and continues to LibRaw; other extensions never parsed |
| `tessera-ffi/src/export.rs` `Source::open` (`.dng` gate, approved site) | export, print | same |
| `tessera-ffi/src/merge.rs` `load_linear` | merge input | `70921d37` adds the same `.dng` gate; previously every input was offered to the reader (which returned None) |
| `tessera-ffi/src/lrcat.rs` import mask extent | import | errors and None both fall through to the previous LibRaw probe |
| `tessera-ffi/src/catalog.rs` metadata | index | unchanged from `6682ca82`: LibRaw first |
| `tessera-ffi/src/lrcat_profile.rs` | opt-in audit only | reports unclaimed headers as their own class (none found) |

The synthetic `linear-gradient-jxl.dng` (PhotometricInterpretation 34892,
Compression 52546, three channels) is still claimed; all LR-13 tests that use it
pass unchanged.

Ordinary originals, tests that pin "exactly as before" (all in the final gate):

- `pipeline-cpu/tests/golden.rs::raw_fixture_goldens`: pixel goldens for
  `fixtures/raw` CR3, ARW, NEF, RAF and the CFA `sample.dng`, files untouched.
- `image-core/tests/linear_dng.rs::cfa_dng_stays_raw_and_corrupt_linear_dng_does_not_fall_back`:
  `sample.dng` through `RawImage::open` stays a LibRaw CFA source.
- `raw-decode/tests/lr8e_safety.rs::ordinary_cfa_with_exotic_ifd_still_decodes_through_libraw`.
- `raw-decode/tests/baseline.rs::ordinary_dng_retains_default_baseline_exposure`.
- `tessera-ffi/tests/develop.rs` (Develop sessions on `sample.dng` and
  `nikon-nef.NEF`) and `pipeline-gpu/tests/resident.rs` (NEF resident parity).

## Machine A approval conditions (A-LR13-REVIEW, top section)

| # | Condition | Answer |
| --- | --- | --- |
| 1 | Ship the minimal notice; fix the lens blur / retouch texts | `cca99b5b` stays in the tip (Rust `ignored_settings` + status line). Texts are now "Lens Blur is not rendered on Smart Preview yet." and "Retouch is not rendered on Smart Preview yet." RED `7df6f558` (`proxy_notice_tests::lr13_lens_blur_and_retouch_notices_do_not_blame_the_original`), fix `348b74f7`, FFI coverage `e07fe9bc`; Swift `testProxyRenderNoticeOwnsOnlyItsPhotoAndPreservesNewerStatus` covers the status line |
| 2 | HDR note only when HDR output is on | RED `7df6f558` (`lr13_hdr_note_only_when_hdr_output_is_on`), fix `348b74f7`. Proxies with an HDR note: 16,658 before, **366** after |
| 3 | LR-8f fixtures come along; re-run audit and sample on the merged decoder | Fixtures verified byte-identical to `38817f56`; README kept both texts plus a provenance note for the JPEG XL fixture (`5d5be036`). Numbers in the next section |

## Re-measurement on the merged decoder (LR-8e..8h + conditions 1 and 2)

Run at `5d5be036` (later commits change one integration-test file and docs).

Admission audit, 19,727 proxies, real headers read by the hotfix reader:

| App route | Admitted | Rejected |
| --- | ---: | ---: |
| Thumbnail / loupe preview | 19,727 | 0 |
| Develop | 19,727 | 0 |
| Export from proxy | 19,727 | 0 |

No proxy header was left unclaimed or rejected by the hotfix reader (the audit
counts those classes separately; both are absent). Originals resolving: 21,643.

Settings degraded with an info note (counts overlap):

| Field | Proxies |
| --- | ---: |
| `camera_profile/look` | 15,543 |
| `lens/profile` | 11,978 |
| `locals/adjustments` | 625 |
| `effects/lens_blur` | 417 |
| `output/hdr` (HDR output on) | 366 |
| `locals/retouch` | 285 |
| `white_balance/mode` | 52 |

16,658 proxies carry non-default presentation headroom; it is reset for the SDR
proxy render without a note.

Full renders through the real FFI calls, fresh scratch import of all 19,727
proxies, same deterministic 200 sample + 12 reference pairs (211 photos), four
workers, 2,193 s including the import:

| Route | Rendered | Failed |
| --- | ---: | ---: |
| Develop session frame to an IOSurface + nonempty histogram | 211 | 0 |
| Grid thumbnail (256) | 200 | 0 |
| Loupe (2048) | 200 | 0 |
| Analysis (quality) | 200 | 0 |
| PNG export from proxy | 200 | 0 |

Decoder parity against the pre-hotfix run of the same sample: all 211 Develop
frames are byte-identical PNGs, and all 200 exports have identical decoded pixel
data (the files differ only in the embedded ICC chunk). 165 of the 200 export
warning files lost exactly their HDR line (condition 2); no other warning line
changed. Outputs are only in scratch `lr13-out/contact2`; the scratch app
directory was removed by the harness.

## Deferred to LR-13b (not started, per Machine A)

Everything under "Next lane LR-13b" in A-LR13-REVIEW. One item was begun before
that ruling arrived and withdrawn: a persistent loupe notice with
`loupe.smart-preview-notice`. Its RED test was committed as `1f64ff6f` and
reverted by `33e56d7d`; the tip contains neither. The grid/loupe badge is
unchanged. Known limits of the shipped minimum, all LR-13b items: the notice is
the shared one-line status message (a newer status replaces it, long lists
truncate with the full text in the tooltip, no dedicated accessibility
identifier), and it is refreshed per frame through an FFI call.

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

Gates at `c5132de6` (all LR-13 code before the hotfix; release and dev
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

Final gates on the combined tip (hotfix + conditions; raw-decode, image-core,
tessera-ffi and the other touched crates cleaned first):

Code tip `e07fe9bc`; only this HANDOFF commit follows. Release and dev
artifacts of raw-decode, pipeline-cpu, pipeline-adobe, pipeline-gpu, image-core,
export, cull and tessera-ffi cleaned first.

| Gate | Result |
| --- | --- |
| `cargo test --release`, same twelve crates | PASS: 2,026 passed, 0 failed, 72 ignored, one attempt, no exclusions |
| Workspace clippy, all targets, `-D warnings` | PASS |
| `cargo fmt --all --check` | PASS |
| `apps/mac/build-ffi.sh` | PASS; tracked bindings unchanged afterwards |
| `tools/orchestrate/swift-gate.sh` | SWIFT GATE OK: 921 tests, 3 skipped, 0 failures |
| Strict release `Tessera` build | PASS |

Confirmed in that run: `raw_fixture_goldens`,
`cfa_dng_stays_raw_and_corrupt_linear_dng_does_not_fall_back`,
`ordinary_cfa_with_exotic_ifd_still_decodes_through_libraw`,
`ordinary_dng_retains_default_baseline_exposure`, `lr8e_safety` (14) and
`lr8f_safety` (14), `tessera-ffi/tests/develop.rs` (9), `pipeline-gpu/tests/resident.rs` (6),
and every `lr12_*` / `lr13_*` test.

Cargo.lock, dependency manifests, board.json and existing goldens are unchanged.
No private pixel, path, file name or catalog-derived string is committed.
