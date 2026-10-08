# Machine B merge log

From 2026-10-07 Machine B is the sole coordinator and main merger (Machine A is offline for about a month). Same standard as Machine A: every lane is reviewed by an independent reviewer that did not write it; the full gate runs on the exact merged tree with a per-candidate target directory; every exit is verified 0 before main moves. Gate = workspace release tests, workspace clippy -D warnings, fmt, build-ffi with zero bindings drift, swift-gate, strict release build.

## Batches 50–52 (main 862d7db1)

- batch 50 (f151fe8d): B5-21/25 keyboard Tab loop and key safety over focused panel views, plus B5-49/49b/49c/49d/49e automated keyboard checklist. Full Keyboard Access is read through one injectable policy and pinned in-process for tests (no preference writes); traversal is asserted by responder identity with bounded, order-checked walks that tolerate SwiftUI focus proxies. Production fix: Tab leaves a Layers row when the inspector joins an existing window. Independent reviews: B5-49d CHANGES-REQUIRED → B5-49e APPROVE. Known limit: never run on a machine with Full Keyboard Access really on.
- batch 51 (9d35851b): LR-5/5b/5c/5d Lightroom AI masks. Rasters regenerated (Adobe rasters are not read), keyed by content with request-aware session identity; oriented import extents; an unavailable AI component skips the adjustment in preview (pending/unavailable UI) and is an export error; download-on-first-use unchanged; publication, ownership and collection share one admission so cancel → prune → resume keeps masks. Independent reviews: CHANGES-REQUIRED (2 blockers) → LR-5d APPROVE.
- batch 52 (862d7db1): LR-11/11b/11c Lightroom local mask adjustments (local curves, Point Color, colour overlay, defringe) with CPU fallback; AI-instance masks unsupported; extended local curves only for HDR output; one local Point Color stage; source groups matched by stable id; local defringe −100..100. Lane evidence sanitized (no private paths) with a guard test. Independent reviews: CHANGES-REQUIRED (privacy) → LR-11c APPROVE.
- Gates on 862d7db1: Rust 3,276 passed / 0 failed / 99 ignored; clippy 0; fmt 0; bindings drift 0; swift-gate 986 executed, 3 skipped, 0 failures; strict release build 0. Batch 50 alone (f151fe8d): Rust 3,183 / 0; swift 986 / 0.
- Real catalog (aggregate counts only): develop-settings warnings 1,289 → 738; MaskGroupBasedCorrections 633 → 82 (64 person/part masks and 17 instance selections remain unsupported by ruling).

## Batch 53 (main d1263985)

- LR-CLEAN/2/3 post-merge cleanup from Machine A's reviews: LR-1 v4 predicate only on Adobe subfields; LR-2 sticky-schema test restored; LR-4 partial MaskValue on non-Paint masks unsupported (warning + source); LR-3 CPU-chain selection factored before admission with full settings validation, Lua hook order geometry → LR-2 → retouch; LR-6 exact reason/error-kind pins; LR-9c `cloud` matrix status, retained mixed-retouch source, circle CenterWeight/Feather agreement check, empty DepthBasedCorrections no-op; identifier pins regenerated with anchored templates (document.layers.fx / layerStyle restored after review), stale layer row identifiers fixed; ENG-4 host-computed log constants for presence/tone_local, independent f64 oracle, overflow handling (decode(NaN) stays NaN); the two wall-clock tests release-only, script timeout asserts outcome with hang guards; B5-49 focus claim only in document mode, anchored address normaliser. Keyword-name collision left as a pending product decision.
- Independent review: APPROVE-WITH-NITS → S1–S3 fixed in LR-CLEAN3 (coordinator-checked delta).
- Gates on d1263985: Rust 3,290 passed / 0 failed / 99 ignored; clippy 0; fmt 0; bindings drift 0; swift-gate OK; strict 0.
- Found while reviewing LR-8d (fixed in batch 54): image-core `fixture_level3_matches_pipeline_cpu` fails for canon-cr3 (max diff 0.0433) when IMAGE_CORE_ALL_FIXTURES=1; the default gate runs only the ARW case, so this mismatch is silently skipped on main.

## Batch 54 (main f741b1c4)

- ENG-6 raw fixture parity. The image-core L3 test's reference model applied lens geometry at full resolution before level-size Detail, contradicting the documented preview stage order (downsample → Detail → Geometry at level size); with Auto lens profile estimating distortion on the CR3 and RAF samples this showed as 0.043 / 0.264 differences, hidden because the gate ran only the ARW. Reference corrected; new exact L0 engine-vs-pipeline-cpu test across all cameras (independent oracle; mutation-checked by review); every fixture in fixtures/raw runs by default; missing fixtures print SKIPPED (TESSERA_REQUIRE_RAW_FIXTURES=1 in CI fails instead). No tolerance loosened, no golden changed.
- Independent review: APPROVE-WITH-NITS (legitimate oracle correction). Open product question raised: Auto lens profile applies distortion estimated from image content when no lens profile exists (k1 −0.10 / −0.13 on the CR3 / RAF samples).
- Remaining silently-skipping fixture tests outside image-core/pipeline-cpu are listed in tools/orchestrate/wp/ENG-6/HANDOFF.md.

## Batch 55 (main 3e19cbf5) — Smart Previews

- Lightroom Smart Previews (JPEG XL / lossy-JPEG LinearRaw DNG) as editable offline proxies: LR-8/8b/8c port (LR-8R onto the LR-5-free main), LR-10 Adobe parity, LR-8e–8h safe decoding (admission only for identified lossy LinearRaw; all IFD fields validated before allocation; Σ compressed ≤ min(file size, decoded budget); streaming tiles; seeded mutation tests), LR-8d BaselineExposure split (Native unchanged; Adobe/proxies apply 2^(baseline+user) once), LR-8e2/8e3 embedded-profile fallback only for proxies naming an uninstalled Adobe profile (Native: only Adobe Standard/Color, approximated with a note), DCP spec defaults, HueSatMap without pre-exposure clamp, Adobe DNG SDK NOTICE; LR-12/13/13b proxy routing for thumbnails, loupe, analysis, cull, Develop, export, print, documents, with persistent notices; LR-13c–13f library open does no pixel work (deferred, cached, incremental near-duplicate hashing with joined shutdown); LR-8m one sensor frame for crop/masks/Upright on proxies and ordinary imports (catalog orientation display-only), relinked originals on the normal raw/GPU path, proxy→original edit parity; SP-INT1–6 integration, B1 thumbnail with pending AI raster, gamut parity with originals, id_global edit keys with never-delete migration (conflicts kept separate, crash recovery by recorded edit time with labelled backups, linear with progress/cancel).
- Independent reviews: Machine A (LR-8/8b/8c/10 changes-required; 8e/8f approved; LR-13a approved), then REV-SP-A/B → REV2-SP → REV3-SP → REV4-SP APPROVE-WITH-NITS; LR-8d APPROVE-WITH-NITS; LR-13c → REV2/REV3 (LR-13d/e/f). Reviews in ~/tessera-evidence/rulings.
- Gates on 3e19cbf5: Rust 3,496 passed / 0 failed / 108 ignored; clippy 0; fmt 0; bindings drift 0; swift-gate OK; strict 0. First gate attempt failed one counted-operations test that over-counted under parallel siblings (process-wide counter); fixed by serializing that test file (SP-INT6), bound unchanged.
- Open follow-ups: LR-8n relinked RGB originals use the rotated frame; GPU tail for rotated proxies; Transform/Upright direction on rotated photos vs Adobe; Adobe-process originals export via the Native pipeline on main (Develop/print look differs); maker-note lens corrections; remaining silently-skipping fixture tests (ENG-6 HANDOFF).

## Batch 56 (main 488dea82)

- LR-8n/8n2: relinked catalog-oriented RGB originals (JPEG/TIFF/HEIC/PNG, working-space DNG) are decoded in their stored frame and report the catalog orientation for display (new `RenderSource::StoredRgb`), so crop/masks stay on the same content as the Smart Preview after relink; orientation applied exactly once in Develop, thumbnails, analysis, export, print, documents and export AI-mask segmentation. Ordinary RGB imports unchanged (pixel-pinned).
- Independent review: merge-ready; S1 export-segmentation test added (fails under mutation), N1/N5 done. Follow-ups: relink guard for originals whose pixels were rotated after cataloguing; ordinary/online RGB imports still store edits in the rotated frame (affects none of the user's current photos per a read-only scan; product decision parked).
- Gates on 488dea82: Rust 3,500 / 0; clippy 0; fmt 0; drift 0; swift-gate OK; strict 0.

## Batch 57 (main 9fc49548)

- ENG-9/9b: export, print, documents, MCP export and HDR export render Adobe-process (imported Lightroom) recipes through the same Adobe pipeline Develop uses; Develop honours the recipe's gamut policy (Perceptual default; Clip bit-identical to before). Develop-vs-output difference for Adobe RAW went from up to 32.8 levels in gamut and 255 saturated to ≤ 0.53 (quantisation). Develop's HDR viewport for Adobe originals fixed; Adobe HDR export now matches Develop (SDR-range content) and tells the user.
- Independent review: APPROVE with should-fix; HDR notice and retouch parity row added in ENG-9b.
- Known cost: Adobe-process exports are CPU-only at full resolution — roughly 20–60× slower than main's previous (wrong-look) GPU path. ENG-10 (scaled prefix, GPU Adobe path, memory bounds) is running.
- Gates on 9fc49548: Rust 3,511 / 0 / 107 ignored; clippy 0; fmt 0; drift 0; swift-gate OK; strict 0.

## Batch 58 (main eeb5a955)

- ENG-7/7b/7c lens corrections match Lightroom (user decision 2026-10-07): the default Auto lens mode applies the raw's embedded correction or a matching profile, never a distortion/vignette estimated from image content (estimation only via explicit AutoCalibrated); "Remove Chromatic Aberration" defaults to off for recipes that never stored it (imports honour AutoLateralCA); built-in DNG opcode corrections always apply, including per-colour CA, even with lens mode None or an unavailable named profile; unavailable named profiles render uncorrected with a note in Develop and export (no per-file "no lens profile" warning files); preview cache render epoch bumped so old distorted thumbnails are not served; legacy Smart Previews of opcode raws in mode None open as Stale ("regenerate from original") with journal-only sync and no edit loss. Release notes in docs/RELEASE-NOTES.md (existing edits on previously estimated photos shift slightly).
- Re-pins: default recipe hash, six serialisation pins, import-lrcat digests/baselines/compat files, sidecar ACR packet pins — reviewer verified by decode diff that they differ only by the CA default and its history patch.
- Independent reviews: REV-ENG-7 CHANGES-REQUIRED → REV2 CHANGES-REQUIRED → REV3 APPROVE-WITH-NITS. Nit left: rebuilding a Stale preview deletes it before regenerating.
- Gates on eeb5a955: Rust 3,541 / 0 / 107 ignored; clippy 0; fmt 0; drift 0; swift-gate OK; strict 0.

## Batch 59 (main 56003b05)

- ENG-10/10b fast Adobe-process export and print: Adobe outputs render through Develop's own renderer at the pyramid level for the render scale (print at scale > 1; in-app file exports still render at scale 1 then resize), Adobe stages run tile-parallel within RendererConfig.threads (bit-identical to serial; Develop's Adobe renders ≈5× faster), banded parallel output transform, file outputs skip unused gamut warnings, memory-aware pairing in the CLI batch pipeline. 16 MP ARW full-size export 12.3 s → ~1.5 s; in-app 2048-px export ≈1.4–2.4 s. Full-size CR3/RAF now match Develop exactly also with AutoCalibrated lens (pipeline_adobe differed). Coordinator ruling: ENG-9 Adobe export-vs-Develop max bound 0.60 level (mean 0.35) — ICC matrix quantisation near the sRGB toe on clipped-plus-dark channels, pre-existing.
- Independent review: APPROVE with should-fix → ENG-10b (docs/claims corrected, CR3/RAF parity rows, thread cap). GPU Adobe stages judged not worth a lane (≤0.4 s/16 MP, would break exact equality).
- Gates on 56003b05 (with TESSERA_REQUIRE_RAW_FIXTURES=1): Rust 3,552 / 0 / 109 ignored; clippy 0; fmt 0; drift 0; swift-gate OK; strict 0.

## Batch 60 (main 54e32768)

- ENG-11 honest fixture/model tests: shared dev-only crate crates/test-fixtures; every fixture- and model-weight-dependent test runs by default when its input exists, prints SKIPPED when absent, fails under TESSERA_REQUIRE_RAW_FIXTURES=1 / TESSERA_REQUIRE_MODEL_WEIGHTS=1; pipeline-gpu fixture gate covers all five cameras (tolerances unchanged); no previously hidden failures. Cargo.lock adds only the internal test-fixtures package (coordinator-approved). Merge needed one fix: ENG-10b's new test still included the moved helper by path.
- Independent review: APPROVE. Follow-ups (ENG-12): ml-embed HNSW top-1 flake (~1.5%, random graph levels) — re-rank candidates exactly + relax the test to top-5 membership; ~10 fixture tests still #[ignore]/env-gated though fixtures/raw has their inputs; ml-faces README variable name.
- Gates on 54e32768 (with TESSERA_REQUIRE_RAW_FIXTURES=1): Rust 3,555 / 0 / 109 ignored; clippy 0; fmt 0; drift 0; swift-gate OK; strict 0.

## Batch 61 (main d55486fb)

- ENG-12/12b: ml-embed HNSW search exactly scores points outside the graph's largest strongly connected component (with an open exact-SQLite fallback above 5% isolated), fixing the ~1.5% top-1 flake without keep_pruned (which the review showed cut recall 5–25× on clustered/near-duplicate data); clustered and exact-duplicate recall tests; eight previously env-gated/ignored fixture tests now run by default (raw-decode captured CFA, export lens analysis, pipeline-gpu resident NEF, tessera-ffi and Swift Smart Preview workflows); export band planner charges sensor-domain stages per sensor pixel and developed stages per developed pixel via a shared image-core BandGeometry, so cropped sensors (Canon CR3 fixture) take the GPU band path again; plan-time decline. Compositor CI skips are visible; ml-faces README documents TESSERA_REQUIRE_MODEL_WEIGHTS.
- Independent reviews: REV-ENG-12 REQUEST CHANGES (keep_pruned) → REV2 APPROVE. Pre-existing finding opened as ENG-13: the pipeline-gpu export buffer pool does not count idle recycled buffers, so a band worker can hold ~350 MiB against its 192 MiB share.
- Gates on d55486fb (TESSERA_REQUIRE_RAW_FIXTURES=1): Rust 3,568 / 0 / 100 ignored; clippy 0; fmt 0; drift 0; swift-gate OK; strict 0.

## Batch 62 (main 5c57b2c7)

- ENG-13 export GPU buffer pool: recycled buffers are counted when a band transaction takes them and kept in an idle list; exact-size reuse; export transactions evict idle buffers oldest-first under pressure; uploads metered; checkpoint releases only counted bytes; readback publishes true held bytes; per-band-renderer statistics (previously shared across in-flight bands). Device-measured peak export scratch (MTLDevice.currentAllocatedSize) fell from 494–677 MiB to 346–394 MiB across five fixtures × three scales; export ~8% faster.
- Independent review: APPROVE. Follow-up ENG-14 (running): the vignette/grain effects map is uncounted and built per band worker (903 MiB measured on a 36 MP NEF with vignette), plus ~10 MiB of unmetered staging/params.
- Gates on 5c57b2c7 (TESSERA_REQUIRE_RAW_FIXTURES=1): Rust 3,571 / 0 / 100 ignored; clippy 0; fmt 0; drift 0; swift-gate OK; strict 0.

## Batch 63 (main 229bf0bb)

- ENG-14: GPU exports never build the vignette/grain effects map (inline WGSL path, bit-identical over 60 independent comparisons; interactive renders still build it); every export device allocation is metered (upload staging copies, parameter buffers/arenas, resize offsets/taps, readback staging; output LUT taken off BUDGET); planner sensor term 26→30 B + 2 MiB/band. Device-measured peak ≤ 380.8 MiB vs 384 MiB BUDGET on 5 fixtures × 3 scales × none/vignette/grain (was up to 904 MiB). New device-peak diagnostics.
- Independent review: APPROVE (merged by rebasing onto main; the branch carried replayed ENG-13 commits). Follow-ups in ENG-15: CR3 Web export +22% (16→19 bands); pre-existing ~1.8 MiB leak per GPU-declined export (presence recipes) until the next GPU submission.
- Gates on 229bf0bb (TESSERA_REQUIRE_RAW_FIXTURES=1): Rust 3,576 / 0 / 101 ignored; clippy 0; fmt 0; drift 0; swift-gate OK; strict 0.
