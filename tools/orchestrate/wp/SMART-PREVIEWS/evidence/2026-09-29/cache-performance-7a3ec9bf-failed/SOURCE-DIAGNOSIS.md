# Warm WB regression: source-only diagnosis and bounded next step

Status: overall Task3 performance FAILED. No waiver, selective rerun, implementation change or new runtime. Exact candidate7a3ec9bf/base5f31f148 and source-preparation-v2 remain frozen. Full report: BetterSSD/proxy-decision-cache/task3/PERFORMANCE-VALIDATION.md.

## Evidence

All22 processes passed route/resource/fidelity/freeze checks; final runner exited1 on both warm-WB gates. SDR5.016->14.738ms and EDR4.850->14.446ms. All10 candidate WB samples were13.679–20.663ms, versus all10 baseline4.628–5.900ms, so this is not a single outlier. Every WB frame submits once on Metal at matching L1/820x546, settings exposure0.5/Daylight/5500K/tint0. No changed geometry or format is hidden in the comparison.

The actual baseline logs explicitly show L0 calibration [first,tone,WB] every session. In pair0-A-sdr cycle5 GPU calibration WB costs13.613ms, while the subsequent measured user WB frame costs5.016ms. This is consistent with the baseline paying the WB computation before the measured edit. Candidate cache hits skip that work and deliver a fresh operator/cache as designed. It is a real regression relative to existing session behavior, even though the benchmark happens to request the exact calibration state. It cannot be relabeled away.

## Exact source mechanism

backend.rs measure_at clones initialsettings(exposure0.25/AsShot), renders original, increments exposure0.25, then togglesWB toDaylight. Those are exactly the harness's exposure and WB probe settings at decision_reuse_qualification.rs326–349. The calibration renderer is built by Backend::renderer over the same Arc StageOp and TileCache subsequently used by the session; it does not discard the selected backend after measuring.

On candidate hit, select_inner returns newly built CPU/Metal Backend before measure_at. Thus new per-session caches contain only states rendered by the actual viewport. The initial AsShot frame warms source/AsShot intermediates; the exposure edit reuses upstream values, but first Daylight invalidates WB and downstream detail.

image-core/resident_render.rs balanced_tiles772+ caches camera source under Demosaic and profile/WB output under WhiteBalance; develop_resident_level972+ caches paddedWB and Detail with ordered recipe hashes. run_camera_linear_coarse1121+ first develops the full L0 linear tail then downsamples, so baseline L0 calibration intermediates can also benefit the L1 displayed frame. Changed WB can therefore redo full proxy matrix/gather/detail work even at a small viewport.

GpuContext::from_shared/from_device compiles its base pipeline during backend construction; GpuStageOp::with_cache_budget creates resident/gather pipelines before returning. These operations occur before both variants' measured edits. Some other lazy kernels exist; current telemetry lacks stage-cache hit and pipeline-creation counters, so exact attribution between intermediate recomputation and any lazy compilation is not proven. The source/evidence strongest hypothesis is calibration-primed exact Daylight intermediates, not a timer defect.

## Timing oracle review

Both variants share the same edit_frame body: serialize settings beforetimer, start before set_settings, listener timestamps callback entry, await exactgeneration+final, stop at thattimestamp. Pixel readback and file writes follow. Expected routes are independently asserted. There is no source evidence of a variant-specific measured-interval difference. Scheduling and GPU stage attribution remain unmeasured. Do not weaken the warm gate or substitute another WB value for the failed probe.

## Smallest follow-up proposal (requires separate review/grant)

First use test-only scalar counters at the existing resident cache lookups for camera source, WB tiles, paddedWB and Detail, plus existing dispatch count per edit. Capture copied counter deltas around calibration, initial frame, exposure and firstDaylight; no strong cache/image/backend retention and no pixel reads in timed intervals. Add per-operator pipeline creation counts only if existing counters cannot exclude compilation. The falsifiable prediction is baseline final-session Daylight WB/detail cache hits and candidate misses, with matching source hits; any contrary outcome rejects this explanation. Preserve original uninstrumented failed data and keep diagnostic timing separate.

A second same-session repeat of identical WB can be an additional diagnostic only, never replace the preregistered first WB gate. Its predicted convergence would distinguish first-state cache miss from persistent edit overhead.

Do not pre-render calibration/Daylight on hit or retain previous heavy objects: that violates this decision-only slice and shifts work to evade the oracle. No immediate decision-cache-only repair is justified. If diagnostics confirm recomputation, any product follow-on must independently reduce first-time WB-dependent work in the existing proxy renderer (for example separating immutable camera-profile work from WB or avoiding redundant identity-detail work), preserve operation order/numeric bounds/cache accounting, and pass the unchanged full first-frame and first-WB gates. Such renderer optimization is a separately scoped proposal, not implemented or presumed sufficient here. If that scope is undesirable, leave the decision cache unmerged.
