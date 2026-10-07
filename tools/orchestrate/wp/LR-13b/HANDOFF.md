# LR-13b — proxy correctness and library follow-through

Work in progress. Base: `2ee8bfc6` (`wp/LR-8R`). Binding rulings:
`A-LR13-REVIEW.md` and `A-LR8-REVIEW.md`, read completely.
Synthetic fixtures only. No foreground GUI or system-settings changes.
No board or lockfile changes. Commits up to `0875ac46` are by a Codex worker
(no trailers); later commits are by Claude Opus 5.5 (Co-Authored-By trailer,
"Mixed authorship" noted where finishing the Codex worker's uncommitted work).

## Findings and coverage

| Finding | Code | Test / status |
| --- | --- | --- |
| LR-13c deferred, bounded and cached near-duplicate work | Ported four commits in order; no conflicts | Initial open RED: 32 pixel calls vs required 0; 879 passed, 0 failed, 42 ignored |
| Proxy export/print retain imported AI/depth masks; unavailable AI is an error | `export::proxy_recipe`, `ai_masks::ready_masks` and process-aware proxy hook | RED: 2 failures; GREEN: 2 passed (Native and Adobe, AI and cached depth) |
| Proxy lens blur and retouch use actual dependencies | Resource-aware plan; CPU/Adobe retouch and pre-geometry depth hooks; Develop/export/thumbnail routes | 3 integration tests + 1 thumbnail test, RED then GREEN |
| Proxy mask raster errors surface in thumbnails/export | Proxy scalar hook propagates errors; thumbnails reject unavailable imported planes | Core 1 test + FFI 1 test RED; core 1 and FFI 2 GREEN (including effects regression) |
| Embedded-profile parse errors do not block Native proxy exports | `tessera-ffi/src/export.rs` `Source::open(path, orientation, process)`: embedded profile read only for `ProcessFamily::Adobe`; call sites in export/print/document pass `recipe.process_version`. Minimal call-site change, no DCP/BaselineExposure code touched. `export::needs_segmenter` excludes depth components (served from the depth cache) | `tessera-ffi/tests/lr13b_proxy_bridge.rs`: Native print+export with a pixel-valid TIFF field the profile reader rejects; cached depth range exports without a segmentation model. RED 0/2, GREEN 2/2 |
| Persistent notice survives status changes without per-frame FFI locking | `DevelopController.renderNotices` cached, refreshed on open/history/commit/non-interactive flush/mask flush (`refreshRenderNotices`); `AppModel.developRenderNoticeList` + `developRenderNotices(for:)`; `SmartPreviewLoupeBadge` (`loupe.smart-preview-notice`, badge label lists notices) | `SmartPreviewNoticeAccessibilityTests` (2) restored from 1f64ff6f; `DevelopRecoveryAdmissionBehaviorTests.testProxyRenderNoticeOwnsOnlyItsPhotoAndPreservesNewerStatus` adds zero `renderNotices()` calls across 5 frame callbacks and the persistent-list assertions. RED compile; GREEN 3/3 |
| Thumbnail identity includes raster availability and render-plan version | `tessera-ffi/src/preview.rs`: `RequestKey.render = imported_render_identity(recipe, support)` = blake3(`IMPORTED_RENDER_PLAN_VERSION` = 2, each referenced imported raster key + presence + pinned payload revision). Zero for ordinary originals | `lr13b_thumbnail_request_identity_tracks_mask_rasters_and_plan_version`: missing raster fails, storing it re-renders (not the cached failure), bytes delivered; version and raster removal change the identity. RED compile, then RED 0/1 with identity disabled; GREEN 16/16 preview tests |
| Thumbnail same-level render_region comparison | test only | `lr13_imported_proxy_thumbnail_renders_at_thumbnail_level` now asserts exact equality with `Renderer::render_region` at levels 2, 1, 0 |
| Admission assertion in ab924590 | Kept (justified): the old "every external DNG declines" assertion encoded a limitation; the Metal regression checks GPU submissions and CPU agreement | `export/tests/lrcat_jxl.rs::external_dng_resident_tail_admits_only_identity_native_rgb`: identity/none orientation admitted; rotated 3/6/8, lens blur and retouch still decline (CPU route). 4/4 |
| M4 relink uses ordinary RAW GPU/lens route | Pending | Pending |
| M5 remaining library-listing work | Pending | Pending |
| M6 catalog filename display/export, never accessibility identity | Pending | Pending |
| M7 copies keyed by image ID | Pending | Pending |
| M8 common orientation/geometry frame and lens ordering | Pending | Pending |
| M10 normalized proxy edits survive full-resolution relink | Pending | Pending |
| Minor: copy toggle accessibility ID | Audit pending | Pending |
| Minor: import override test-only | Pending | Pending |
| Minor: library.json only in app support | Pending | Pending |
| LR-8h compressed tiles streamed one at a time | Pending | Pending |

## Attempts

1. Imported both LR-13c test commits. Full lazy-preview test target could not
   compile before implementation: the async preview API did not yet exist.
2. Ran the original first test commit's `lr13c_open_never_calls_pixel_provider`
   against the unchanged implementation: **0 passed, 1 failed**; actual 32
   pixel calls, expected 0. Restored the complete imported test file unchanged.
3. Cherry-picked LR-13c implementation and handoff without conflicts.
4. `cargo test --locked --release -p cull -p raw-decode -p tessera-ffi
   --no-fail-fast`: **879 passed, 0 failed, 42 ignored**, exit 0.

12. Profile deferral: the Codex RED fixture corrupted CalibrationIlluminant1
    out of file range, which the pixel reader also rejects (test failed on its
    precondition `lossy_dng::read(..).unwrap()`, 1 passed / 1 failed with the
    fix applied). Fixture-only fix (`4e3755a6`): IFD0 NewSubFileType retyped LONG→SHORT. Assertions unchanged.
    RED without fix 0/2; GREEN 2/2.
13. Notice: first GREEN 1/3 (badge label read back nil: SwiftUI reports a
    static-text element's label as AXValue). Badge made a plain element. 3/3.
14. Thumbnail identity: compile RED; behavioural RED with identity forced to
    zero 0/1 (cached failure returned after the raster was stored); GREEN.

## Final gates

Not yet run. Required: clean touched crates; release workspace tests;
release workspace all-target Clippy with warnings denied; formatting;
FFI build and no generated binding drift; Swift gate; strict release app build.

5. Proxy masks initial RED: **0 passed, 2 failed** (missing mask silently
   dropped; stored zero/one alpha produced identical print pixels).
6. First GREEN attempt reached an invalid depth fixture (depth ranges cannot
   carry Adobe segmentation metadata). Corrected only fixture construction to
   use the existing depth cache; retained all pixel assertions and bounds.
7. Corrected depth fixture RED: **1 passed, 1 failed**, unsupported AI mask kind.
8. Proxy mask GREEN: **2 passed, 0 failed**, including Native/Adobe and
   print/file output. Full export suite: **114 passed, 0 failed, 7 ignored**, exit 0.

9. Proxy effects RED: **0 passed, 3 failed** (available effects omitted and
   invalid depth accepted). First build after the fix had one missing context
   argument; corrected the call. GREEN: **3 passed, 0 failed**.
10. Thumbnail effect RED: **0 passed, 1 failed**; the thumbnail lacked the
    registered retouch/depth resources. GREEN: **1 passed, 0 failed**.
    No profile parsing, DCP, BaselineExposure, or golden changes.

11. Raster errors: core RED **0 passed, 1 failed** and thumbnail RED
    **0 passed, 1 failed**. GREEN: core **1 passed**, thumbnail/effects **2 passed**.
    Core checks both processes at levels 0, 1 and 2 and zero emitted tiles on error.
