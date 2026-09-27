# M5-31 recovered integration review

Reviewed 2026-09-27, read-only at `953204e` in `/Users/rutmehta/Developer/tessera/.worktrees/M5-31`, comparing against main `c0d4535`. This is the recovered `8c8c130` round-two implementation plus current main. No source edits, builds or GPU tests were performed during this review. Read brief, ROUND2, COMPOSITOR changes, affected implementation and tests. The parent's focused panorama/deadlock log was inspected.

## Decision

**Publishable as a provisional pixel-layer performance test candidate**, with exact SHA and explicit known correctness gaps below; **not merge-ready or correctness-accepted**. Machine B can safely run the existing bounded sparse five-layer timing fixture to learn whether the M4 Max meets the unchanged threshold. Prefer fixing and gating the narrow live-content integration defects before treating any result as final acceptance of the merge candidate. Do not use the historical top-level pass verdict or select only passing timing samples.

## Prioritized findings

### P1 — Styled live text bypasses the explicit font snapshot; font replacement reuses stale style planes

`crates/compositor/src/resident/styles_runtime.rs:207–215` constructs the source child with `Self::with_budget`, forwarding smart quality and filter evaluator only. It never forwards the parent's explicit `TextRenderer`. `render/live.rs:232–235` then creates a different renderer and discovers system fonts. Adding any style to text (or styling an isolated group containing text) can change its font, fail for a caller-supplied non-system font, or render differently across machines. This contradicts M5-30's explicit-font contract and CPU style sources, which reuse the caller's compositor.

Additionally, `resident/mod.rs:624–627` only updates `self.live` and invalidates rendered levels. It does not clear `self.styles`; the new style key at `styles_runtime.rs:176–189` contains no font identity. Even once forwarding is fixed, replacing fonts can reuse source/effect buffers rendered with the old font database. Nested style/source children must inherit the same policy and be invalidated too. Existing smart child creation in `filters.rs:308–313` also omits font propagation; account for this when styled text lives inside a smart source.

**Required regression:** use the repo's explicit Noto font fixture without relying on system discovery; compare CPU/resident styled direct text and an isolated styled group containing text at L0/L1/L2. Replace the explicit renderer with an empty/different font database without editing the document; render must honor the replacement rather than hit old style buffers. Restore fonts and verify clean parity. Existing `live_text_resident` exercises unstyled text and therefore misses the new path.

### P2 — Styled live geometry retains native-depth quantization while the CPU source is F32

`render/effects.rs:270` sets the neutralized style source's state depth to F32. The resident source at `styles_runtime.rs:203–210` retains native depth and only sets `float_adjustments = true`. That flag controls adjustment clamping at `resident/mod.rs:1454`; it does not change live geometry rasterization. `resolve` supplies the native depth to `live_tile` (`resident/mod.rs:906–910`), which quantizes through `tile_from_normalized(depth)` (`render/live.rs:373–381`). Thus U8 live shape/text antialias/color samples are rounded before effects, whereas the CPU style source uses F32. The potential difference is roughly 1/255, exceeding the documented 1e-4 composite tolerance.

**Required regression:** a real `LayerKind::Shape` with fractional bounds/transform and non-byte-exact color/alpha, styled with an overlay or glow, in U8 (also U16/F32 controls), CPU versus resident at L0/L1/L2, interpreter and specialized. Current style helpers named `shape` create `LayerKind::Pixel` fixtures, so existing U8 parity does not establish live-geometry parity. This is a high-confidence source-trace finding; no failing GPU run was performed in this review. Fix only after observing the focused regression.

### P2 — Aggregate plane limit is checked after one entire style stack has allocated its GPU planes

`styles_runtime.rs:230–249` calls `StylesGpu::render_at` first and only then checks `reserve_plane_words`. `styles_gpu.rs:177–365` accumulates all returned planes; each `emit` allocates an RGBA buffer. Style validation permits 64 effects (`render/styles.rs:498`). Bevel can emit four planes, so 64 enabled Emboss/Pillow bevel effects on a 1368×912 region can allocate roughly 5 GiB of output planes (plus source/intermediates) before the single auxiliary-binding limit rejects the result. Per-buffer `check_size` cannot catch the aggregate. The round-two cumulative fix correctly checks before retaining a *subsequent layer's* stack, but does not prevent oversized allocation within one stack.

**Fix:** preflight source bytes, exact enabled plane expansion count (including bevel variants), metadata and cumulative auxiliary bytes before invoking the effect renderer, or enforce a remaining-byte allowance inside plane emission. Return ResourceExhausted before generating buffers that cannot be used. A pure limit/count test can cover many enabled bevels without attempting a huge GPU allocation. This does not invalidate the bounded five-style benchmark, but the claim of early aggregate allocation validation is incomplete.

## Main preservation

The merge diff against `c0d4535` is confined to 22 M5-31 files: styles implementation/wiring/tests and documentation. The only recorded merge conflict was resident/mod.rs. Its resolved result retains M5-30's `live` field, explicit text-renderer API and live/vector page resolution; M5-32 adjustment source/shader files are unchanged from main; M5-35 `render/smart_filters.rs` is unchanged. No document/edit/format/PSD changes were introduced by this package. `git diff --check c0d4535 HEAD` passed.

The retained APIs are not enough to prove integration correctness: the new style child bypasses the live-font/precision contracts as described above. The parent's fresh `/tmp/tessera-m531-recovery-focused.log` confirms panorama 1/1 and smart_filter_deadlock 1/1 passed after integration. That addresses the inherited hang; it does not replace the current style/live GPU suite.

## Cache and memory observations

CPU pixel caches use the existing byte-budgeted RenderCache, with namespace identities for source/planes. The global 1024-entry namespace registry holds no pixels; globally unique IDs make eviction sacrifice reuse rather than collide. Cached descriptor lookup checks that every plane/source tile remains present, correctly treating partial eviction as a miss. Revisions, style settings, global light, canvas/depth and requested level/tile participate in identity. Document revisions are globally monotonic, so omitting history epoch from CPU style namespaces is not itself a stale-undo bug.

Resident final source/effect cache has LRU eviction bounded by `self.budget`, and oversized entries are not retained. It is not a bound on total peak GPU memory: auxiliary copies, output/page pools, pending Arc handles, effect intermediates and temporary child renderer allocations coexist. The existing page budget was already soft; do not interpret `style_cache_bytes() <= budget` as process/device memory proof. The per-stack allocation finding is separate from this expected accounting distinction.

Level-local evaluation with scaled morphology is an intentional documented change from styling L0 then reducing. Region reference tests compare against a whole canvas evaluated at the same requested level. Real halo fetching, native fill coordinates and partial-cache handling have useful coverage. Nonlinear level differences are not a newly discovered defect.

## Necessary validation after fixes

1. Observe the focused styled explicit-font and true live-shape U8 regressions failing first, then pass them after the narrow integration fixes. Include font reset, nested isolated group and smart-source cases, L0/L1/L2, and interpreter/specialization. No compiling/GPU work until the coordinator grants the shared build slot.
2. Rerun `live_text_resident`, `live_render`, `live_style_damage`, `resident_styles`, `resident_styles_cache`, `resident_styles_semantics`, `resident_styles_validation`, `resident_filter_inventory`, and the nonignored `resident_styles_large` tests on the resolved tree. Ensure per-effect GPU unit tests run as well. Then the package-required full compositor release test/clippy/fmt gate, once, without concurrent GPU/build load.
3. Add a cheap plane-budget preflight test (source + metadata + multi-plane bevel expansion, exact binding boundary, overflow); do not test failure by allocating gigabytes.
4. On Machine B's M4 Max, run `cargo test -p compositor --release --test resident_styles_large twenty_mp_five_styles_1368x912_l1_timing -- --ignored --nocapture` in multiple fresh processes with no other build/GPU workloads. Record all cold and dispatched-warm samples, hardware, exact SHA and commands. Preserve CPU <2 s and resident <100 ms assertions. The warm run correctly invalidates output and asserts nonzero dispatch and plane reuse; the older 4K/20-layer benchmark's idle warm call is not a recomposite measurement.
5. Keep 20/50 MP real viewport correctness at L0/L1/L2 and same-level crop parity in final evidence. The large test currently verifies successful dispatch/cache bounds, not pixel parity on its large fixture; crop parity comes from separate smaller fixtures. Full-L0 single-buffer 50 MP output remains subject to documented device limits.

## Outstanding acceptance

Fresh full postmerge style/live GPU verification is pending. Cold resident timing is unresolved: ROUND2 honestly records both passes and 110.561–253.233 ms failures on Apple M4. Neither historical correctness gates nor the requested M4 Max remeasurement permits a current main merge or a claim of full acceptance. Styles on adjustment/pass-through groups, M5-14 contour/jitter/texture placeholders and pathological halo limits are explicitly documented restrictions rather than hidden fallbacks.

## Authorized repair and validation follow-up — 2026-09-27

After the read-only review, the parent authorized narrow implementation in the
same worktree and granted the exclusive build slot. All three findings now have
observed failing regression evidence and corresponding fixes. Explicit font
forwarding also exposed and fixed an obsolete unfiltered smart-child L0 styling
route; direct/group fonts passed before the smart-child routing fix. Generated
vector mask precision was covered alongside real Shape precision, preserving
native raster mips. See branch note
`tools/orchestrate/wp/M5-31/INTEGRATION-2026-09-27.md` for detailed disposition,
commands, local logs and the intentionally authorized `render/live.rs` helper.

Fresh full compositor release gate: 323 passed, 0 failed, 13 ignored, exit 0.
Focused live integration: 5 passed; styles unit/kernel/viewport: 24 passed.
No pending timing acceptance was run or waived. The branch is a repaired,
provisional Machine B timing candidate, not READY for main solely from these
correctness results. Final clippy/fmt and commit details follow in the handoff.

Strict all-target clippy and final workspace fmt both completed with exit 0.
The only post-release-gate change to a test replaced a mutable reference to
`usize::MAX` with an equivalent local variable to satisfy clippy; no production
code changed. The heavy slot was released to the parent before commit bookkeeping.

Final scoped commit: `9f922bf928dc02c42c5d4788db6c0e81245ccbe0` on `wp/M5-31`.
Tracked working tree is clean after commit; inherited untracked round-two logs
remain untouched. No push, main merge, or timing run was performed.
