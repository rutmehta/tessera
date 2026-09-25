# Tessera — Build Status (2026-09-25)

Auto-maintained by the coordinator. Board: `python3 tools/orchestrate/board.py list`.

## Merged and verified
**Engine (Rust, `crates/`)**: engine-api contracts 1.1 (recipe/history/crs table/tool API), libraw-ffi + raw-decode (CR3/ARW/NEF/RAF/DNG), pipeline-cpu (M1+M2 operators, native revision 2, goldens), pipeline-gpu (wgpu ports, GPU-resident render path: tone 4 ms / WB 7–23 ms at L2 on a 36 MP NEF), image-core (tiled progressive renderer, stage memoisation, f16 cache, mask hooks), jobs scheduler, index (SQLite/FTS5, <10 ms search at 100k), sidecar (XMP + recipe JSON, native round trip), previews (JPEG pyramid, rendered fallback), export (JPEG/PNG/TIFF, ICC, XMP, batch, upscale hook), import-lrcat (read-only .lrcat → plan), library (albums, groups, smart albums, saved-search grammar), cull (decisions, groups, dHash, derived status, defect sweep, learning, face strip), lens (Brown–Conrady, lensfun/LCP, auto-calibration, Upright, composed geometry), color-mgmt (ICC registry, display/proof transforms, gamut warnings, GPU LUT), merge (HDR deghost, panorama, HDR pano, float DNG), masks + local adjustments (procedural, brush, range, refinement).
**ML (`crates/ml-*`)**: ml-runtime (ort + CoreML, registry, partition guard), ml-faces (YuNet + SFace), ml-quality (classical scores), ml-embed (SigLIP, HNSW, NL search), ml-segment (U²-Net, MobileSAM), ml-depth (Depth Anything V2 Small) + lens blur, ml-enhance (Real-ESRGAN super-resolution; denoise in progress). All weights Apache-2.0/MIT, fetched on demand, hashes pinned in `crates/ml-runtime/models.toml`.
**App (`apps/mac`, AppKit + SwiftUI over UniFFI)**: folder-first grid (60 fps at 20k), culling on the engine (groups, keep-best, defect sweep, compare, safe delete), Develop (Basic + tone curve, HSL, colour grading, detail, effects, crop, presets, snapshots, history; 6–12 ms slider path), masking UI (brush, gradients, ranges, AI masks, overlay), library UI (albums, smart albums, filter bar with facets, keywords, IPTC), Sparkle 2 auto-update (starts only when keys are configured), signed release pipeline + GitHub Actions CI/release.
**CLI (`apps/tessera-cli`)**: index, ls, cull, develop, render, preview, import lrcat, ml, export.

## Verified on screen by GPT-6 Sol (computer use)
M0-05 smoke (real engine), M1-09 culling (28/32, misses were verifier procedure), M1-10 develop (works; WB bug found → fixed in M2-04b).

## In progress
M3-05 AI denoise (relaunched with placement decision), M2-17 operator performance at full resolution, M2-13v on-screen verification of library + develop panels, M2-13c Swift strict-concurrency CI fix.

## Known gaps / next
- Adobe undocumented XMP structures (PointColors strings, brush dabs, Look tables) need **real Lightroom-exported sidecars** to finish (M2-02b).
- Export must install `MaskHooks` to render AI masks; local adjustments are not yet on the fast GPU path (M2-14 notes).
- Lens: downloadable lensfun data pack, DNG opcode execution in-stage, independent manual CA (M2-09b).
- Adobe PV6 compatibility renderer for imported edits + in-app Lightroom import with ΔE fidelity report (M2-18, M2-13b).
- Milestone 3 remainder: tool API/MCP server, scripting console, tethering. Milestone 4 agentic editing. Milestone 5 layered editor.
- Release: needs your Developer ID certificate + notarization profile and a Sparkle EdDSA key (see apps/mac/Support/release/README.md).
