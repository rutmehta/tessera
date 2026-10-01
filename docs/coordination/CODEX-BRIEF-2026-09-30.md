# Codex brief — Lightroom Classic edit translation (LR-1 … LR-7)

Owner: Codex coordinator on Machine A (resumed). Issued by the Claude Machine A coordinator on 2026-09-30 at Rut's request. Rut's subscription is expiring: run as many parallel workers as the machine allows and keep going until every lane below is merged or blocked.

## Goal

Everything in a Lightroom Classic catalog's develop settings converts into Tessera's recipe format and renders. No "retained as opaque source" categories remain for: AI/parametric masks (`MaskGroupBasedCorrections`, including `Mask/Image`), Lens Blur, healing/retouch spots (`RetouchAreas`, `RetouchInfo`), Point Color, `ExtendedToneCurvePV2012*`, Upright, `EnableDistractionRemoval`, legacy PV2010 sliders, and any other key the import currently reports as unknown or unsupported. Rut explicitly wants full fidelity here; the minimal-complexity rule yields to that for this brief, but still prefer the smallest correct mapping onto EXISTING recipe types over new ones.

## Ground rules

- Base on current `main` (≥ `10f0a3f7`). One branch per lane: `codex/lr-<n>-<slug>`. Never push to `main`; the Claude coordinator merges after independent review and gates.
- Tests first: a RED commit with failing tests, then the implementation. Fixtures are SYNTHETIC. Never commit anything derived from Rut's catalog (paths, names, literal rows). Rut's catalog copy lives on Machine B only; ask the coordinator for aggregate counts if needed.
- Coordinate through the existing Git mailbox (`tools/coordination/mailbox.py`, machine A → the coordinator reads `codex/coordination-a` and B's `codex/coordination-b`) AND write a `tools/orchestrate/wp/LR-<n>/HANDOFF.md` per lane with exact hashes, RED/GREEN evidence, gates, and limitations. The Claude coordinator polls both.
- Compiler lane: `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/codex-lr CARGO_BUILD_JOBS=2 RAYON_NUM_THREADS=2 TMPDIR=/Volumes/betterSSD/tmp/`. Keep everything on betterSSD; the startup disk was full once. Do not run Swift gates (the Claude coordinator runs them at merge). Do not launch the app or GUI tests.
- B (Machine B) owns `crates/import-lrcat` performance (B5-29c) and keeps recipe output byte-identical for existing inputs. Your lanes ADD translation in the codec/recipe layer and in `import-lrcat` mapping tables; coordinate on `lua_develop.rs` / `xmp.rs` edits through the coordinator to avoid conflicts (small, additive hunks only; rebase-free merges of main).
- Each lane ends with: unit tests, a synthetic end-to-end import test proving the recipe field is populated and renders (CPU path), clippy `-D warnings`, fmt, and the HANDOFF.

## Source contract (from B5-29c, Machine B)

For every Adobe key that the current decoder does not translate, `import-lrcat` retains the exact source value UNCONDITIONALLY in `recipe.unknown["lrcat_develop_source"]`, a map keyed by the Adobe key (Lua rows: the exact Lua value; XMP rows: the exact XMP fragment). Your lanes read from there and translate into recipe fields; when a lane lands, its key moves from "retained" to "translated" in the matrix. Until 29c's fix lands, treat the key name as provisional and confirm against `crates/import-lrcat/README.md` on main.

## Lanes (parallelizable; dependency noted)

LR-0 Inventory and plan (first, source-only, 1 worker): for every key the import reports unknown/unsupported, map it to an existing recipe field (`crates/engine-api/src/recipe/`), or state what is missing. Produce `docs/coordination/LR-TRANSLATION-MATRIX.md`: Adobe key → recipe path → lane → status. Reviewed by the coordinator before LR-1+ start implementation (they may start RED tests in parallel).

LR-1 Point Color (`PointColors` → `/settings/color/point_colors`): per-point hue/sat/lum with range/feather. Render parity test vs a hand-computed reference on a synthetic swatch.

LR-2 Extended tone curves (`ExtendedToneCurvePV2012`, `+Red/Green/Blue`), `ConvertToGrayscale` (+ `GrayMixer*`; present on 21,615 of 21,656 real images, so treat as high priority), `AutoToneDigest*`, `DepthMapInfo`, and legacy PV2010 sliders (`Exposure`/`Brightness`/`Contrast` PV2010 forms on 21,239 real images) → existing tone-curve fields; document PV2010 → PV2012 conversion where Adobe's semantics differ; report what cannot be represented.

LR-3 Retouch (`RetouchAreas`, `RetouchInfo`: heal/clone spots with source offsets, feather, opacity; brush strokes) → `/settings/locals/retouch`. Render through the existing heal/clone operators. Spots must land within 1 px of Adobe's normalized coordinates.

LR-4 Parametric masks (`MaskGroupBasedCorrections` with gradient/radial/brush/range masks, mask groups, add/subtract/intersect, per-group local adjustments) → `/settings/locals/adjustments` (44 groups already translate; cover the rest, including luminance/color range masks and depth range).

LR-5 AI masks (`Mask/Image`, subject/sky/background/people/objects): determine where LrC stores the mask rasters (catalog helper `.lrdata` / `Masks` tables) and import them as mask rasters into `mask-store` when present; when absent, regenerate via `ml-segment`/`mask-ai` with the same category and record "regenerated" in the recipe diagnostics. Depends on LR-4 for group semantics.

LR-6 Lens Blur (`LensBlur`: amount, focal range, bokeh, depth source) → `/settings/effects/lens_blur`, using `ml-depth` for the depth map when Adobe's isn't available; record "regenerated depth".

LR-7 Upright / geometry (`Upright*`, `UprightFourSegments*`, `UprightTransform*`, perspective sliders) → the existing transform/geometry recipe fields; `EnableDistractionRemoval` and other cloud-only features → explicit unsupported diagnostics with a user-facing explanation (not silent).

## Acceptance (per lane)

- RED commit shows the failing test(s); GREEN shows them passing; no test weakened.
- Synthetic import fixture round-trips: Lua row → recipe → render (CPU) → expected pixels within stated tolerance.
- `cargo test -p import-lrcat -p engine-api -p <touched crates>`, clippy, fmt green.
- HANDOFF lists exact hashes, measured numbers, limitations, and what Machine B must know for 29c compatibility.

Report lane completions on the mailbox with the branch hash. The coordinator reviews independently and merges in dependency order (LR-0 → LR-1/2/3/7 → LR-4 → LR-5/6).

## Phase 2 (after the LR lanes, or in parallel if workers are free): engine performance lanes

Source: Machine B's release-build profiling report, `origin/wp/B5-prof` (`tools/orchestrate/wp/B5-prof/REPORT.md`), measured under load (diagnostic, not baselines). Same ground rules as above; branches `codex/perf-<n>-<slug>`; every lane needs a reproducible benchmark test (ignored, release) with before/after numbers in HANDOFF, and a correctness golden test proving identical output.

PERF-1 Layer-style export (P15): a styled 14 MP Export Flat takes 83 s. Cache style effect planes by layer revision and avoid re-evaluating repeated blurs. Target 5–10×, identical pixels.

PERF-2 Camera Raw global preview (P19 + P11): the 100%-zoom global-settings preview takes 10.2 s and multi-GB. Cache compatible intermediates (demosaic/colour stages that don't depend on the changed setting) across preview submits; bound memory explicitly. Target 2–5×.

PERF-3 Vector drag (P13 + P19): 152 ms per frame at 20 MP with 4.9 GiB peak RSS. Reuse vector tiles, dirty regions and resident buffers across drag frames. Target < 80 ms and a bounded RSS.

PERF-4 Gaussian apply (P19): 723 ms at 24 MP. SIMD/reusable-buffer separable CPU path or exact-parity GPU separable path. Target 2–4×, pixel-identical within 1/65535.

PERF-5 Memory ceilings: Camera Raw preview peaks at 7 GiB and the Export Flat UI suite at 7.5 GiB. Identify the dominant allocations and add explicit bounds (reuse or streaming), with counting-allocator tests.

Machine B keeps the app-side lanes (P16/P20 export main-thread spans, B5-33). Coordinate any shared file (tessera-ffi) through the coordinator.
