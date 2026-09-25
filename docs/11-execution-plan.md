# Execution Plan — Multi-Model Build of the Spec Set

Written 2026-09-25. Covers how docs 01–10 get built, by which model, in what order, and how each piece is verified.

## 0. Environment (verified on this machine)

| Item | Value |
|---|---|
| Hardware | Apple M4, 24 GB, macOS 26.6.2, Xcode 26.6 |
| Toolchains | Rust 1.87 / cargo, Swift 6.3, Node 26, Python 3.13 |
| Claude Code | 2.1.282 — Fable 5.1 (this session), Opus 5.5 via the Agent tool |
| Codex CLI | 0.154.0 refused `gpt-6-luna`/`gpt-6-sol` on the ChatGPT account; **updating to 0.157.0 fixed it** (both answer). Kept as a fallback runner; Hermes remains the primary because its computer-use path is verified. |
| Hermes | 0.21.1 with an `openai-codex` OAuth credential that **does** reach `gpt-6-luna` and `gpt-6-sol` (smoke-tested 2026-09-25: Luna built and tested a cargo crate in 30 s; Sol took a screenshot via the `computer_use` toolset, cua-driver 0.25 with Accessibility + Screen Recording granted). **Decision: run Luna and Sol through Hermes.** |

## 1. Technical decisions (revised after the 2026-09-25 stack review)

1. **Engine in Rust**, Cargo workspace, one crate per module, `rust-toolchain.toml` pinned to current stable (the machine's 1.87 is below the minimum for `ort` 1.88, `lcms2`/`rawler` 1.89). Reasons unchanged: compiler + `cargo test` are a cheap oracle for Luna's retry loop, memory safety with 4–6 parallel authors, `wgpu` covers Metal now and WebGPU later.
2. **GPU compute via `wgpu` 30.x; presentation owned by Swift/AppKit.** Swift creates the `CAMetalLayer`, sets per-monitor colour space and EDR headroom; Rust renders into IOSurface-backed textures imported with `texture_from_raw` and synchronises with `MTLSharedEvent`. UniFFI carries commands and metadata only; pixels never cross the boundary as bytes. Milestone 0 includes a **wgpu-vs-MSL benchmark spike** (demosaic, guided filter, 3D LUT on the M4) plus an HDR-surface test; MSL passthrough is the fallback for any kernel that is too slow or must be exact.
3. **No "bit-for-bit" GPU/CPU claim.** wgpu's Metal path compiles with fast-math and WGSL permits FMA/reassociation, so spec 07 §1's "bit-identical on any GPU/CPU" is replaced by: deterministic per backend, and GPU output gated against the CPU reference at a tolerance (max abs error ≤ 1e-4 linear, ΔE2000 ≤ 0.5 on the golden suite).
4. **Cached intermediates may be f16.** A 45 MP RGBA f32 buffer is ~720 MB; memoising post-demosaic and post-denoise per image at f32 does not fit in 24 GB alongside prefetch. Compute in f32, store cached stage buffers as f16 (spec 07 §1's "no 16-bit intermediate" is relaxed for caches only, never for in-flight math).
5. **macOS app is AppKit where performance matters, SwiftUI elsewhere.** `NSCollectionView` for the grid (SwiftUI `LazyVGrid` does not recycle cells), an `NSView` with `CAMetalLayer` for the viewport, custom `NSControl` sliders on the < 16 ms path; SwiftUI for inspectors, panels, sheets, settings. This is what Pixelmator Pro and Nitro do.
6. **Headless first.** Every engine feature ships with the `tessera` CLI and, from Milestone 3, the MCP server (spec 10). Luna and Sol test through these before UI exists.
7. **Third-party, licence-checked.** `libraw-sys` is dead (2015); we write an in-repo `libraw-ffi` crate (bindgen over vendored LibRaw 0.22.1, CDDL option), with `rawler` (LGPL, dynamic link) as cross-check decoder. `rusqlite` bundled with FTS5. `ort` 2.0 RC with the CoreML provider plus a CI test that fails if any hot model has CPU-partitioned operators; native Core ML `.mlpackage` route kept for denoise and segmentation. `lcms2` for ICC. JPEG XL: `jxl-oxide` for decode, direct `libjxl` (BSD) bindings for encode; `jpegxl-rs` is GPL and is banned. Thumbnail tier of the preview cache is JPEG (`zune-jpeg`/ImageIO hardware decode); JXL only for large previews and export. `uniffi` 0.32 for Swift bindings. A `cargo-deny` licence gate runs in CI from Milestone 0.
8. **Fixtures**: CC0 sample raws from raw.pixls.us fetched by script into `fixtures/` (git-ignored); golden crops committed.
9. **Cross-platform UI is deferred, engine stays UI-agnostic.** gpui, Slint, iced, Tauri were reviewed and rejected for v1 (licensing, maturity, or no EDR Metal viewport path). Windows UI is decided when Windows is scheduled.

## 2. Model roles and the mechanical contract for each

| Model | Invoked as | Owns | Never does |
|---|---|---|---|
| **Fable 5.1** (coordinator) | this session | Work-package (WP) decomposition, interface contracts (crate APIs, JSON/XMP schemas, SQLite schema), the task board, merges to `main`, escalation decisions, golden-image sign-off | Leaf coding |
| **Opus 5.5** (design workhorse) | `Agent` tool, `model: "opus"`, `isolation: "worktree"` | Architecture packages (pipeline graph, tile/cache design, color-science operator design), SwiftUI app and culling UX, design review of any diff touching a public API or UI | Mechanical ports, parsers, fixture wrangling |
| **GPT-6 Luna** (executor) | `hermes -z "<brief>" --provider openai-codex -m gpt-6-luna --yolo --ignore-user-config --in <worktree>` | Every task with a mechanical oracle: codecs, parsers, `.lrcat` reader, SQLite index, XMP round-trip, CPU reference kernels, GPU kernel ports against the CPU reference, benchmarks, unit tests, scaffolding | Deciding design questions; touching files outside its WP allow-list |
| **GPT-6 Sol** (verifier) | `hermes -z "<acceptance>" --provider openai-codex -m gpt-6-sol -t computer_use --yolo --ignore-user-config`, one at a time; evidence saved with `screencapture -x` | Build + launch the mac app, drive it per the WP's acceptance script, screenshot, report pass/fail with evidence | Editing source |

**Luna loop** (scripted in `tools/orchestrate/run-luna.sh`): prompt = WP brief + interface contract + allowed paths + the test command. After the run, the script runs `cargo test -p <crate>` and `cargo clippy`. On failure it re-invokes Luna with the failure log appended, up to 3 attempts. After 3 failures the WP is flagged for Opus (design problem) or me (spec problem).

**Opus review gate**: any Luna diff touching `engine-api`, `recipe` schema, `index` schema, or the Swift app gets an Opus review pass before merge. Pure-internal crate changes merge on green tests.

**Sol verification** (`tools/orchestrate/verify-sol.sh`): each UI WP ships an `acceptance.md` with numbered steps and expected screen state. Sol executes it, writes `evidence/<wp>/*.png` and a JSON verdict. Sol takes the screen while running, so verification is serialized and batched at the end of each wave.

**Concurrency**: Opus 2–3 agents, Luna 4–6 codex processes, Sol 1. Each WP runs in its own git worktree on branch `wp/<id>`; I merge into `main` after the gate for that WP passes.

**Cost policy**: Luna first for anything with a test oracle, even if it fails twice. Opus first only when the task is under-specified, cross-cutting, or a matter of taste. Fable never codes leaves.

## 3. Repository layout (created in Milestone 0)

```
Cargo.toml                 workspace
crates/
  image-core/              tiles, pyramids, color (spec 04 §2)
  raw-decode/              LibRaw wrapper, embedded preview + EXIF extraction
  pipeline-cpu/            reference operators, golden tests (spec 07)
  pipeline-gpu/            wgpu kernels, bit-compared to pipeline-cpu
  recipe/                  recipe JSON, history, crs: XMP mapping (spec 05 §3.2, 06 §2.1)
  sidecar/                 XMP sidecar + .edits/*.json read/write, atomic rename
  index/                   SQLite WAL+FTS5+R*Tree index, folder scanner, library.json (spec 05 §2)
  previews/                content-addressed pyramid cache (spec 05 §2.2)
  import-lrcat/            read-only .lrcat → sidecars + library (spec 05 §3)
  cull/                    decisions/grades/marks, groups, signal store (spec 06)
  ml-runtime/              ort + CoreML, model registry (spec 09 §5)
  ml-*/                    faces, embeddings, segmentation, scores, denoise
  jobs/                    priority scheduler (spec 08 §2)
  engine-api/              typed tool API, UniFFI bindings, MCP server (spec 10)
  compositor/              layered editor (Milestone 5)
apps/
  tessera-cli/             headless driver used by tests and Sol
  mac/                     SwiftUI app
fixtures/                  fetched raws (ignored) + committed golden crops
tools/orchestrate/         run-luna.sh, verify-sol.sh, board.json, wp/<id>/{brief.md,acceptance.md}
```

## 4. Milestones and work packages

### M0 — Scaffold and harness (1 wave, ~1 day)
| WP | Model | Deliverable | Gate |
|---|---|---|---|
| M0-01 | Luna | Cargo workspace with empty crates above, `rust-toolchain.toml`, `deny.toml` + `cargo-deny` licence gate, CI script (`cargo test --workspace`, clippy, deny, `xcodebuild`), fixture fetch script, `.gitignore` | CI green on empty workspace |
| M0-02 | Luna | `tools/orchestrate/`: run-luna.sh, verify-sol.sh, board.json format, WP brief template | I dry-run both scripts |
| M0-03 | Opus | `engine-api` contracts: `Tile`, `Pyramid`, `Recipe` schema v1, `Job` trait, error types, the crs: key table as a Rust enum | I review; becomes the contract every Luna WP links against |
| M0-04 | Opus | Mac app shell: window, folder picker, `NSCollectionView` grid, `CAMetalLayer` loupe view with EDR/colour-space setup, filmstrip, keyboard map from spec 06 §3; SwiftUI for panels; wired to stub data | Builds; Sol smoke test (M0-05) |
| M0-05 | Sol | Prove the computer-use path: build M0-04, launch, open `fixtures/`, screenshot grid, report | Evidence PNG + verdict JSON |
| M0-06 | Luna | `libraw-ffi`: vendored LibRaw 0.22.1, bindgen, safe wrapper for open/unpack/raw CFA access/embedded preview; builds on macOS arm64 | Decodes the fixture set |
| M0-07 | Luna | GPU spike: wgpu 30 compute vs handwritten MSL for demosaic, guided filter, 3D LUT on the M4; HDR surface configure test; report ms/frame and max-abs-error | Go/no-go memo for wgpu, reviewed by Opus |

### M1 — Folder-first raw workflow, P0 of spec 01 §4 (3 waves)
Goal: open a folder of raws, see embedded previews in < 1 s, cull with X/U/P and grades, develop with the Basic panel, export JPEG, everything persisted to sidecars.

| WP | Model | Deliverable |
|---|---|---|
| M1-01 | Luna | `raw-decode`: LibRaw decode to linear CFA float tiles; embedded JPEG + EXIF extraction for CR3/ARW/NEF/RAF/DNG fixtures |
| M1-02 | Luna | `index`: schema from spec 05 §2, scanner, FTS5, incremental facets; 100k-file synthetic benchmark < 100 ms search |
| M1-03 | Luna | `sidecar` + `recipe`: XMP read/write, recipe JSON with append-only history and state hash; selection-model ↔ XMP mapping (spec 06 §2.1) with round-trip tests |
| M1-04 | Luna | `previews`: content-addressed JXL pyramid, LRU eviction, embedded-preview fast path |
| M1-05 | Luna | `pipeline-cpu`: linearize, black/white, WB (CAT16), demosaic (bilinear + RCD), dual-illuminant DCP matrix, Rec.2020 working space, Basic tone (exposure/contrast/highlights/shadows/whites/blacks), sigmoid display transform, sRGB encode; golden tests |
| M1-06 | Opus | Design of the pipeline graph and stage memoization keyed per spec 04 §3; `jobs` scheduler with priority classes (spec 08 §2). Opus writes the design + core; Luna fills operators |
| M1-07 | Luna | `pipeline-gpu`: wgpu ports of M1-05 operators, gated against the CPU reference at the §1.3 tolerance |
| M1-08 | Luna | Export: tile-parallel render, JPEG/TIFF/PNG encoders, XMP embed, batch over an album |
| M1-09 | Opus | Culling UX in the mac app: decision/grade/mark keys, group navigation, basket, safe delete (spec 06 §3–4) |
| M1-10 | Opus | Develop UI: Basic panel sliders bound to recipe with < 16 ms screen-res update via the memoized graph; histogram |
| M1-11 | Luna | `tessera`: `tessera index <dir>`, `tessera cull set`, `tessera develop set`, `tessera export`, `tessera render --stage` used by tests and Sol |
| M1-V | Sol | Acceptance: open 200-raw fixture folder, grid visible < 1 s, cull 20 images by keyboard, sidecars written, adjust exposure, export 10 JPEGs, verify files exist |

### M2 — Parity for enthusiasts, P1 (3 waves)
Tone curve, HSL in OkLCh, Color Grading, detail (sharpen/NR), lens corrections from embedded opcodes + lensfun, single composed geometry map, crop/Upright manual, geometric + range masks with local operators, presets/snapshots/history UI, albums/smart albums/album groups, soft proof with ICC, **`.lrcat` importer** with PV6 compatibility tone mapper and ΔE report. Split the same way: Opus for operator design (color science, mask compositing, geometry map), Luna for implementation and importer, Opus for panel UI, Sol for acceptance.

### M3 — On-device ML and the tool API, P2 (3 waves)
`ml-runtime` on `ort` + CoreML; faces (SCRFD + ArcFace + HDBSCAN), embeddings (SigLIP), duplicate/burst groups, quality scores, semantic and promptable segmentation → AI masks, joint denoise+demosaic net, depth. Engine tool API + MCP server (spec 10 phase 2), style-profile auto-edit (spec 10 phase 1). Luna does model integration and batching; Opus designs the mask system and the tool API surface; Sol verifies the face strip and defect sweep in the app.

### M4 — Agentic editing (2 waves)
Planner + critic loop over the MCP tool API (spec 10 phase 3), batch mode with consistency constraints, "Agent base edit" history group with fade slider, explainability strings. Opus designs the planner/critic protocol; Luna builds the harness and the metrics critic; Sol runs end-to-end shoots.

### M5 — Layered editor (spec 02, ongoing)
`compositor` crate: scene graph, blend modes, adjustment layers, masks, smart objects; brush engine; selections; PSD read/write. Same split. Starts after M2 so the raw app is usable while it's built.

## 5. First wave I will launch on approval (all parallel)

- Opus: M0-03 contracts, M0-04 app shell
- Luna: M0-01 scaffold, M0-02 orchestration scripts, M0-06 libraw-ffi, M0-07 GPU spike, then M1-01, M1-02, M1-03, M1-05 (they depend only on the contracts, which I will stub for them and reconcile at merge)
- Sol: M0-05 as soon as M0-04 builds
- Me: task board, merge queue, contract reconciliation, memory notes

## 6. Risks and how they are handled

- **Workers run unsandboxed on the host** (Hermes `--yolo`). Mitigation: path allow-lists enforced by the merge script, worktrees, no secrets in the repo; a separate macOS user account is the next step if this becomes a problem.
- **Sol computer use takes the screen.** Verification runs in batches at wave end; you will see the app being driven. If that is not acceptable on this machine, Sol runs headless `tessera` acceptance only and UI checks fall to Opus reading screenshots from `xcrun` captures.
- **Luna drift across worktrees.** Each WP has an explicit path allow-list; the merge script rejects diffs outside it.
- **Contract churn.** Contracts are versioned in `engine-api`; Luna WPs pin the version they built against; I own reconciliation.
- **LibRaw coverage gaps** (CR3 etc.). Fallback to `rawloader`/`rawspeed` bindings per-format is a Luna task, not a redesign.
- **Scope.** Docs 01+02 describe two flagship products. This plan reaches a usable raw workflow app at M2 and defers the layered editor to M5 deliberately.
