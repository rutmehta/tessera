# Tessera — Build Status (2026-09-25)

Auto-maintained by the coordinator. Board: `python3 tools/orchestrate/board.py list`.

## Merged and verified
**Engine (Rust, `crates/`)**: engine-api contracts 1.1 (recipe/history/crs table/tool API), libraw-ffi + raw-decode (CR3/ARW/NEF/RAF/DNG), pipeline-cpu (M1+M2 operators, native revision 2, goldens), pipeline-gpu (wgpu ports, GPU-resident render path: tone 4 ms / WB 7–23 ms at L2 on a 36 MP NEF), image-core (tiled progressive renderer, stage memoisation, f16 cache, mask hooks), jobs scheduler, index (SQLite/FTS5, <10 ms search at 100k), sidecar (XMP + recipe JSON, native round trip), previews (JPEG pyramid, rendered fallback), export (JPEG/PNG/TIFF, ICC, XMP, batch, upscale hook), import-lrcat (read-only .lrcat → plan), library (albums, groups, smart albums, saved-search grammar), cull (decisions, groups, dHash, derived status, defect sweep, learning, face strip), lens (Brown–Conrady, lensfun/LCP, auto-calibration, Upright, composed geometry), color-mgmt (ICC registry, display/proof transforms, gamut warnings, GPU LUT), merge (HDR deghost, panorama, HDR pano, float DNG), masks + local adjustments (procedural, brush, range, refinement).
**ML (`crates/ml-*`)**: ml-runtime (ort + CoreML, registry, partition guard), ml-faces (YuNet + SFace), ml-quality (classical scores), ml-embed (SigLIP, HNSW, NL search), ml-segment (U²-Net, MobileSAM), ml-depth (Depth Anything V2 Small) + lens blur, ml-enhance (Real-ESRGAN super-resolution; denoise in progress). All weights Apache-2.0/MIT, fetched on demand, hashes pinned in `crates/ml-runtime/models.toml`.
**App (`apps/mac`, AppKit + SwiftUI over UniFFI)**: folder-first grid (60 fps at 20k), culling on the engine (groups, keep-best, defect sweep, compare, safe delete), Develop (Basic + tone curve, HSL, colour grading, detail, effects, crop, presets, snapshots, history; 6–12 ms slider path), masking UI (brush, gradients, ranges, AI masks, overlay), library UI (albums, smart albums, filter bar with facets, keywords, IPTC), Sparkle 2 auto-update (starts only when keys are configured), signed release pipeline + GitHub Actions CI/release.
**CLI (`apps/tessera-cli`)**: index, ls, cull, develop, render, preview, import lrcat, ml, export.

## Verified on screen by GPT-6 Sol (computer use)
M0-05 smoke (real engine), M1-09 culling (28/32, misses were verifier procedure), M1-10 develop (works; WB bug found → fixed in M2-04b).

## Since the last update (all merged)
Masks and local adjustments + masking UI (verified on screen with the real subject model), soft proofing and ICC display path, lens corrections + Upright, HDR/panorama merge, AI denoise (DRUNet) + super-resolution (Real-ESRGAN), depth + lens blur, assisted culling that learns, MCP tool server (`tessera-mcp`), library UI, Develop panels UI (tone curve, HSL, grading, detail, effects, crop, presets, history), Lightroom import flow in the app (verified on screen), Adobe PV6 compatibility renderer wired into the graph, export dialog + Print + soft proof, style-profile auto-edit (agentic phase 1), agent planner/critic loop with Anthropic/OpenAI/Ollama providers (phase 3), performance redesign (every slider < 12 ms at screen level; Texture/Clarity/Dehaze 230 ms → < 10 ms), Sparkle auto-update with CI/release workflows, app-dir isolation and `tessera index prune`.

## Since the previous update (all merged)
Design system + visual overhaul (Theme tokens, light/dark, lint test), HDR/EDR presentation (float surfaces, SDR bit-identical), GPU export without viewport starvation (36 MP full-size export 51 s → ~4 s; GPU lens/geometry), accessibility identifiers on every control, brush-erase modifier fix and two-pass SAM mask selection, importer rating→grade mapping, app-dir isolation + `tessera index prune`, Adobe compat op set wired into the renderer, agent planner/critic (Anthropic/OpenAI/Ollama providers), style-profile auto-edit.

## Latest (merged)
Export throughput (36 MP full-size 1.2 s; 100 Web JPEGs 26 s render+encode; app exports develop at full resolution for exactness), lens follow-ups (lensfun data pack download with attribution, DNG opcodes in-stage, independent manual CA, GPU CA batching), assisted culling + agent review UI (face strip, Keep?/Reject? suggestions with confirm, Auto Edit sheet, review queue, "Agent base edit" history group with amount fader and per-step toggles, Settings ▸ AI with Keychain keys).

## Also merged since
Tethered capture + live ingest (ImageCaptureCore; no camera here to verify physically), keywords/captions/OCR engine (SigLIP zero-shot over 2,080 concepts, Florence-2 captions + OCR) and the app UI for them, agent runs on the GPU renderer (~50 ms/step vs 40 s/image), slider keyboard focus fix, Develop panels verified on screen.

## Milestone 5 (layered editor) so far — merged
Compositor core (27 blend modes exact, groups/knockout/Blend If, adjustments, smart objects, COW tiles, dirty rects, non-linear history, .tessera-doc), PSD/PSB read/write + adapter, engine-api 1.2 (document/layer ids, node memo key, document tool calls, history, action descriptors), GPU-resident compositing via a shared `gpu-core` device (dab 1.6 ms, L2 13.6 ms; L0 198 ms vs 100 target), filters crate (blurs/sharpen/noise/distort/stylize/adjust with Metal parity), exact critic metrics, tethering UI, keywords/captions UI.

## Also merged (2026-09-26)
Brush engine (dynamics, ABR import, heal/clone/patch, GPU dabs) + selection tools (all shapes, refine edge, contours, alpha channels), document tool executor with 32 MCP tools and Actions record/replay, brush/selection wired into it, self-supervised CFA denoise network (training + export scripts; weights local), incremental library updates (change feed; tether/import/rescan update in place), assisted culling + agent UI verified on screen.

## Also merged (2026-09-26, later)
Compositor bit-exact GPU maths and L0 < 100 ms (92 ms; 4K viewport 38 ms, full recomposite under 8 ms shown infeasible on M4), text layers (`typography`), vector shapes (`vector`), CFA denoise GPU-resident handoff + restored training prerequisites (6 dB gain), performance benchmark harness with CI workflow, public-repo docs (README/CONTRIBUTING/ARCHITECTURE).

## In progress / next
- M5-14 layer styles + smart filters, M2-29 develop JPEG/TIFF/HEIC, M3-19 people clustering (Astra); M2-25v2 regression pass (Sol).
- Machine B owns the layered-editor UI (M5-09). Coordinator fallback: `tools/orchestrate/supervise.sh` (Astra 900k via Hermes) if the Fable session is downgraded.
- Needs from the owner: real Lightroom-written XMP sidecars in `fixtures/lightroom/` to finish Adobe-specific structures (M2-02b).

## Known gaps / next
- Adobe undocumented XMP structures (PointColors strings, brush dabs, Look tables) need **real Lightroom-exported sidecars** to finish (M2-02b).
- Export must install `MaskHooks` to render AI masks; local adjustments are not yet on the fast GPU path (M2-14 notes).
- Lens: downloadable lensfun data pack, DNG opcode execution in-stage, independent manual CA (M2-09b).
- Adobe PV6 compatibility renderer for imported edits + in-app Lightroom import with ΔE fidelity report (M2-18, M2-13b).
- Milestone 3 remainder: tool API/MCP server, scripting console, tethering. Milestone 4 agentic editing. Milestone 5 layered editor.
- Release: needs your Developer ID certificate + notarization profile and a Sparkle EdDSA key (see apps/mac/Support/release/README.md).

## Coordination across machines (2026-09-26)
Two coordinator sessions run in parallel on two laptops sharing this repo. Ownership by package id prefix:
- **Machine A (this Mac, engine + Astra-heavy):** M5-08 compositor perf, M5-07 document executor, M2-28 incremental updates, and subsequent engine crates (denoise/CFA net, PV6 fidelity with real samples, tethering backends, MCP/agent).
- **Machine B (second laptop):** the **layered-editor UI** (`M5-09`: Document window over the resident compositor, layers panel, tools palette wired to brush/selection/filters, history, PSD open/save; Opus), and app polish packages (`M2-3x`). Use ids `M5-09`–`M5-19` and `M2-30`–`M2-39` to avoid collisions.
Rules: each session creates `wp/<id>` branches in its own worktrees, keeps `CARGO_TARGET_DIR` outside the repo, never edits `engine-api` without a contract package, pushes `wp/*` branches, and **only Machine A merges to `main`** (Machine B opens its branches and notes them in `tools/orchestrate/board.json` under its own ids; Machine A merges and pushes). Rebase or merge `main` into a branch before requesting a merge.
