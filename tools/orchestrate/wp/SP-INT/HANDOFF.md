# SP-INT — Smart Preview integration branch

Status: assembled, A-LR8 M8/M4/M10 done, then the SP-INT2 review fixes;
rebased onto `origin/main` `fc3e757d` (batch 54, ENG-6). Final gates are in
the SP-INT2 section at the end. Ready for the
coordinator to merge. Binding reviews: `A-LR8-REVIEW.md`, `A-LR13-REVIEW.md`.
Synthetic fixtures only (plus the repo's `fixtures/raw` in skip-if-absent
tests). No board.json change. Cargo.lock differs from main only by the two
approved raw-decode edges (`jxl-oxide`, `zune-jpeg`) from LR-8.

## Steps

| Step | Result |
| --- | --- |
| 1. Replay LR-13b (`2ee8bfc6..861f3652`, 30 commits) on `wp/LR-8d` `01dbaa0d` | Done, commits kept separate, messages untouched. 4 conflicted commits, resolved below |
| 2. LR-13d + LR-13e (`199933c8..6d6fdca2`, 7 commits) | Done, no conflicts |
| 2b. LR-13f (`6d6fdca2..origin/wp/INT-1-rut-build`, 3 commits, coordinator follow-up) | Done, no conflicts |
| 3. Gate on the assembled branch | `cargo test --release --workspace --no-fail-fast`: 3426 passed, **3 failed**, 107 ignored. The three were a reconciliation bug, fixed in `fix(SP-INT)` (below). LR-13b's known Rust reds (ml-embed/ml-quality grouping) were green after step 2. The Swift gate (where LR-13b's IncrementalLibraryTests red lived) was run only at the final tip: SWIFT GATE OK. The remaining gates were run once, at the final tip |
| 4. A-LR8 M8, M4, M10 | Done, tests first (table below) |
| 5. Rebase onto `origin/main` | Done, no conflicts. 21 commits carried by the old LR-8R base were already on main and dropped (19 LR-11/LR-11b commits, patch-identical, and the 2 batch 51/52 merge commits) |
| 6. Final gates | Below |

## Conflicts and resolutions (step 1)

| Commit | File | Resolution |
| --- | --- | --- |
| `86a68a15` fix(LR-13b): render proxy retouch and depth | `pipeline-adobe/src/render.rs` | Kept LR-8d's `validate_baseline_exposure` on the camera metadata (BaselineExposure split) and LR-13b's `render_linear_scaled_with_lens(.., context)` for retouch/depth resources |
| same | `tessera-ffi/src/develop.rs` `render_notices` | Kept LR-8e3's structure (proxy notices plus `profile_notice` for the Native "Adobe Standard/Color only" approximation) and LR-13b's resource-aware `proxy_render_plan` |
| same | `tessera-ffi/src/preview.rs` `render_imported` | Kept LR-8e3's `with_host_ignored_native_profiles()` and LR-13b's retouch renderer and depth provider |
| `b8af4a90` fix(LR-13b): embedded profiles only for Adobe exports | `tessera-ffi/src/export.rs` `Source::open` | LR-13b scope (read the embedded profile only for the Adobe family) combined with LR-8d's rule that a malformed profile never fails a render (`.ok().flatten()` instead of `map_err(failure)?`) |
| `1cddb262` fix(LR-13b): catalog names … | `tessera-ffi/src/develop.rs` `render_notices` | LR-8e3 structure with LR-13b's `Shared::proxy_plan_fields` (session depth provider) |

Reconciliation bug found by the step-3 gate, `fix(SP-INT)`: LR-8e3 planned
proxy settings at `prepare_dcp`, `render_tiles` and `render_progressive` with
the resource-blind `CameraLinearProxy::render_plan`, which dropped Lens Blur
and retouch before LR-13b's resource-aware route saw them. Those entry points
now use `Renderer::proxy_render_plan` (same Auto→As Shot WB plan). Tests:
`tessera-ffi/tests/lr13b_proxy_effects.rs` (RED 1/3 → GREEN 3/3) and
`preview::tests::lr13b_thumbnail_uses_proxy_effect_resources` (RED → GREEN).

## A-LR8 M8 / M4 / M10

Frame decision (M8): one convention for everything, the one ordinary RAW
imports already use. Crop, masks, Upright and lens corrections are
normalized in the sensor (active-area) frame; the orientation is a display
orientation applied by the consumer (Mac loupe/crop tool via
`DevelopInfo.orientation`, which already stores crop in sensor orientation;
the preview store; export/print `apply_orientation`; AI segmentation input;
merge inputs). The absolute catalog orientation replaces EXIF as
`RawMetadata::orientation` and is never composed with it. RGB sources keep
consuming orientation in their decoder, as ordinary RGB imports do. This
reverses LR-8b's "oriented edit frame" for proxies and relinked originals.

| Finding | Code | Tests |
| --- | --- | --- |
| M8 one frame for proxies, relinked and ordinary RAWs (incl. lens order/resolution) | `pipeline-cpu` render: no early `orient_image`; `with_catalog_orientation` sets `orientation`; Smart Preview codec restores old entries with the catalog orientation. `image-core` `open_with_catalog_orientation`, unswapped `active_extent`, only proxies reach the proxy resident tail (a relinked original with catalog orientation 1 panicked on an unwrap there). `export` ai_masks, `tessera-ffi` lrcat import extents, export `Source::open`, `EmbeddedMetadata` displayed orientation | New: `image-core/tests/lr8m_frame.rs` (relinked vs ordinary for all 8 orientations; proxy catalog orientation display-only with manual distortion/vignetting resolved in the sensor frame before crop/Upright), `export/tests/lr8m_orientation.rs` (proxy export = sensor render oriented once). RED 0/2, 0/1 (the first export RED failed on recipe history; corrected in `3ca0ea94`, then RED for the frame: orientation 2 off by 0.30). GREEN. Changed expectations (all in tests added by the Smart Preview chain, none on main): pipeline-cpu `catalog_orientation` test 1, image-core `lrcat_linear` mask width, tessera-ffi `lrcat_orientation_tests`, `combined_tests` (int1, lr8r raster extent 12x10), export `smart_preview_admission` (expected rotated once), tessera-ffi `tests/lrcat.rs` (thumbnail/export are displayed dims). Each listed with before/after in the commit messages |
| M4 relinked originals use the normal RAW path (GPU, lens profiles) | `image-core` render dispatch (`render_tiles`, `render_progressive`, `output_extent`) sends only Smart Previews to the scalar camera-linear route; `supports_resident` and export `gpu.rs` no longer refuse a catalog orientation | `lr8m_frame::lr8m_relinked_original_takes_the_ordinary_raw_route_at_every_level` (levels 0-2 pixels/extents and resident capability equal an ordinary import), `pipeline-gpu/tests/lr8m_relinked.rs` (Metal resident-capable, GPU frames equal), `export/tests/lr8m_relinked_export.rs` (GPU export used, pixels equal). RED 2/3, 0/1, 0/1; GREEN. The GPU test first called `render_resident_region`, which declines the ordinary synthetic import as well; corrected in `57c1d19c` to compare the Develop GPU route |
| M10 proxy edits land the same on the full-res original | (no code change needed after M8/M4) | `image-core/tests/lr8m_relink_parity.rs`: synthetic 5120x3412 Bayer DNG → external LinearRaw 2560 px proxy from its own camera-linear pixels; same recipe (rotated crop, Upright, vignetting, radial + linear masks), catalog orientation 6 on both; 48x32 normalized cell means: mean 0.03 / max 0.40 levels (bound 1.5 / 8). Real `fixtures/raw/sample.dng` variant (skipped if absent): mean 0.38 / max 5.37. Sensitivity control: mask moved 4% must exceed 24 levels. GREEN on arrival (first run after M8) |

Not changed (follow-ups for a ruling or a later lane):
- Rotated external LinearRaw proxies are still declined by the GPU proxy tail
  (`CameraLinearProxy::resident_tail_plan`, `try_camera_linear_resident`).
  After M8 orientation no longer affects their pixels, so admitting them is
  now possible; `export/tests/lrcat_jxl.rs` still pins the decline.
- Existing indexes keep the old displayed orientation (1) for catalog-oriented
  files until the file is re-indexed; only merge inputs read it (rendering,
  thumbnails, Develop and export read the source).
- Imported Lightroom crop/mask coordinates are taken as sensor-frame values,
  as ordinary imports always did; no Adobe parity is claimed for rotated
  photos (unchanged README position).
- The pipeline-cpu test `cfa_and_generated_proxy_share_the_oriented_edit_frame`
  keeps its old name; its assertions (CFA = proxy) still hold.

## Golden and fingerprint audit versus origin/main `a61703d4`

| Item | Differs? | Why |
| --- | --- | --- |
| `fixtures/golden/*.png` (native pixel goldens) | No | — |
| `pipeline-cpu/tests/golden.rs` | No (byte-identical) | LR-8d removed the zeroing line it had added |
| `pipeline-cpu/examples/regenerate_goldens.rs` | Yes | LR-8d: renders with the settings `tests/golden.rs` uses (lens profile None, CA off) so regeneration reproduces the stored goldens |
| `import-lrcat/tests/golden.rs` digests | No | One added test re-asserts the same `GOLDEN` with a Smart Preview bundle present (LR-8) |
| `import-lrcat/tests/data/*` | No | — |
| `CameraLinearProxy::GENERATOR_REVISION` | 2 → 3 | LR-8d: generated proxies no longer bake BaselineExposure; cached generated proxies regenerate |
| `IMPORTED_RENDER_PLAN_VERSION` (tessera-ffi preview) | New (2) | LR-13b thumbnail identity for imported proxies |
| Existing test expectations on main | None changed | `cull/tests/grouping.rs` (finish deferred previews first, LR-13c) and `tessera-ffi/tests/lrcat_streaming_parity.rs` (library folder not the catalog's, LR-13b) change setup only |

## Commit map

Regenerated after the SP-INT2 rebase onto batch 54 (`fc3e757d`); matched by
subject (every subject on the branch is unique). Hashes quoted in the SP-INT
sections above refer to the d059e671 branch; this table maps them.

### LR-8R + LR-8d/8e2/8e3 (old main..origin/wp/LR-8d)

| Source | New | Subject |
| --- | --- | --- |
| `2574bd4c` | already on main (dropped) | test(LR-11): expose missing per-mask translation and CPU operators |
| `362faf84` | already on main (dropped) | test(LR-11): cover synthetic codecs pixels and exact CPU fallback |
| `25065032` | already on main (dropped) | feat(LR-11): translate and render local mask adjustments with CPU fallback |
| `5173e627` | already on main (dropped) | docs(LR-11): record local adjustment coverage and final catalog counts |
| `1aef59f0` | already on main (dropped) | test(LR-11b): keep AI instance selections unsupported and name the real blocker |
| `ebbec569` | already on main (dropped) | fix(LR-11b): reject AI instance selections and name the decoder's real blocker |
| `80cb6a57` | already on main (dropped) | test(LR-11b): extended local curves must not override SDR curves |
| `c4fe6c7e` | already on main (dropped) | fix(LR-11b): translate extended local curves only for HDR output |
| `a9c132b6` | already on main (dropped) | test(LR-11b): local Point Color must be one stage regardless of B&W |
| `cd9d8eaf` | already on main (dropped) | fix(LR-11b): run local Point Color at one stage on every render path |
| `7f971634` | already on main (dropped) | test(LR-11b): match mask groups to their source by stable id |
| `3699a9f3` | already on main (dropped) | fix(LR-11b): pair mask groups with source groups by stable group id |
| `548fc004` | already on main (dropped) | test(LR-11b): local defringe accepts Adobe's signed -100..100 range |
| `cc79ebc3` | already on main (dropped) | fix(LR-11b): accept Adobe's signed local defringe range |
| `4c8eece4` | already on main (dropped) | docs(LR-11b): describe the HDR curve rule, the single Point Color stage and unsupported instances |
| `a6cfe1ca` | already on main (dropped) | docs(LR-11b): record restack range-diff, findings, measurement and gates |
| `abbefcce` | already on main (dropped) | test(LR-11b): name the real blocker on the LR-5b stack |
| `911cd9cc` | already on main (dropped) | docs(LR-11b): matrix wording for the LR-5b stack |
| `6a578467` | already on main (dropped) | docs(LR-11b): record the rebase onto LR-5b final, counts and gates |
| `16cf23db` | already on main (dropped) | Merge batch 51 (LR-5/5b/5c Lightroom AI masks: regenerated rasters, oriented extents, export errors when unavailable) onto main |
| `392c2156` | already on main (dropped) | Merge batch 52 (LR-11/11b Lightroom local mask adjustments) onto main |
| `666b9d48` | `f673a910` | test(LR-8): expose lossy smart-preview LibRaw decode blocker |
| `a1c83430` | `10103e14` | docs(LR-8): hand off missing LibRaw lossy DNG capability |
| `3433246b` | `64b5949f` | test(LR-8): cover synthetic lossy linear DNG camera samples |
| `e374d096` | `b3528c2e` | test(LR-8): distinguish JPEG XL and verify read-only UUID lookup |
| `1dcbe4fe` | `b8265000` | feat(LR-8): add bounded classic JPEG LinearRaw decoder and UUID lookup |
| `4795c7fd` | `e7045c34` | docs(LR-8): correct JPEG XL blocker and record decoder groundwork |
| `ec5ee6d7` | `be9753de` | test(LR-8): cover 16-bit JPEG XL camera channels in synthetic DNG |
| `6399c216` | `19c4c352` | test(LR-8): require camera DNG admission to Native and Adobe Develop |
| `edf066f9` | `efaf704d` | test(LR-8): require offline proxy import, protected edits, copy and automatic relink |
| `0d698e23` | `e809b449` | test(LR-8): require DNG polynomial sample mapping before crop |
| `5dd4d6e1` | `9b90d889` | test(LR-8): refuse unconsumed camera DNG correction lists |
| `695a801e` | `96e350a9` | feat(LR-8): develop and relink editable Lightroom smart previews |
| `164da9d7` | `7aebceb3` | docs(LR-8): hand off validated editable smart preview import and relink |
| `f6a3f94c` | `96eed3e4` | test(LR-8b): require all catalog orientations before normalized edits |
| `0c12d9cd` | `38882037` | test(LR-8b): cover proxy relink, source-safe profiling and numeric tone diagnostics |
| `ccda096c` | `947b9ec8` | test(LR-8b): require shared BaselineExposure handling for originals and proxies |
| `d54ccf21` | `ecaeaff4` | test(LR-8b): require oriented CFA parity and imported mask hooks |
| `9c69d92d` | `53aafebf` | test(LR-8b): require header-only smart-preview metadata indexing |
| `3f3e4c8a` | `a74c13dd` | fix(LR-8b): align catalog orientation, RAW exposure and imported proxy masks |
| `f0ab1459` | `2130eba6` | test(LR-8b): make metadata-only decode regression reject the full JPEG payload |
| `027f4e3b` | `ae067975` | fix(LR-8b): sample comparison pairs from proxies with standard previews |
| `0d547d66` | `118b4d2a` | test(LR-8b): cover split standard-preview JPEG levels |
| `4a5a323e` | `23712a84` | fix(LR-8b): read current split-JPEG standard-preview caches |
| `6b0fa428` | `1ac7b173` | test(LR-8b): cover REAL image IDs in the standard-preview index |
| `4af8fd44` | `279cbcec` | fix(LR-8b): accept integral REAL IDs in Lightroom preview databases |
| `985ea86a` | `d062ea89` | test(LR-8b): cover DNG AsShotWhiteXY as the neutral alternative |
| `48519488` | `899836cf` | fix(LR-8b): decode AsShotWhiteXY camera-neutral metadata |
| `27664f0b` | `9bfca185` | test(LR-8b): require white-xy calibration inheritance from IFD0 |
| `73dbfeeb` | `76bb226e` | fix(LR-8b): inherit white-xy and calibration tags from the DNG root |
| `a0546dbf` | `29e71978` | test(LR-8b): retain unresolved lens references without blocking proxy previews |
| `c4c60e92` | `a3c84a0c` | fix(LR-8b): report unresolved lens profiles without blocking Develop |
| `fba5eaf0` | `75b92339` | test(LR-8b): require aspect-preserving comparison thumbnails |
| `1294093a` | `089cb4dc` | fix(LR-8b): preserve comparison image aspect at a 1024-pixel long edge |
| `8b6d33df` | `0449f1fe` | docs(LR-8b): record orientation, exposure, real-catalog evidence and gates |
| `75d048ab` | `69a9115b` | docs(LR-10): record integration conflicts and import golden boundary |
| `76be668d` | `6c61c1a1` | test(LR-8c): preserve ordinary import bytes with neighboring previews |
| `1074cdfc` | `1f4f00da` | fix(LR-8c): retain catalog orientation only for offline proxy recipes |
| `f38f86f0` | `7cb96368` | test(LR-8c): preserve original mask and depth extents with catalog rotation |
| `8cfea557` | `90e9a80c` | fix(LR-8c): keep original resource extents on the main import path |
| `40b5a981` | `8e2cdbc0` | test(LR-10): pin default curve and post-exposure look order |
| `3686e18c` | `0561d9e1` | feat(LR-10): use public ACR3 defaults and defer profile looks after exposure |
| `3f1705c9` | `3de1fc57` | test(LR-10): require baseline exposure offsets and explicit black-render policy |
| `a2d6de93` | `7e54e45a` | feat(LR-10): apply profile exposure and black policy between hue and look tables |
| `e9bd8d29` | `c2fef482` | test(LR-10): require embedded DNG profile fallback and Adobe-name dispatch |
| `a8d456fc` | `266dfe84` | test(LR-10): retain Adobe profile identity through Develop admission |
| `35047da9` | `10b334c7` | test(LR-10): require signed RGB means in private pair measurements |
| `d3170174` | `6d9fe997` | feat(LR-10): resolve embedded DNG profiles and camera-neutral white balance |
| `d9b9ce7e` | `9d48284c` | feat(LR-10): measure luminance and signed RGB deltas from fixed private pairs |
| `97601a57` | `64e056fd` | test(LR-10): preserve output-referred DNG tone and unknown-illuminant admission |
| `46265e9d` | `a7878eeb` | fix(LR-10): honor output-referred DNG defaults and unknown illuminants |
| `14204a9b` | `790b5225` | docs(LR-10): record rendering contract measurements and complete gate results |
| `bc94887e` | `90564b95` | test(LR-8e): expose LinearRaw admission and allocation regressions |
| `ee3a108f` | `1859a558` | fix(LR-8e): gate LinearRaw admission and bound tile decoding |
| `b79af9c4` | `7f04070b` | docs(LR-8e): record hotfix scope and verified gates |
| `af031e7d` | `db3be43a` | test(LR-8f): expose admission, read amplification and JPEG color regressions |
| `5fed2c78` | `ea5e0635` | fix(LR-8f): restrict admission and bound compressed decoding work |
| `0a4a47a8` | `3f071940` | docs(LR-8f): record review resolutions, private parity and release gates |
| `9bc03578` | `25dc5c1c` | test(LR-8g): cover large originals and ambiguous tile encodings |
| `7882fe16` | `11d4398a` | fix(LR-8g): admit large originals and reject ambiguous tile encodings |
| `59bf1d54` | `fa3c27dc` | docs(LR-8g): record cap rationale, parity, mutation and gate evidence |
| `e79d4607` | `333c795f` | test(LR-8h): expose the fixed 128 MiB compressed cap on large originals |
| `b150489e` | `998a3ebb` | fix(LR-8h): bound compressed tiles by file size and the decoded budget |
| `57ef9ae2` | `dbfb3eec` | docs(LR-8h): record compressed-budget ruling, tests and gates |
| `7dea75cf` | `2e76bfc8` | test(INT-1): cover nested local edits on an oriented offline Adobe proxy |
| `ba6eba08` | `b0694177` | fix(LR-12): count and sample split Lightroom previews in import sheet |
| `9e8b31ef` | `217076eb` | test(LR-12): reproduce optional LinearRaw failures and audit recipe fields |
| `f9022b96` | `4ce79454` | docs(LR-12): record partial fix, red regressions, and private baseline blocker |
| `299eaa18` | `8b690393` | test(LR-13): reproduce imported JXL proxy analysis decode failure |
| `4f4ac577` | `97c2a54b` | fix(LR-13): share imported proxy preview routing with analysis and assist |
| `1e0faff3` | `44cc2d25` | test(LR-13): cover culling, Adobe preview parity and proxy export notices |
| `b712fd3c` | `88384f93` | fix(LR-13): render LinearRaw proxies without unavailable optional settings |
| `8494a226` | `d85a7a89` | fix(LR-13): export from an external proxy with a quality warning |
| `4dd6a6fd` | `5da1118d` | fix(LR-13): route imported proxy thumbnails, loupe, analysis and culling through the Smart Preview decoder |
| `35df7477` | `1e62f9b6` | test(LR-13): reproduce full-resolution render for imported proxy thumbnails |
| `beb48639` | `027df82c` | fix(LR-13): render imported proxy thumbnails at thumbnail size |
| `ebfd9023` | `b26eff23` | feat(LR-13): admit identity-oriented external LinearRaw to the resident GPU tail |
| `a2d6994c` | `27d42cdc` | fix(LR-13): read JPEG XL LinearRaw proxies as merge inputs |
| `b97c458d` | `95817de2` | fix(LR-13): size imported mask rasters from the LinearRaw header |
| `36f4250a` | `51c8afea` | feat(LR-13): surface per-photo proxy render notices in Develop |
| `f8cf128a` | `18201813` | test(LR-13): add opt-in aggregate proxy admission audit and app-path sampler |
| `0f5ae2b0` | `1c926803` | docs(LR-13): review map, entry-point findings, measurements and gate results |
| `15ab199d` | `16230f2e` | fix(LR-13): offer only .dng merge inputs to the LinearRaw reader |
| `97da44da` | `d72af6ed` | refactor(LR-13): extract the proxy notice sentences into one function |
| `7f083f03` | `85b5e844` | test(LR-13): reproduce wrong proxy notice wording and the no-op HDR note |
| `7a6ecc63` | `f2d21f7d` | fix(LR-13): correct lens blur/retouch notice wording and drop the no-op HDR note |
| `71bcac78` | `a77d406d` | docs(LR-13): state the JPEG XL fixture's provenance after the LR-8f fixture regeneration |
| `9551060f` | `031c160b` | test(LR-13): cover the minimum proxy notice through the FFI Develop session |
| `726c334a` | `5cbb97b3` | docs(LR-13): record hotfix LR-8e..8h integration, approval conditions and re-measurement |
| `814517fb` | `30055fea` | test(LR-8R): require oriented proxy mask extents on the LR-5b base |
| `c77cb368` | `d967b2b8` | fix(LR-8R): validate imported proxy masks in the oriented active frame |
| `01a0cc8a` | `5766668d` | test(LR-8R): adapt LR-11b synthetic RAW metadata to proxy fields |
| `3aeb1d33` | `0612d4f9` | test(LR-8R): require proxy notices to recognize regenerated masks |
| `bafe4684` | `afa167cb` | fix(LR-8R): align proxy mask notices with LR-5b readiness |
| `1e31cd27` | `7d496029` | docs(LR-8R): record port map, integration fixes and Rust gate attempts |
| `63be13a6` | `7f532002` | test(LR-8R): carry proxy options into the B5-50 accessibility fixture |
| `94c55e95` | `dc410544` | docs(LR-8R): record final gates and deferred accessibility conflict |
| `2d01aab4` | `724649d8` | fix(LR-8R): give the Smart Preview import controls namespaced identifiers |
| `2ee8bfc6` | `a3a1f993` | docs(LR-8R): record LR-8R2 compatibility ruling and passing gates |
| `c2ad67b7` | `6ab3f169` | test(LR-8d): expose Native BaselineExposure regression without golden overrides |
| `0ac66088` | `78ca9f15` | fix(LR-8d): keep Native baseline neutral and apply Adobe exposure once |
| `c936e53c` | `8f094610` | test(LR-8e2): restore DCP contracts and expose fallback scope and headroom failures |
| `0f324c94` | `cedbeef8` | test(LR-8e2): require default unknown second illuminant without masking malformed matrices |
| `1417184b` | `39c131b9` | test(LR-8d): retain Adobe baseline gain validation after removing Native gain |
| `88bd0b0e` | `166dc969` | fix(LR-8d): validate finite positive baseline gain at Adobe boundaries |
| `9369c0c8` | `58a2d95a` | test(LR-8e2): require visible substitution notes and installed-profile precedence |
| `1adee24e` | `b08812c1` | fix(LR-8e2): scope embedded DCP fallback to Adobe-named LinearRaw proxies |
| `60cb8f8d` | `11072c87` | docs(LR-8e2): add Adobe DNG SDK licence attribution for the ACR3 table |
| `7f5b9637` | `18aed9eb` | docs(LR-8d): record hunk dispositions, golden audit and final gates |
| `d7c6ec49` | `f20810d5` | test(LR-8e3): pin Native profile refusal, approximation note, Auto WB entry points and SDK illuminants |
| `8d257d90` | `c1d6154a` | fix(LR-8e3): refuse unreproducible Native profiles and plan proxy WB on every entry point |
| `5732bdcc` | `13ffca54` | docs(LR-8e3): document Native profile scope, rounding, PV2010 order and LookTable clamp |
| `01dbaa0d` | `96f76a1c` | docs(LR-8e3): record review fixes, fixture investigation and gates |

### LR-13b own commits (2ee8bfc6..861f3652, including the LR-13c port)

| Source | New | Subject |
| --- | --- | --- |
| `bb18640a` | `84d55c53` | test(LR-13c): forbid pixel work while opening synthetic proxy libraries |
| `a34193c9` | `a316d5be` | test(LR-13c): cover lazy hashes, cancellation, reuse and cheap proxy pixels |
| `ecd966f4` | `e91d3141` | fix(LR-13c): defer cached cull hashes and reuse library listings |
| `da238001` | `6e19b120` | docs(LR-13c): record lazy-open evidence and full verification gates |
| `8082d40e` | `3667e039` | test(LR-13b): reject dropped proxy AI masks in print and export |
| `7b7fef8b` | `9aeec33a` | test(LR-13b): use the depth cache contract for range-mask fixtures |
| `26bc1417` | `bb7a1df2` | fix(LR-13b): preserve proxy AI and depth masks in exports |
| `599cc3bb` | `bda41682` | test(LR-13b): require proxy depth and real retouch rendering |
| `22782ce4` | `8c2f68dd` | test(LR-13b): require thumbnail parity for available proxy effects |
| `86a68a15` | `fd588e68` | fix(LR-13b): render proxy retouch and depth with real resources |
| `3e9aea33` | `8d2afaf3` | test(LR-13b): surface proxy raster and thumbnail mask failures |
| `b411b0a2` | `6737af1b` | fix(LR-13b): surface proxy mask raster failures |
| `0875ac46` | `7fe2d388` | test(LR-13b): cover Native profile errors and cached depth at the bridge |
| `4e3755a6` | `ce011d0a` | test(LR-13b): use a pixel-valid TIFF field the profile reader rejects |
| `b8af4a90` | `3d918b1b` | fix(LR-13b): read embedded profiles only for Adobe-process proxy exports |
| `cae105a7` | `38f7da8c` | test(LR-13b): require a persistent loupe notice without per-frame FFI reads |
| `8570f913` | `2d6829a6` | test(LR-13b): key imported thumbnails by mask availability and plan version |
| `61f46ee6` | `655a8cf3` | fix(LR-13b): include plan version and mask availability in thumbnail identity |
| `bfee012e` | `10a372a1` | test(LR-13b): justify ab924590's resident-tail admission flip |
| `88e61485` | `673d767c` | fix(LR-13b): persistent loupe notice, cached render notices |
| `ca1cd974` | `fff37a7e` | style(LR-13b): rustfmt preview tests |
| `3e169ea0` | `980fa553` | docs(LR-13b): record STEP 1 and STEP 2 items and attempts |
| `a06eb0da` | `40636dec` | test(LR-13b): catalog names, image-keyed copies, listing facts, safe defaults |
| `5eb6962f` | `12db2483` | test(LR-13b): LR-8h decoder peak is output plus one tile |
| `bdfc02fc` | `0cb28a63` | fix(LR-13b): stream LinearRaw tiles and crop the output in place |
| `1cddb262` | `49465108` | fix(LR-13b): catalog names, image-keyed copies, scan-time listing facts |
| `03887afa` | `ab744404` | feat(LR-13b): show imported Smart Previews under their catalog file name |
| `09b0ff6e` | `953b43ed` | style(LR-13b): keep the proxy-owner doc comment on its function |
| `b398b73d` | `67d37b5c` | fix(LR-13b): use the Theme secondary ink in the loupe notice |
| `861f3652` | `649b0d6f` | docs(LR-13b): record STEP 3 items, attempts and final gate results |

### LR-13d, LR-13e, LR-13f (199933c8..origin/wp/INT-1-rut-build)

| Source | New | Subject |
| --- | --- | --- |
| `e4c1bd4e` | `b7365787` | test(LR-13d): reproduce deferred grouping, cache policy, and queue regressions |
| `749b8712` | `f27f073a` | fix(LR-13d): join hash workers, bound default regroup, approve cache roots |
| `a350fa8a` | `ec2ce1bf` | docs(LR-13d): record review fixes, finding map and gate results |
| `429ee7c7` | `563544d2` | test(LR-13e): reproduce whole-library default regroup, joint joins and quit hangs |
| `8fabb780` | `a81937d2` | fix(LR-13e): incremental default regroup, independent joins, bounded quit |
| `dca744e7` | `f13f8941` | test(LR-13e): finish deferred hashing before ml grouping assertions |
| `6d6fdca2` | `17204601` | docs(LR-13e): record re-review fixes, finding map and gate results |
| `32aff94a` | `8811e0d7` | test(LR-13f): reproduce poll blocking on piled-up wake tickets |
| `8940e108` | `8c63b9cc` | fix(LR-13f): only the Wake ticket clears wake_pending |
| `95399931` | `345f85ec` | docs(LR-13f): record wake-ticket fix, coverage nits and gate results |

### New in SP-INT and SP-INT2

| Before the SP-INT rebase | On d059e671 (or SP-INT2 before its rebase) | Final | Subject |
| --- | --- | --- | --- |
| `a96cc529` | `c4229eea` | `e9799fc7` | fix(SP-INT): plan proxies with their render resources at every entry point |
| `62a82d06` | `4fa8e6f0` | `7e683e22` | test(LR-8m): one sensor edit frame for proxies, relinked and ordinary RAWs |
| `5b6836ee` | `3ca0ea94` | `eca00a5e` | test(LR-8m): edit export recipes through history; align remaining frame expectations |
| `0dff9a2c` | `e9fbaf20` | `0cdb8295` | style(LR-8m): rustfmt frame tests |
| `7c4d2599` | `a99c43f8` | `fead2a7b` | fix(LR-8m): catalog orientation is a display orientation; edits stay in the sensor frame |
| `35bb7f4e` | `630c2154` | `ca47e6f2` | test(LR-8m): relinked originals take the ordinary RAW, GPU and export routes |
| `953c4a4f` | `57c1d19c` | `72d6941e` | test(LR-8m): compare relinked GPU frames through Develop's render route |
| `3268fcc0` | `1f25208e` | `50978ce5` | fix(LR-8m): relinked originals use the ordinary RAW, GPU and export routes |
| `14ff3a96` | `4dad527f` | `35550f4f` | test(LR-8m): Smart Preview edits land the same on the relinked original |
| — | `cd58c8c4` | `649b9826` | style(LR-8m): drop outer dead_code allows that duplicate common's inner allow |
| — | `d059e671` | `cf1f67df` | docs(SP-INT): record assembly, conflict resolutions, M4/M8/M10, golden audit and gates |
| — | `59913afe` | `67e23020` | test(SP-INT2): proxy thumbnails skip a missing imported mask, fail an invalid one |
| — | `55232fa6` | `ecbb4489` | fix(SP-INT2): proxy thumbnails render with pending imported masks skipped |
| — | `32cab587` | `17f59780` | test(SP-INT2): thumbnail identity tracks Lens Blur depth and depth-model availability |
| — | `c1b5efc9` | `3349069f` | fix(SP-INT2): include Lens Blur depth and model availability in the thumbnail identity |
| — | `770651f9` | `3cd06e83` | test(SP-INT2): re-polled thumbnail requests must not re-read owner recipes |
| — | `0d8a5eb2` | `f622d67c` | fix(SP-INT2): cache owner-recipe facts for thumbnail requests per recipe hash |
| — | `c217fb2e` | `8a296e22` | test(SP-INT2): proxies share main's original gamut policy in Develop and print |
| — | `7dfe74ec` | `bacaf236` | test(SP-INT2): proxy documents keep retouch; prints report proxy omissions |
| — | `f8e03f30` | `0e2e0ead` | fix(SP-INT2): proxy documents render retouch; print returns proxy notes as sentences |
| — | `38107b9f` | `1a014ffd` | test(SP-INT2): in-place byte-identical Smart Previews stay separate photos |
| — | `c60daf41` | `0e7811f0` | fix(SP-INT2): key in-place Smart Preview recipes by catalog image |
| — | `cd479b03` | `d6ea3b79` | test(SP-INT2): cropped LinearRaw pixels release the full-size allocation |
| — | `55f9eb1c` | `9add9bae` | fix(SP-INT2): shrink cropped LinearRaw pixels when the active area is much smaller |
| — | `f295fa7d` | `33ab74e5` | test(SP-INT2): merges use the catalog orientation, not a stale index row |
| — | `d9b42165` | `d9b2d0bd` | fix(SP-INT2): merge inputs take the catalog orientation first |
| — | `4c460972` | `8c925b82` | test(SP-INT2): plant the LR-13d cache tiers at the orientation the app now writes |
| — | `e9412ee4` | `7ae04644` | test(SP-INT2): INT-1 orientation is display-only; sensor-frame test name; TMPDIR test location |
| — | `0181f05d` | `dd41f080` | test(SP-INT2): a supplied lens profile resolves in the sensor frame of a rotated proxy |
| — | `0a2c5694` | `9dfe8135` | docs(SP-INT2): disclose relinked RGB frame and rotated Transform semantics; pin LR-8n |
| — | `58f106e9` | `2b11428f` | fix(SP-INT2): private Lightroom comparison orients Develop output for display |
| — | — | `24883aa9` | test(SP-INT2): M10 real-RAW parity uses ENG-6's shared fixture helper |
| — | — | `7d67910a` | style(SP-INT2): clippy in the gamut parity test |
| — | — | `65927d65` | chore(SP-INT2): regenerate Swift bindings for PrintImage.notes |

## Final gates (SP-INT, at d059e671)

Env: `CARGO_TARGET_DIR=$HOME/.cache/tessera-target/SP-INT`, `CARGO_BUILD_JOBS=5`,
`RAYON_NUM_THREADS=5`. Before the gates: `cargo clean --release -p` for every
touched crate (cull, export, image-core, import-lrcat, index, libraw-ffi,
merge, ml-embed, ml-quality, pipeline-adobe, pipeline-cpu, pipeline-gpu,
raw-decode, sidecar, tessera-ffi, tessera-mcp).

| Gate | Result |
| --- | --- |
| `cargo test --release --workspace --no-fail-fast` | At `443a388a` (rebased tip before the clippy style commit): exit 0, **3465 passed, 0 failed, 107 ignored** (load 15-22). Includes `pipeline-cpu/tests/golden.rs::raw_fixture_goldens` with the real fixtures present (ran, not skipped) and both import-lrcat catalog goldens. `cd58c8c4` only removes duplicate attributes in three new test files; those targets were rerun: lr8m_frame 3/3, lr8m_relink_parity 2/2, pipeline-gpu lr8m_relinked 1/1 |
| Native pixel goldens, real fixtures | `cargo test --release -p pipeline-cpu --test golden`: 1 passed (`raw_fixture_goldens`, no skip message) |
| `IMAGE_CORE_ALL_FIXTURES=1 cargo test --release -p image-core fixture_level3_matches_pipeline_cpu` | **FAILED** (reported, not fixed in this lane): `canon-cr3.CR3: scene-linear max diff 0.04327532`. Same failure exists on main (separate lane) |
| `cargo clippy --release --workspace --all-targets -- -D warnings` | Attempt 1: `duplicated attribute` (outer `#[allow(dead_code)]` on `mod common` in the new LR-8m tests), fixed in `cd58c8c4`. Attempt 2: exit 0 |
| `cargo fmt --all -- --check` | exit 0 |
| `cd apps/mac && ./build-ffi.sh` | exit 0, no bindings drift (worktree clean apart from this HANDOFF) |
| `tools/orchestrate/swift-gate.sh` | **SWIFT GATE OK**: 996 tests executed, 3 skipped, 0 failures (load 8-16) |
| strict release build (`swift build -c release --product Tessera -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors`) | exit 0 (146 s). Only the known BLAKE3 neon linker warning |

Wall-clock note (earlier, affected-crate run after M8, load ~21):
`tessera-ffi/tests/develop.rs::export_batch_does_not_starve_slider_drag`
failed once ("drag stays at L2", 6 frames at L3); serialized rerun
(`--test-threads=1`, load 21 → 11) passed 10/10. It passed in the final
workspace run. No bound was changed.

## SP-INT2 — review fixes (REV-SP-A, REV-SP-B)

Reviews: `REV-SP-A.out.md` (CHANGES-REQUIRED, one blocker) and
`REV-SP-B.out.md` (APPROVE-WITH-NITS), read completely. Commits sit on top of
the SP-INT work; the branch was then rebased onto `origin/main` `fc3e757d`
(batch 54, ENG-6) with no conflicts. Tests first throughout; where a test was
written together with its change it says so.

| Item | Code | Test |
| --- | --- | --- |
| A-B1 (blocker) proxy thumbnails fail on a missing/pending imported AI raster | `MaskShared::invalid_imported`: only a failed entry or a stored raster that cannot be used (corrupt, wrong extent) fails; a missing raster keeps the Hooks' zero alpha. The render identity re-renders when the raster arrives | `preview::tests::lr13b_thumbnail_request_identity_tracks_mask_rasters_and_plan_version` now expects a rendered thumbnail without the mask, then a different one once stored; `lr13b_thumbnail_reports_invalid_and_skips_missing_imported_mask` keeps the invalid (wrong-extent) error. RED 0/2, GREEN |
| A-S1 gamut policy (coordinator ruling) | No change needed. **Finding (main, separate from proxies):** an Adobe-process original is drawn in Develop through `AdobeStageOp` → `CpuStageOp::display_linear` (hard sRGB clip, ignores `output.gamut_mapping`), while every original's export and print use the managed output transform, which honours `output.gamut_mapping` (Perceptual by default). Main therefore differs between Develop and export for saturated colours; kept per the ruling. Proxies follow exactly the same two policies. **Corrected in SP-INT3 (NS1):** the split on main is wider; see the SP-INT3 section | `export/tests/sp_int2_gamut.rs`: Develop of an Adobe proxy and of a synthetic Adobe original equal the hard-clipped scene-linear render for both gamut settings; Develop-vs-print parity for the saturated proxy: in gamut ≤ 2 levels (both settings), saturated with Clip ≤ 2 levels, saturated with Perceptual print applies the recipe's mapping. GREEN on arrival (pins main's policy) |
| A-S2 document open drops proxy retouch | `document/io.rs` renders through `render_pixels_with_resources` with the brush retouch renderer | `tessera-ffi/tests/sp_int2_proxy_outputs.rs` retouch test. RED (identical pixels), GREEN |
| A-S3 print hides proxy omissions; nits: JSON pointers in notes, profile-substitution note lost on export | `export::proxy_notes` (source note, planned-away settings as Develop's sentences via the shared `export::proxy_notice_text`, embedded-profile note on Adobe), used by file export and `render_pixels_with_notes`; `PrintImage.notes` (uniffi default empty, bindings regenerated); the Mac print controller shows them with the result message | `sp_int2_proxy_outputs.rs` print-notes test (compile RED; with notes stubbed 0/1), `export/tests/sp_int2_notes.rs` (written with the change). GREEN |
| A-S4 per-request recipe I/O for thumbnails | `Engine::preview_sources`: owner facts per path, valid while the recipe hash is unchanged; ordinary photos do no recipe I/O on re-poll, imports one stat of the catalog original (relink still switches immediately). Bounded at 200k entries | `sp_int2_repeated_thumbnail_requests_do_not_reread_recipes`. RED 8 reads vs 2, GREEN |
| A-S5 byte-identical in-place Smart Previews merge (coordinator ruling: key by catalog image id) | **Superseded in SP-INT3 (REV2-SP NB1).** `Sidecar::pin_protected_identity` + `Alias::Pinned` in the protected store; Lightroom import pins each in-place Smart Preview to (catalog, catalog image id) before reading or writing its recipe. Already-imported libraries keep their aliases until the photo is re-imported | `lr13b_library_followups::proxies_are_keyed_by_image_and_named_from_the_catalog` now runs both modes with two byte-identical offline photos (distinct ids, paths, names, recipes). RED (in place: both named lost-01.jpg), GREEN. New `sidecar/tests/storage.rs::pinned_protected_identities_keep_identical_sources_separate` (also verified in a child process; RED was a compile error) |
| A-S6 thumbnail identity ignores Lens Blur depth | `render_identity` adds, for Lens Blur, the depth slot key and its pinned revision plus the cached model weights | `sp_int2_thumbnail_identity_tracks_lens_blur_depth_and_model`. RED 0/1, GREEN |
| A-nit `truncate` keeps the full-size allocation | `lossy_dng.rs`: `shrink_to_fit` when the active area is under 3/4 of the decode | `lossy_dng.rs::sp_int2_cropped_pixels_release_the_full_size_allocation`. RED capacity 256, GREEN; LR-8h peak test still green |
| B-S1 relinked RGB originals read in the rotated frame | Not fixed (follow-up lane **LR-8n**). Disclosed here and in `crates/import-lrcat/README.md` | Ignored `image-core/tests/lr8m_frame.rs::lr8n_relinked_rgb_original_uses_the_stored_frame_like_its_smart_preview` (fails today: display orientation 1) |
| B-S2 merges read a stale index orientation | `photo_sources` prefers `catalog::catalog_orientation(path)` | `merge::proxy_source_tests::sp_int2_merge_sources_prefer_the_catalog_orientation_over_the_index`. RED 6 vs 8, GREEN |
| B-S2 remaining stale-index readers | Not changed, disclosed: `export.rs` pending rows and `document/io.rs` pass the value to `Source::open`, which ignores it; `ImageSummary.orientation` and `SessionImage.orientation` are exposed over FFI but not read by Swift; `embedded_preview` uses it only for `.jpg/.jpeg`; `assist.rs` routes catalog-oriented sources through the indexed preview first; the Inspector EXIF "Orientation" row shows the stale value (1 for INT-era imports) until re-index (cosmetic) | — |
| B-S3 LR-13d cull-hash test plants the wrong tier | Test only | Plants orientation 6 and 1; the hash must ignore both. Passes |
| B-S4 Transform/Upright direction on rotated photos | Open: these act along the stored axes, as for ordinary RAW; Adobe's convention for `PerspectiveVertical`, `UprightTransform_*` and the Upright mode on rotated photos is unverified (README) | — |
| B-N2 INT-1 orientation | Test only | `int1_offline_proxy_nested_locals_adobe_render_and_orientation` also asserts orientation 6 renders exactly as orientation 1 |
| B-N3 named/embedded lens profile on a rotated proxy | Test only | `catalog_orientation.rs::rotated_proxy_resolves_a_supplied_lens_profile_in_the_sensor_frame` (anisotropic coordinate scale, off-centre distortion, CA, vignetting; all 8 orientations equal). Passes |
| B-N5 stale LR-8b names | Renamed `cfa_and_generated_proxy_share_the_sensor_edit_frame`; the rotated-proxy GPU-tail decline is commented as a performance follow-up only | — |
| B-N6 `profile_rejects_tmpdir_widening` depends on checkout location | Picks its synthetic TMPDIR outside the OS scratch roots (cwd, manifest dir or the test binary's directory). Setup only; the assertions on main are unchanged | Passes |
| B-S5 private 12-pair comparison | Harness (ignored, private) now orients Develop output for display and records orientation/crop per pair. Run read-only against a scratch copy of the catalog and the Smart Previews/Previews bundles; outputs only in a scratch dir, never committed | Numbers below |
| ENG-6 rebase follow-up | `lr8m_relink_parity` real-RAW case uses ENG-6's `common/raw_fixtures.rs` (visible SKIPPED, failure under `TESSERA_REQUIRE_RAW_FIXTURES`) | Passes with the real fixture |

### B-S5: private comparison (numbers only)

Same 12 deterministic pairs (every 1,243rd of 14,924 comparable offline proxies),
64×64 Triangle comparison, after this lane's frame change:
- Rotated pairs (display orientation 5-8): **1 of 12** (pair 12, orientation 8). Rotated **and** cropped: **0 of 12**, so this sample cannot discriminate the crop frame on rotated photos.
- Orientation/aspect match: **11 of 12**, the same pairs as before; pair 12 still mismatches. Tessera's display output for pair 12 is portrait (683×1024), Lightroom's cached preview landscape (1752×1168). Pair 12 is uncropped, so the crop frame is not the cause; the harness does not apply the preview cache's own orientation column (`ImageCacheEntry.orientation`), which is the likely cause. Not investigated further in this lane.
- Luminance MAD per pair (8-bit): 5.72, 9.12, 5.55, 17.09, 11.48, 27.12, 17.93, 9.25, 11.85, 13.71, 19.64, 48.14; mean **16.38**, max 48.14 (LR-10 recorded mean 19.44 on its base). 8 of 12 pairs reference a lens profile that is unavailable to Tessera.

### Golden audit versus `origin/main` `fc3e757d`

Unchanged from the SP-INT audit: no golden PNG, `pipeline-cpu/tests/golden.rs`,
import-lrcat digest or `tests/data` change; `regenerate_goldens.rs` (LR-8d) and
the one added import-golden test as before; Cargo.lock only the two approved
raw-decode edges. Test files that exist on main with removed lines: setup only
in `cull/tests/grouping.rs`, `tessera-ffi/tests/lrcat_streaming_parity.rs` and
(SP-INT2) the TMPDIR location in `lrcat_profile.rs::profile_rejects_tmpdir_widening`.
New version identity: thumbnails of imported Lens Blur recipes now include
depth/model availability (no constant bump).

### Final gates (SP-INT2)

Env as above. Before the gates: `cargo clean --release -p` for every touched
crate (cull, export, image-core, import-lrcat, index, libraw-ffi, merge,
ml-embed, ml-quality, pipeline-adobe, pipeline-cpu, pipeline-gpu, raw-decode,
sidecar, tessera-ffi, tessera-mcp).

| Gate | Result |
| --- | --- |
| `cargo test --release --workspace --no-fail-fast` | At `24883aa9`: exit 0, **3478 passed, 0 failed, 108 ignored** (load 5-20). With ENG-6, `fixture_level3_matches_pipeline_cpu` runs every camera by default and passed; `raw_fixture_goldens` and the real-RAW M10 parity ran (no SKIPPED line). The two later commits are a test-only clippy fix (`sp_int2_gamut` rerun 2/2) and regenerated Swift bindings (covered by the Swift gates) |
| `IMAGE_CORE_ALL_FIXTURES=1 cargo test --release -p image-core fixture_level3_matches_pipeline_cpu` | **passed** (ENG-6 fixed the canon-cr3 failure SP-INT reported) |
| Native pixel goldens with real fixtures | `raw_fixture_goldens` passed in the workspace run |
| `cargo clippy --release --workspace --all-targets -- -D warnings` | Attempt 1: two lints in `sp_int2_gamut.rs`, fixed in `7d67910a`. Attempt 2: exit 0 |
| `cargo fmt --all -- --check` | exit 0 |
| `cd apps/mac && ./build-ffi.sh` | Attempt 1 regenerated bindings for `PrintImage.notes` (committed in `65927d65`); attempt 2: exit 0, no drift |
| `tools/orchestrate/swift-gate.sh` | **SWIFT GATE OK**: 996 tests, 3 skipped, 0 failures (load 6-9) |
| strict release build | exit 0 (141 s); only the known BLAKE3 linker warning |

Not done in SP-INT2: LR-8n (relinked RGB frame), the rotated-proxy GPU tail
admission (performance), verification of Transform/Upright axis semantics
against Adobe, and the pair-12 preview-orientation question above.

## SP-INT3 — re-review fixes (REV2-SP)

Review: `REV2-SP.out.md` (CHANGES-REQUIRED: one new blocker from the SP-INT2
S5 keying), read completely. Commits sit on top of `184974b1`; `origin/main`
had not moved (`fc3e757d`), so no rebase was needed. Tests first.

| Item | Code | Test |
| --- | --- | --- |
| NB1 (blocker) re-import silently dropped Tessera edits on in-place Smart Previews imported before SP-INT2 | (a) Apply re-keys every in-place Smart Preview in one pre-pass before any conflict check or write, so `existing_edit_conflict` judges the recipe the photo actually has; plan preview and apply agree. (b) **Superseded in SP-INT4 (NB2, NB3)**, which replaced this migration. `Sidecar::pin_protected_identities` (replaces `pin_protected_identity`) migrates rather than orphans: the recipe a path resolves to now (alias or legacy content key) is copied to the new key when that key has none, so byte-identical proxies that shared one legacy recipe each get a copy; the legacy object is removed once no remaining path alias references it (one directory listing per apply, only after a migration). With overwrite the copy is then replaced by the Lightroom recipe, so nothing is orphaned. (c) Key: `AgLibraryFile.id_global`, the UUID that already names each Smart Preview (`<id_global>.dng`) and that the importer uses to find it. It does not change when the catalog is renamed or moved (SP-INT2 hashed the catalog path). **Corrected in SP-INT4 (NS4):** it is *not* unique across catalogs: a duplicated or restored catalog keeps its ids. See SP-INT4 for how shared and conflicting ids are handled. Virtual copies share their master's file and Smart Preview, as before. (d) The pinned alias is written durably in the pre-pass, so skipped and resumed rows keep the key across a restart without a recipe write (N3) | `lrcat_rekey_tests.rs` (synthetic legacy state: content-keyed recipes carrying a Tessera edit from the app): without overwrite skipped and kept, plan and apply agree; with overwrite replaced, legacy object not orphaned; two byte-identical proxies with shared legacy edits both keep them; renamed and moved catalog (with its Smart Previews bundle) keeps edits; new keys survive a restart (child process) without a recipe write. RED 0/5, GREEN 5/5. Sidecar storage test moved to the batch API (11/11) |
| NS2 request cache held full recipes | `PreviewSources` caches `ImportedFacts` (stored original path, catalog orientation, imported AI raster keys, Lens Blur depth key) only; bounded at 50,000 entries. Entries are reused while the caller's recipe hash and the owner recipe file's size and mtime are unchanged | `sp_int3_request_cache_holds_a_small_projection_not_the_recipe`: one proxy with a 1 MiB retained Lightroom payload. RED 1,054,721 bytes retained (bound 4,096), GREEN |
| N4 model-folder listing per poll | Folded into NS2: the listing for Lens Blur identities is refreshed at most every 2 s per engine | Covered by the existing S6 identity test (identity computed directly) |
| N1 stale original/orientation in the request key | The render identity includes the stored original path and the catalog orientation; the cache is keyed by the recipe file stamp as above | `sp_int3_request_key_tracks_stored_original_and_catalog_orientation`. RED (same key after the stored original changed), GREEN |
| N2 print notes reuse the export wording | `render_pixels_with_notes` says "Rendered from a Smart Preview…"; file export keeps "Exported from…" | `sp_int2_proxy_outputs` print test now requires the render wording. RED, GREEN |
| NS1 HANDOFF understated main's Develop/export split | HANDOFF corrected (A-S1 row points here). **Separate finding for a future lane (main, not changed here):** main has no process dispatch for originals in export, so an Adobe-process original is exported and printed through the **Native** pipeline (the Adobe-recipe print is bit-identical to the Native-recipe print). Its whole look therefore differs between Develop (Adobe pipeline, hard clip) and export/print (Native pipeline, managed gamut mapping): per REV2-SP's measurement mean 4.4 / max 5.5 levels in gamut and mean 6.0 / max 36.8 saturated. Proxies, which export through `pipeline_adobe`, match Develop better than originals do on main | — |

Not done in SP-INT3: everything listed as open in SP-INT2 (LR-8n, rotated-proxy
GPU tail, Transform/Upright on rotated photos, pair-12 preview orientation),
and the Adobe-original export lane above. Already-imported libraries migrate
on their next Lightroom re-import (the pre-pass); until then they keep their
legacy content-keyed recipes, which still resolve.

### Final gates (SP-INT3)

Env as above; `cargo clean --release -p` for every touched crate before the
gates (same list as SP-INT2).

| Gate | Result |
| --- | --- |
| `cargo test --release --workspace --no-fail-fast` | At `f119d145`: exit 0, **3486 passed, 0 failed, 108 ignored** (load 13-23). `raw_fixture_goldens`, `fixture_level3_matches_pipeline_cpu` (all cameras by default since ENG-6) and the real-RAW M10 parity ran and passed; no SKIPPED line. The next commit is the clippy fix only (two test-only `cfg(test)` attributes and a function reference in a test); the affected tessera-ffi preview and rekey tests were rerun green |
| `cargo clippy --release --workspace --all-targets -- -D warnings` | Attempt 1: two dead-code errors (identity wrappers now test-only) and one redundant closure, fixed in the clippy commit. Attempt 2: exit 0 |
| `cargo fmt --all -- --check` | exit 0 |
| `cd apps/mac && ./build-ffi.sh` | exit 0, no bindings drift |
| `tools/orchestrate/swift-gate.sh` | **SWIFT GATE OK**: 996 tests, 3 skipped, 0 failures (load 16-23) |
| strict release build | exit 0 (158 s); only the known BLAKE3 linker warning |

### SP-INT3 commits

- `89e7204e` test(SP-INT3): re-keying in-place Smart Preview recipes keeps legacy Tessera edits
- `116bcacf` fix(SP-INT3): migrate in-place Smart Preview recipes to a Lightroom file key before any check
- `7620680c` test(SP-INT3): thumbnail request cache is a small projection; key tracks original and orientation
- `41b48ac4` fix(SP-INT3): thumbnail request cache keeps an identity projection, keyed by the recipe file
- `3f280dfd` test(SP-INT3): print notes say Rendered from a Smart Preview
- `f119d145` fix(SP-INT3): print and document notes say Rendered from a Smart Preview
- `a04f2233` style(SP-INT3): clippy: test-only identity wrappers, function reference

## SP-INT4 — migration safety and scale (REV3-SP)

Review: `REV3-SP.out.md` (CHANGES-REQUIRED: NB2 and NB3 in the NB1
migration), read completely. Commits sit on top of `261e3088`; `origin/main`
had not moved (`fc3e757d`). Tests first. Coordinator ruling applied: a recipe
is never deleted unless the recipe at its destination key holds identical
bytes.

| Item | Code | Test |
| --- | --- | --- |
| NB2 (blocker) migration deleted a recipe it never copied | `store::migrate_pins` replaced by `PinBatch` (`Sidecar::protected_pin_batch`; `protected_pin_preview` for the plan), with a per-photo `PinOutcome`. Destination absent: copy recipe and XMP (Migrated). Destination identical: share it (Shared when another source is pinned to it). Destination different and owned by another pinned source: **Conflict**: the photo keeps its own recipe and key; nothing is copied, merged or deleted. Destination different and owned by nobody (an interrupted migration): **Recovered**: the newer by mtime is on the key; if that is the legacy recipe the older copy is renamed to `<key>.backup-<ns>.json`, otherwise the legacy recipe is kept. A legacy recipe and its XMP are deleted in `finish()` only when no path or content alias references them and their bytes equal the destination's (N6: same rule for XMP). When a photo migrates, its content alias is pointed at the identical new object, so unmigrated sources with those bytes still find the edits | `sidecar/tests/pin_migration.rs`: two catalogs sharing a file id with different edits keep both (second reports Conflict and keeps reading its own); identical edits share one recipe and the legacy object is still deduplicated; crash after the copy before the key was saved, both directions (newer wins, other kept). RED was a compile error; the behavioural RED is REV3-SP's reproduction. GREEN 4/4 |
| NB2 visible conflict (ruling) | The import report and the plan preview list "Edits conflict" (kept separate), "Edits recovered" and "Shared with another catalog" entries (`unsupported` issues) | `lrcat_rekey_tests::sp_int4_second_catalog_with_different_edits_keeps_them_and_reports_a_conflict` (plan and report; A keeps 1.25, B keeps 2.5). RED 0/1, GREEN |
| NB3 (blocker) quadratic reference scan; no progress or cancel | Each store's `paths/` and `content/` alias directories are read once into reference counts. The apply pre-pass ticks `LrcatPhase::Preparing` ("Updating edit keys") and stops at a cancel request; `finish()` still runs for what was re-keyed | `pin_migration::migration_reads_each_alias_a_bounded_number_of_times`: 600 migrations read 1,200 alias files once (bound: aliases + 4 per pin; the old scan read every alias once per migrated object, about 720,000 reads here). A 20,000-recipe wall-clock bound was tried first and measured **451 s** under load 20-35; the time is three durable writes per photo (`sync_all` = F_FULLFSYNC on macOS, about 7 ms each), linear and now with progress and cancel. Expect several minutes for ~20k proxies on the first re-import after upgrade. The test was changed to the counted bound the coordinator allowed |
| NS3 concurrent Develop save during the pre-pass | Each photo is re-keyed under its own `OriginalWriteReservation`; a photo whose edits are reserved keeps its recipe and is reported ("Edit key not updated", re-keyed on the next import) | `sp_int4_photo_open_in_develop_is_not_rekeyed_under_it`. RED 0/1, GREEN |
| NS4 "unique across catalogs" claim; sharing not shown | SP-INT3 row corrected: `id_global` is stable across rename/move but not unique across catalogs (duplicated or restored catalogs keep it). Identical edits share one recipe and the report says so; differing edits are kept separate | `sp_int4_second_catalog_with_identical_edits_shares_and_reports_it`. RED 0/1, GREEN |
| N5 double hashing | Each in-place proxy is content-hashed once per apply (in the pre-pass); afterwards its pinned alias resolves without hashing, and the content-alias update reuses the cached hash | Analysis; no separate test |
| N6 XMP | Under the same never-delete-unless-identical rule (above) | Covered by the pin_migration tests (copy path) |

Not done in SP-INT4: no lrcat-level test of cancellation during the
pre-pass (the cancel check is the same flag the import loop uses, checked per
photo); the items listed as open in SP-INT2/SP-INT3 remain open.

### Final gates (SP-INT4)

Env as above; `cargo clean --release -p` for every touched crate before the
gates (same list as SP-INT2).

| Gate | Result |
| --- | --- |
| `cargo test --release --workspace --no-fail-fast` | At `7334497f`: exit 0, **3493 passed, 0 failed, 108 ignored** (load 10-35). `raw_fixture_goldens`, `fixture_level3_matches_pipeline_cpu` (all cameras) and the real-RAW M10 parity ran and passed; no SKIPPED line |
| `cargo clippy --release --workspace --all-targets -- -D warnings` | exit 0 (first attempt) |
| `cargo fmt --all -- --check` | exit 0 |
| `cd apps/mac && ./build-ffi.sh` | exit 0, no bindings drift |
| `tools/orchestrate/swift-gate.sh` | **SWIFT GATE OK**: 996 tests, 3 skipped, 0 failures (load 8-23) |
| strict release build | exit 0 (196 s); only the known BLAKE3 linker warning |

### SP-INT4 commits

- `2bab9f1d` test(SP-INT4): re-keying never deletes a differing recipe and scales linearly
- `4a368047` fix(SP-INT4): never delete a recipe that differs from its destination; one reference scan
- `f5de6747` test(SP-INT4): import reports edit-key conflicts, cross-catalog sharing and photos open in Develop
- `7334497f` fix(SP-INT4): re-keying reports conflicts and sharing, reserves each photo, ticks and cancels

## SP-INT5 — last nits before merge (REV4-SP)

Review: `REV4-SP.out.md` (APPROVE-WITH-NITS), read completely. Commits sit on
top of `7f235b34`; `origin/main` had not moved (`fc3e757d`). Tests first. The
RED commit for S1 is labelled `test(SP-INT4b)` by mistake; it belongs to this
lane (not reworded, per the lane rules).

| Item | Code | Test |
| --- | --- | --- |
| S1 crash recovery chose "newer" by mtime and left the loser unlabelled | The winner is the recipe whose recorded `last_writer` (timestamp, counter, machine) is newer, as the sidecar merge decides; file mtime only when a document has no recorded time. In both directions the loser is renamed to `<key>.backup-<ns>.json` (with its XMP) beside it; lookups only resolve `<key>.json`, so nothing reads or overwrites it. The content alias points at the winner. A legacy object other photos still use is left in place for them. The report's "Edits recovered" text names where the other version is | `pin_migration::crash_after_copy_before_key_save_keeps_the_newer_recipe_and_a_backup`, rewritten: recorded times decide while mtimes are falsified the other way; exactly one labelled backup holding the loser; no unlabelled loser at the legacy key; a never-pinned twin with the same bytes resolves to the winner. RED (3.0 shown instead of 2.0), GREEN |
| N9 plan/apply agreement in that case | The plan preview applies the same decision | Same test asserts the preview's outcome |
| N7 Develop open fails while its photo is re-keyed | `reserve_develop` waits up to 250 ms (5 ms steps, gate lock released) when an external writer holds the photo; longer writers still fail as before | `image_edit_admission::tests::sp_int5_editor_waits_out_a_brief_external_writer` (30 ms writer). RED, GREEN |
| N10 "Edit key not updated" guessed the reason | Each listed photo carries its actual reason (another import or export writing its edits, open in Develop, unsaved Develop changes, or the raw error) | `sp_int4_photo_open_in_develop_is_not_rekeyed_under_it` asserts the reason. RED, GREEN |
| N11 no import-level cancel test | Test-only hook cancels after k re-keyed photos | `sp_int5_cancel_during_rekey_then_rerun_keeps_every_edit`: cancel after 1 of 2, report cancelled, both edits intact; re-run keeps both, separate recipes. RED (not cancelled), GREEN |
| N8 plan refreshes re-hash every in-place proxy | The protected content-hash cache (keyed by path and file version) holds 131,072 entries instead of 8,192 (~200 bytes each) | `pin_migration::repeated_lookups_over_a_large_library_hash_each_proxy_once` (9,000 proxies, second pass re-hashes none). RED 9,000, GREEN |

### Final gates (SP-INT5)

Env as above; `cargo clean --release -p` for every touched crate before the
gates (same list as SP-INT2).

| Gate | Result |
| --- | --- |
| `cargo test --release --workspace --no-fail-fast` | At `8910fe9f`: exit 0, **3496 passed, 0 failed, 108 ignored** (load 10-19); no SKIPPED line, real-fixture tests ran. The next commit is clippy-only (`then_some`, an unused test helper removed); `pin_migration` rerun 5/5 |
| `cargo clippy --release --workspace --all-targets -- -D warnings` | Attempt 1: two lints, fixed in the clippy commit. Attempt 2: exit 0 |
| `cargo fmt --all -- --check` | exit 0 |
| `cd apps/mac && ./build-ffi.sh` | exit 0, no bindings drift |
| `tools/orchestrate/swift-gate.sh` | **SWIFT GATE OK**: 996 tests, 3 skipped, 0 failures (load 11-16) |
| strict release build | exit 0 (187 s); only the known BLAKE3 linker warning |

### SP-INT5 commits

- `2623110d` test(SP-INT4b): crash recovery picks the recorded newer edit and labels the loser
- `4b59d0d6` fix(SP-INT5): crash recovery uses recorded edit times and labels the loser
- `96eedf76` test(SP-INT5): Develop waits out a brief re-key; real reservation reasons; cancel and re-run
- `c1b6d5de` fix(SP-INT5): editors wait out a brief re-key; reservation reasons named; cancel hook
- `7c9bc3f6` test(SP-INT5): a large library is not re-hashed on every plan refresh
- `8910fe9f` fix(SP-INT5): size the protected content-hash cache for whole libraries
- `25b9d334` style(SP-INT5): clippy: then_some for the recorded stamp; drop an unused test helper
