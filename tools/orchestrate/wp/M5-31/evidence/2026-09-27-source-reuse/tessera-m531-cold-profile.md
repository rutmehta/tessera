# M5-31 cold-start diagnostic, 2026-09-27

Candidate: 9f922bf928dc02c42c5d4788db6c0e81245ccbe0. Acceptance remains blocked: Machine B M4 Max cold samples 125.855125 / 43.970875 / 33.298750 ms against unchanged <100 ms threshold. Warm samples 3.959416 / 5.281167 / 4.106625 ms. Original measurement includes render_viewport and wait, including lazy style pipelines, uploads, conversions, effects, auxiliary assembly and final dispatch. No required work was moved outside that interval.

B evidence: origin/wp/B5-16:tools/orchestrate/wp/B5-16/evidence/M5-31-timing-9f922bf/. Each host snapshot shows a yes process at roughly 100% CPU and significant PerfPowerServices/sysmond/storage/WindowServer activity. This is a confound, not a reason to discard the failed sample. The source of that specific spike has not been proved.

## Instrumentation and local observations

Exact diagnostic delta: /tmp/tessera-m531-stage-profile.patch (v2), touching precise.rs, styles_runtime.rs, resident_styles_large.rs. Metadata/commands/exits: /tmp/tessera-m531-stage-metadata.json. Apple M4, 24 GiB; external release target /Volumes/betterSSD/tessera-cache/target/M5-31.

Three bounded unsynchronized fresh-process runs: /tmp/tessera-m531-stage-{2,3,4}.log, with corresponding -host.txt snapshots. Cold 46.771959 / 48.272833 / 52.155500 ms, encode 15.703750 / 15.696291 / 14.744167 ms, final wait 31.068209 / 32.576542 / 37.411333 ms. Warm 9.215916 / 9.787667 / 9.754167 ms. These instrumented local results do not establish M4 Max acceptance.

Pipeline creation inside cold is small in these samples: source-conversion plumbing approximately 1.05–1.14 ms; style pipeline approximately 0.75–0.79 ms. First source encode 4.98–5.43 ms, uploading 30 pages / 31,457,280 bytes, 12 mip pages, 5,280 blocks. Four later neutralized source renders dispatch zero blocks and upload nothing, yet each still allocates/converts a separate 21,626,880-byte straight source. Final auxiliary allocation is 237,896,800 bytes (five source copies, six effect planes, metadata).

Synchronized diagnostic /tmp/tessera-m531-stage-sync.log changes queue scheduling by waiting between stages; do not treat as acceptance or additive unbiased stage timing. It shows first source wait 11.251875 ms; five conversion waits total about 11.51 ms, effects waits about 24.75 ms, final wait 19.732166 ms. Whole cold 80.501708 ms. This supports investigating redundant GPU conversion/copies, not claiming shader compilation explains the B failure.

CONTAMINATED: /tmp/tessera-m531-stage-1.log and exact /tmp/tessera-m531-stage-profile-v1.patch printed the full frame report with 5,280 damage rectangles. Stderr overhead inflated cold to 233.125 ms. Preserve as instrumentation evidence; exclude from performance conclusions. V2 logs scalar counters only.

## Proposed narrow production change (awaiting parent review)

Keep one previous conversion inside a single prepare_styles batch. After successful child.render_viewport, reuse the immutable converted buffer only if report.blocks == 0, level unchanged, exact child output Buffer identity unchanged, and actual child region unchanged. This does not use source raster pointer, layer ID, equal dimensions, or a new incomplete content hash.

Proof from resident/mod.rs: materialization changes page pools; output overlap copies at lines 1325–1377 target newly allocated output buffers; existing output is written only within nblocks > 0 at lines 1436–1558. The idle path writes no output. Existing program/node/damage validity therefore checks masks, content, transforms, nested child revisions and font-derived nodes before zero dispatched blocks can permit reuse. Child font snapshot, evaluator, smart quality and float intermediate settings remain fixed inside the batch. Region/buffer replacement or any dispatch forces conversion. A batch-local tuple cannot survive font resets, document edits, epoch changes, or later render calls.

Deduplicate source reservations/aux copies by exact immutable converted Buffer identity only. wgpu 30 Buffer Eq/Hash delegates to inner backend identity (api/buffer.rs:232), so no custom content key or Arc wrapper is necessary. Plane expansion/metadata remain per effect; preserve overflow/preflight rejection before conversion/effect allocations. Keep conservative per-entry retained cache accounting; optimizing that is unnecessary to this slice. Expected fixture auxiliary storage falls by four source buffers (~86.5 MB), without removing work from the timed interval.

Tests before implementation: exact duplicate source aliases one auxiliary source while distinct source/mask/child/transform inputs remain separate; verify CPU parity after mutation and at L0/L1/L2/viewport changes; rerun explicit-font/cache tests and preflight tests. Keep the existing cold and warm real-dispatch assertions and five-evaluation cache assertion unchanged. Full compositor release, strict all-target clippy, and fmt after focused green. B must still rerun all cold samples on the new candidate.

## Meaningful-document guard discovery

The initial three-source sharing regression failed as expected (six copies vs four): `/tmp/tessera-m531-reuse-red.log`. A subsequent guard found an important benchmark fixture flaw: `document()` creates all root layers with ID zero and directly wraps DocState, while Document::new does not assign IDs. Different effects happened to keep style cache keys distinct. With valid unique layer IDs, the zero-dispatch reuse remained conservative and did not trigger: each source dispatched 12 blocks. Evidence `/tmp/tessera-m531-reuse-focused-2.log` and `/tmp/tessera-m531-reuse-diagnose.log` (temporary scalar trace now removed).

Cause: resident/program.rs Step::set copies the layer-derived Params.seed even for Normal blending, so Program.bytes differs despite identical source pixels. Seed is consumed only by Dissolve: blend.wgsl composite_core mode==1 and resident/doc.wgsl adjustment h.y==1. Other uses forward the seed. CPU Params is unchanged. Parent approved narrow semantic canonicalization: preserve seed verbatim for Dissolve, use zero for other resident Step modes. This changes neither document IDs nor step layout and makes existing program/damage/specialization identities reflect actual shader dependencies. Nested and adjustment Dissolve must retain identity and pixel parity.

Test fixture corrections are not production findings: the first across-call assertion ignored the renderer's initial reset; distinct-input fixtures initially had invalid duplicate IDs; cloned smart fixtures initially replaced child contents without changing child revision. Fixed tests assign IDs and real child document revisions and perform later changes through SetMask/EditSmartObject. All intermediate failures/logs retained. No parity tolerance was relaxed.

Pending gates after UI releases Machine A heavy slot: unique-ID sharing, different-source/mask/child/transform/nonalias guards at L0/L1/L2, nested Dissolve parity, actual same-document edits, explicit-font regressions, allocation preflight, then original timing three fresh processes AND new separate unique-ID timing companion three fresh processes. Original timing test and thresholds are unchanged. Full release/clippy/fmt remain pending on this follow-up delta; no completion/performance claim yet.

## Coordinator gate continuation

First frozen-source focused gate failed: 5 pass, 1 fail, before any timing run.
Failure retained at /tmp/tessera-m531-final-focused-fixture-failure.log:
case3 L0 sample1292 resident0.85142857 versus CPU0.68. The valid unique-ID
source-sharing regression passed. Investigation found the changed-child fixture
cloned a SmartObject cache key while hand-building different raster content with
the same tile stamp1 and child layer ID1. Document::new changes state.rev, but
CPU unstyled child composite cache uses root_stamp, so it legitimately reused
the first image in that invalid namespace/stamp combination. The fixture now
uses SmartObject::new for its independent changed child; same-namespace edits
remain tested through actual EditSmartObject/SetMask operations below. No
production change was made to accommodate this failure. Retry is pending the
heavy slot after the UI's final targeted checks. No timing sample yet.
