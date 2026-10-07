# LR-13b — proxy correctness and library follow-through

Work in progress. Base: `2ee8bfc6` (`wp/LR-8R`). Binding rulings:
`A-LR13-REVIEW.md` and `A-LR8-REVIEW.md`, read completely.
Synthetic fixtures only. No foreground GUI or system-settings changes.
No board or lockfile changes. No co-author trailers.

## Findings and coverage

| Finding | Code | Test / status |
| --- | --- | --- |
| LR-13c deferred, bounded and cached near-duplicate work | Ported four commits in order; no conflicts | Initial open RED: 32 pixel calls vs required 0; 879 passed, 0 failed, 42 ignored |
| Proxy export/print retain imported AI/depth masks; unavailable AI is an error | `export::proxy_recipe`, `ai_masks::ready_masks` and process-aware proxy hook | RED: 2 failures; GREEN: 2 passed (Native and Adobe, AI and cached depth) |
| Proxy lens blur and retouch use actual dependencies | Pending | Pending |
| Proxy mask raster errors surface in thumbnails/export | Pending | Pending |
| Embedded-profile parse errors do not block Native proxy exports | Pending | Pending |
| Persistent notice survives status changes without per-frame FFI locking | Pending | Pending |
| Thumbnail identity includes raster availability and render-plan version | Pending | Pending |
| Thumbnail same-level render_region comparison | Pending | Pending |
| Admission assertion in ab924590 | Pending review | Pending |
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
