# SP-INT — Smart Preview integration branch

Status: assembled, A-LR8 M8/M4/M10 done, rebased onto `origin/main`
`a61703d4` (batch 53, LR-CLEAN), final gates below. Ready for the
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

Generated by subject; every subject on the branch is unique. The HANDOFF
commit itself is on top of the last row.

### LR-8R + LR-8d/8e2/8e3 (origin/main..origin/wp/LR-8d)

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
| `666b9d48` | `86c4ca82` | test(LR-8): expose lossy smart-preview LibRaw decode blocker |
| `a1c83430` | `171d3137` | docs(LR-8): hand off missing LibRaw lossy DNG capability |
| `3433246b` | `678806a9` | test(LR-8): cover synthetic lossy linear DNG camera samples |
| `e374d096` | `5a333613` | test(LR-8): distinguish JPEG XL and verify read-only UUID lookup |
| `1dcbe4fe` | `bad4838c` | feat(LR-8): add bounded classic JPEG LinearRaw decoder and UUID lookup |
| `4795c7fd` | `d4f75166` | docs(LR-8): correct JPEG XL blocker and record decoder groundwork |
| `ec5ee6d7` | `62347dcc` | test(LR-8): cover 16-bit JPEG XL camera channels in synthetic DNG |
| `6399c216` | `fcf5185f` | test(LR-8): require camera DNG admission to Native and Adobe Develop |
| `edf066f9` | `8f72c04a` | test(LR-8): require offline proxy import, protected edits, copy and automatic relink |
| `0d698e23` | `f2d380c0` | test(LR-8): require DNG polynomial sample mapping before crop |
| `5dd4d6e1` | `51ca74f7` | test(LR-8): refuse unconsumed camera DNG correction lists |
| `695a801e` | `1676db61` | feat(LR-8): develop and relink editable Lightroom smart previews |
| `164da9d7` | `8a097721` | docs(LR-8): hand off validated editable smart preview import and relink |
| `f6a3f94c` | `acada42c` | test(LR-8b): require all catalog orientations before normalized edits |
| `0c12d9cd` | `2302a0d1` | test(LR-8b): cover proxy relink, source-safe profiling and numeric tone diagnostics |
| `ccda096c` | `bc37b333` | test(LR-8b): require shared BaselineExposure handling for originals and proxies |
| `d54ccf21` | `686b849f` | test(LR-8b): require oriented CFA parity and imported mask hooks |
| `9c69d92d` | `b458d43c` | test(LR-8b): require header-only smart-preview metadata indexing |
| `3f3e4c8a` | `8cdf63a3` | fix(LR-8b): align catalog orientation, RAW exposure and imported proxy masks |
| `f0ab1459` | `98bbfeac` | test(LR-8b): make metadata-only decode regression reject the full JPEG payload |
| `027f4e3b` | `fb583e8d` | fix(LR-8b): sample comparison pairs from proxies with standard previews |
| `0d547d66` | `bfd48a45` | test(LR-8b): cover split standard-preview JPEG levels |
| `4a5a323e` | `d429fd83` | fix(LR-8b): read current split-JPEG standard-preview caches |
| `6b0fa428` | `0d53f410` | test(LR-8b): cover REAL image IDs in the standard-preview index |
| `4af8fd44` | `4b1bdd60` | fix(LR-8b): accept integral REAL IDs in Lightroom preview databases |
| `985ea86a` | `cf2e2d8f` | test(LR-8b): cover DNG AsShotWhiteXY as the neutral alternative |
| `48519488` | `4f229f5d` | fix(LR-8b): decode AsShotWhiteXY camera-neutral metadata |
| `27664f0b` | `5aba5aa8` | test(LR-8b): require white-xy calibration inheritance from IFD0 |
| `73dbfeeb` | `3ccecc01` | fix(LR-8b): inherit white-xy and calibration tags from the DNG root |
| `a0546dbf` | `147ec47e` | test(LR-8b): retain unresolved lens references without blocking proxy previews |
| `c4c60e92` | `71e45a83` | fix(LR-8b): report unresolved lens profiles without blocking Develop |
| `fba5eaf0` | `bc1677c4` | test(LR-8b): require aspect-preserving comparison thumbnails |
| `1294093a` | `20059fc4` | fix(LR-8b): preserve comparison image aspect at a 1024-pixel long edge |
| `8b6d33df` | `564534cd` | docs(LR-8b): record orientation, exposure, real-catalog evidence and gates |
| `75d048ab` | `73f33c9f` | docs(LR-10): record integration conflicts and import golden boundary |
| `76be668d` | `c38edc44` | test(LR-8c): preserve ordinary import bytes with neighboring previews |
| `1074cdfc` | `aac33d7b` | fix(LR-8c): retain catalog orientation only for offline proxy recipes |
| `f38f86f0` | `832ebbae` | test(LR-8c): preserve original mask and depth extents with catalog rotation |
| `8cfea557` | `de7631df` | fix(LR-8c): keep original resource extents on the main import path |
| `40b5a981` | `a0670222` | test(LR-10): pin default curve and post-exposure look order |
| `3686e18c` | `cb01bda7` | feat(LR-10): use public ACR3 defaults and defer profile looks after exposure |
| `3f1705c9` | `a23b4db5` | test(LR-10): require baseline exposure offsets and explicit black-render policy |
| `a2d6de93` | `ed196858` | feat(LR-10): apply profile exposure and black policy between hue and look tables |
| `e9bd8d29` | `c66735c2` | test(LR-10): require embedded DNG profile fallback and Adobe-name dispatch |
| `a8d456fc` | `14cbac60` | test(LR-10): retain Adobe profile identity through Develop admission |
| `35047da9` | `18d80ec8` | test(LR-10): require signed RGB means in private pair measurements |
| `d3170174` | `7c212124` | feat(LR-10): resolve embedded DNG profiles and camera-neutral white balance |
| `d9b9ce7e` | `1182d43a` | feat(LR-10): measure luminance and signed RGB deltas from fixed private pairs |
| `97601a57` | `6214d086` | test(LR-10): preserve output-referred DNG tone and unknown-illuminant admission |
| `46265e9d` | `427d8288` | fix(LR-10): honor output-referred DNG defaults and unknown illuminants |
| `14204a9b` | `d1101cc5` | docs(LR-10): record rendering contract measurements and complete gate results |
| `bc94887e` | `f2564656` | test(LR-8e): expose LinearRaw admission and allocation regressions |
| `ee3a108f` | `b4013324` | fix(LR-8e): gate LinearRaw admission and bound tile decoding |
| `b79af9c4` | `b03b70f0` | docs(LR-8e): record hotfix scope and verified gates |
| `af031e7d` | `ce304180` | test(LR-8f): expose admission, read amplification and JPEG color regressions |
| `5fed2c78` | `70731747` | fix(LR-8f): restrict admission and bound compressed decoding work |
| `0a4a47a8` | `707d9670` | docs(LR-8f): record review resolutions, private parity and release gates |
| `9bc03578` | `53c54d9d` | test(LR-8g): cover large originals and ambiguous tile encodings |
| `7882fe16` | `5482b23e` | fix(LR-8g): admit large originals and reject ambiguous tile encodings |
| `59bf1d54` | `53c46acf` | docs(LR-8g): record cap rationale, parity, mutation and gate evidence |
| `e79d4607` | `6f4549b4` | test(LR-8h): expose the fixed 128 MiB compressed cap on large originals |
| `b150489e` | `0fc569a3` | fix(LR-8h): bound compressed tiles by file size and the decoded budget |
| `57ef9ae2` | `25a89bb7` | docs(LR-8h): record compressed-budget ruling, tests and gates |
| `7dea75cf` | `83f798d4` | test(INT-1): cover nested local edits on an oriented offline Adobe proxy |
| `ba6eba08` | `c0c579ce` | fix(LR-12): count and sample split Lightroom previews in import sheet |
| `9e8b31ef` | `b861762f` | test(LR-12): reproduce optional LinearRaw failures and audit recipe fields |
| `f9022b96` | `f08c05c8` | docs(LR-12): record partial fix, red regressions, and private baseline blocker |
| `299eaa18` | `7ce8a130` | test(LR-13): reproduce imported JXL proxy analysis decode failure |
| `4f4ac577` | `cd0a7fcc` | fix(LR-13): share imported proxy preview routing with analysis and assist |
| `1e0faff3` | `18bbe49b` | test(LR-13): cover culling, Adobe preview parity and proxy export notices |
| `b712fd3c` | `12a1dc93` | fix(LR-13): render LinearRaw proxies without unavailable optional settings |
| `8494a226` | `b0ea9228` | fix(LR-13): export from an external proxy with a quality warning |
| `4dd6a6fd` | `8d0646a9` | fix(LR-13): route imported proxy thumbnails, loupe, analysis and culling through the Smart Preview decoder |
| `35df7477` | `db598f10` | test(LR-13): reproduce full-resolution render for imported proxy thumbnails |
| `beb48639` | `2d4a999b` | fix(LR-13): render imported proxy thumbnails at thumbnail size |
| `ebfd9023` | `a1561f8f` | feat(LR-13): admit identity-oriented external LinearRaw to the resident GPU tail |
| `a2d6994c` | `fd47d8e5` | fix(LR-13): read JPEG XL LinearRaw proxies as merge inputs |
| `b97c458d` | `80656c84` | fix(LR-13): size imported mask rasters from the LinearRaw header |
| `36f4250a` | `f590a476` | feat(LR-13): surface per-photo proxy render notices in Develop |
| `f8cf128a` | `a2142ed8` | test(LR-13): add opt-in aggregate proxy admission audit and app-path sampler |
| `0f5ae2b0` | `b8f4dabc` | docs(LR-13): review map, entry-point findings, measurements and gate results |
| `15ab199d` | `94f9a558` | fix(LR-13): offer only .dng merge inputs to the LinearRaw reader |
| `97da44da` | `321002ed` | refactor(LR-13): extract the proxy notice sentences into one function |
| `7f083f03` | `fcefbab8` | test(LR-13): reproduce wrong proxy notice wording and the no-op HDR note |
| `7a6ecc63` | `765d8d1a` | fix(LR-13): correct lens blur/retouch notice wording and drop the no-op HDR note |
| `71bcac78` | `0223907d` | docs(LR-13): state the JPEG XL fixture's provenance after the LR-8f fixture regeneration |
| `9551060f` | `a75a9065` | test(LR-13): cover the minimum proxy notice through the FFI Develop session |
| `726c334a` | `202cf498` | docs(LR-13): record hotfix LR-8e..8h integration, approval conditions and re-measurement |
| `814517fb` | `981fbdc2` | test(LR-8R): require oriented proxy mask extents on the LR-5b base |
| `c77cb368` | `a1c6ea8d` | fix(LR-8R): validate imported proxy masks in the oriented active frame |
| `01a0cc8a` | `6aa3bfd5` | test(LR-8R): adapt LR-11b synthetic RAW metadata to proxy fields |
| `3aeb1d33` | `1646b624` | test(LR-8R): require proxy notices to recognize regenerated masks |
| `bafe4684` | `3f1a9925` | fix(LR-8R): align proxy mask notices with LR-5b readiness |
| `1e31cd27` | `2fbfd6f0` | docs(LR-8R): record port map, integration fixes and Rust gate attempts |
| `63be13a6` | `9f0949cd` | test(LR-8R): carry proxy options into the B5-50 accessibility fixture |
| `94c55e95` | `567263b6` | docs(LR-8R): record final gates and deferred accessibility conflict |
| `2d01aab4` | `b573a6a4` | fix(LR-8R): give the Smart Preview import controls namespaced identifiers |
| `2ee8bfc6` | `a7eee7c2` | docs(LR-8R): record LR-8R2 compatibility ruling and passing gates |
| `c2ad67b7` | `b121b238` | test(LR-8d): expose Native BaselineExposure regression without golden overrides |
| `0ac66088` | `32f95ed4` | fix(LR-8d): keep Native baseline neutral and apply Adobe exposure once |
| `c936e53c` | `e1f55ce8` | test(LR-8e2): restore DCP contracts and expose fallback scope and headroom failures |
| `0f324c94` | `98813622` | test(LR-8e2): require default unknown second illuminant without masking malformed matrices |
| `1417184b` | `6e040efb` | test(LR-8d): retain Adobe baseline gain validation after removing Native gain |
| `88bd0b0e` | `3555897c` | fix(LR-8d): validate finite positive baseline gain at Adobe boundaries |
| `9369c0c8` | `d4f90d19` | test(LR-8e2): require visible substitution notes and installed-profile precedence |
| `1adee24e` | `334ec897` | fix(LR-8e2): scope embedded DCP fallback to Adobe-named LinearRaw proxies |
| `60cb8f8d` | `0c758cac` | docs(LR-8e2): add Adobe DNG SDK licence attribution for the ACR3 table |
| `7f5b9637` | `ad2b0d43` | docs(LR-8d): record hunk dispositions, golden audit and final gates |
| `d7c6ec49` | `197b4345` | test(LR-8e3): pin Native profile refusal, approximation note, Auto WB entry points and SDK illuminants |
| `8d257d90` | `74e432cd` | fix(LR-8e3): refuse unreproducible Native profiles and plan proxy WB on every entry point |
| `5732bdcc` | `63f3e389` | docs(LR-8e3): document Native profile scope, rounding, PV2010 order and LookTable clamp |
| `01dbaa0d` | `7bc17df5` | docs(LR-8e3): record review fixes, fixture investigation and gates |

### LR-13b own commits (2ee8bfc6..861f3652, including the LR-13c port)

| Source | New | Subject |
| --- | --- | --- |
| `bb18640a` | `b56e632a` | test(LR-13c): forbid pixel work while opening synthetic proxy libraries |
| `a34193c9` | `a835c345` | test(LR-13c): cover lazy hashes, cancellation, reuse and cheap proxy pixels |
| `ecd966f4` | `f0ef5b85` | fix(LR-13c): defer cached cull hashes and reuse library listings |
| `da238001` | `b3fd9967` | docs(LR-13c): record lazy-open evidence and full verification gates |
| `8082d40e` | `831823c9` | test(LR-13b): reject dropped proxy AI masks in print and export |
| `7b7fef8b` | `14aec68a` | test(LR-13b): use the depth cache contract for range-mask fixtures |
| `26bc1417` | `4de2e603` | fix(LR-13b): preserve proxy AI and depth masks in exports |
| `599cc3bb` | `70433f3f` | test(LR-13b): require proxy depth and real retouch rendering |
| `22782ce4` | `69857e6b` | test(LR-13b): require thumbnail parity for available proxy effects |
| `86a68a15` | `4769780f` | fix(LR-13b): render proxy retouch and depth with real resources |
| `3e9aea33` | `aa4b2e80` | test(LR-13b): surface proxy raster and thumbnail mask failures |
| `b411b0a2` | `788a5830` | fix(LR-13b): surface proxy mask raster failures |
| `0875ac46` | `6ed54dcb` | test(LR-13b): cover Native profile errors and cached depth at the bridge |
| `4e3755a6` | `96377979` | test(LR-13b): use a pixel-valid TIFF field the profile reader rejects |
| `b8af4a90` | `b8b5553f` | fix(LR-13b): read embedded profiles only for Adobe-process proxy exports |
| `cae105a7` | `6e2c7055` | test(LR-13b): require a persistent loupe notice without per-frame FFI reads |
| `8570f913` | `73632617` | test(LR-13b): key imported thumbnails by mask availability and plan version |
| `61f46ee6` | `8c690641` | fix(LR-13b): include plan version and mask availability in thumbnail identity |
| `bfee012e` | `9d9ab2cc` | test(LR-13b): justify ab924590's resident-tail admission flip |
| `88e61485` | `72f061b7` | fix(LR-13b): persistent loupe notice, cached render notices |
| `ca1cd974` | `a8dc02d7` | style(LR-13b): rustfmt preview tests |
| `3e169ea0` | `a00b4b1b` | docs(LR-13b): record STEP 1 and STEP 2 items and attempts |
| `a06eb0da` | `1d5a0d60` | test(LR-13b): catalog names, image-keyed copies, listing facts, safe defaults |
| `5eb6962f` | `33c86a3b` | test(LR-13b): LR-8h decoder peak is output plus one tile |
| `bdfc02fc` | `5e633760` | fix(LR-13b): stream LinearRaw tiles and crop the output in place |
| `1cddb262` | `672be3b0` | fix(LR-13b): catalog names, image-keyed copies, scan-time listing facts |
| `03887afa` | `b88c15b5` | feat(LR-13b): show imported Smart Previews under their catalog file name |
| `09b0ff6e` | `77a9dccb` | style(LR-13b): keep the proxy-owner doc comment on its function |
| `b398b73d` | `b4d215d6` | fix(LR-13b): use the Theme secondary ink in the loupe notice |
| `861f3652` | `fd9652c4` | docs(LR-13b): record STEP 3 items, attempts and final gate results |

### LR-13d, LR-13e, LR-13f (199933c8..origin/wp/INT-1-rut-build)

| Source | New | Subject |
| --- | --- | --- |
| `e4c1bd4e` | `7fefe98a` | test(LR-13d): reproduce deferred grouping, cache policy, and queue regressions |
| `749b8712` | `5d05392e` | fix(LR-13d): join hash workers, bound default regroup, approve cache roots |
| `a350fa8a` | `9d4efb56` | docs(LR-13d): record review fixes, finding map and gate results |
| `429ee7c7` | `eb63ce7c` | test(LR-13e): reproduce whole-library default regroup, joint joins and quit hangs |
| `8fabb780` | `b5ef980c` | fix(LR-13e): incremental default regroup, independent joins, bounded quit |
| `dca744e7` | `b08f06a2` | test(LR-13e): finish deferred hashing before ml grouping assertions |
| `6d6fdca2` | `9e4840c9` | docs(LR-13e): record re-review fixes, finding map and gate results |
| `32aff94a` | `b2a1fba5` | test(LR-13f): reproduce poll blocking on piled-up wake tickets |
| `8940e108` | `9d65703b` | fix(LR-13f): only the Wake ticket clears wake_pending |
| `95399931` | `443a388a` | docs(LR-13f): record wake-ticket fix, coverage nits and gate results |

### New in this lane

| Before rebase | New | Subject |
| --- | --- | --- |
| `a96cc529` | `c4229eea` | fix(SP-INT): plan proxies with their render resources at every entry point |
| `62a82d06` | `4fa8e6f0` | test(LR-8m): one sensor edit frame for proxies, relinked and ordinary RAWs |
| `5b6836ee` | `3ca0ea94` | test(LR-8m): edit export recipes through history; align remaining frame expectations |
| `0dff9a2c` | `e9fbaf20` | style(LR-8m): rustfmt frame tests |
| `7c4d2599` | `a99c43f8` | fix(LR-8m): catalog orientation is a display orientation; edits stay in the sensor frame |
| `35bb7f4e` | `630c2154` | test(LR-8m): relinked originals take the ordinary RAW, GPU and export routes |
| `953c4a4f` | `57c1d19c` | test(LR-8m): compare relinked GPU frames through Develop's render route |
| `3268fcc0` | `1f25208e` | fix(LR-8m): relinked originals use the ordinary RAW, GPU and export routes |
| `14ff3a96` | `4dad527f` | test(LR-8m): Smart Preview edits land the same on the relinked original |
| (after rebase) | `cd58c8c4` | style(LR-8m): drop outer dead_code allows that duplicate common's inner allow |

## Final gates

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
