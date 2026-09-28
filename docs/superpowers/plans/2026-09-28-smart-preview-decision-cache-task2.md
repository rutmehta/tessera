# Decision reuse Task 2 integration contracts — source preparation

Base: accepted pure Task102752d2845528b378bdeb8d1b7229bd4e68c46c8 on baseline product3614e21b. Parent plan remains2026-09-28-smart-preview-decision-reuse.md. **No production wiring, compiler, formatter, actual decoder/GPU/app workload or candidate benchmark in this checkpoint.** Inspector owns runtime. Native two-file delta only declares a cfg(test,macOS) module and adds six ignored Engine integration tests with explicit no-op/default observation stubs. Tests have not compiled or executed.

## Proposed internal caller changes (not implemented)

1. Promote pure backend/proxy_decision_cache out of cfg(test), with only required crate-private visibility; Engine owns Mutex<Cache>, default empty. No static map, persistent cache or public FFI API. Preserve fixed16/8KiB/noDrop entry type. Lock only copied lookup/insert operations; poisoning advisorybypass. Tests' scalar Probe and SelectionControl become Engine-local cfgtest state, never a global strongArc registry.
2. Replace load_smart_preview's tuple with private validated result containing existing journal/snapshot/RawImage/path and copied full decode containerdigest/encoding dimensions plus journal incarnation getter. It must retain all current actual store/regularfile/codec/source digest checks. Return no digest inferred from128bit renderId. Update existing info/sync callers mechanically without granting cache use there. Identity must originate in the same validated decode+journal object, no secondread.
3. In open_develop_source(proxy), finish stableID admission, load/reconcile/localdoc/originalsidecar baseline/process/proxy recipe checks BEFORE any decision lookup. Pass validated identity only from that successful branch through develop_render_resources to develop_renderer. Use Option or distinct resource entrypoint so histogram/other callers lacking validated identity retain current uncached behavior. Original rendererOnceLock/default and dirtylocal original/export guards unchanged.
4. Refactor select_proxy into equivalent preparation/capability and measured selection result. Probe normalized pixel settings on the GPU-capability implementation even when last decision selectedCPU; do not test CPU renderer's resident support and mistake honest measuredCPU for unsupported. Preserve fullfiniteHDR/headroom in key; temporaryHDRfalse/headroom0 only for existingSDRcalibration and capability admission. All sixfinitepositive samples required to cache measuredCPU orMetal. Unavailable/failedcalibration never successfulCPU insertion.
5. Exact auto hit materializes fresh chosenbackend/ops/cache; devicehealth/capability checked eachopen. GPUfailure onhit dropsentry/falls backhonestly. Devicehealth failure clears bothdecisionkinds. Override/externalunversioned/mapped/unsupported paths bypass lookup/publication. Existing per-edit geometryCPU fallback remains authoritative after cachedGPUselection. Settings/assets without complete externalcontentidentity bypass ratherthan guess.
6. Add test-only measurement/device/preference/eligibility control at these bounded seams, after realvalidation. A control must NOT fake source bytes, recipe compatibility, source admission or cache lookup outcome. MeasuredCPU/Metal controls supply valid fixedsampleoutcomes, while backendmaterialization/capability must follow actual productionpath. Probe stores scalars/fixeddigests only, neverBackend/RawImage/Surface. No microbenchmark timings from controlled tests may be published.

## Six executable test contracts prepared

- `unchanged_public_open_reuses_cpu_and_metal_decisions_in_sdr_and_hdr`: fourcases, exactsettings/recipe retained; newvalidation eachopen; secondopenlookuphit with nomeasurement; finiteHDRkey retains policy; normalizedGPUcapability checked even for measuredCPU; no Shared/Renderer retention after close.
- `cold_public_open_rejects_invalid_sources_without_cache`: container/journal/original/sidecar corruption on disposablecopies. **Pre-existing validation controls**, expectedalreadygreen, not newbehaviorRED. Failure occurs before backendselection and should not require aGPU workload.
- `cached_decision_cannot_bypass_real_source_or_sidecar_validation`: samefourinvalidations after priming realpublicopen; failsbeforelookup/hit/measurement. Priming introduces selection; later failureisrealvalidation, not fault-hook rejection.
- `validated_full_identity_is_transported_and_rebuild_or_edit_misses`: probeall256containerbits against actualdecodedcontainer; incarnation againstrealjournalrecord;generation/exactdocumentdigest againstvalidated snapshot. Clean discard/rebuild changesincarnation; persistededit changesgeneration/doc/settings andforcesmiss. Existingdirtylocal Originalopen refusal retained. Lowlevel key tests already prove container tail mutation with identicalfirst128bits; no infeasible256bit collision construction claimed by Engine tests.
- `overrides_external_geometry_and_device_failures_do_not_reuse_or_poison`: controlledexplicitCPU/Metal/externalunversioned/device policies plusrealgeometryedit. Calibration failure is exercised on an emptycache, not incorrectly expected to preempt avalidhit. Later success caninsert. External control tests routing after dependency resolver reportsunversioned; actualmutableexternalasset contentresolution remains source-review/production eligibility responsibility and needs a dedicatedassetfixture before claiming that resolververified.
- `cache_does_not_override_active_editor_or_retain_failed_open_lease`: activeeditor rejection beforeselection; corruptopen thenrestorepermitsnextopen; no cache-induced lease retention.

Fixturehelper requires explicitTESSERA_SMART_PREVIEW_RAW and builds nativecurrent/denoiseOff Compact from a disposablecopy. Originalfixture read-onlyhash checked. Scalar/defaultstubs do not fake a cachehit. Newtests will normallyfail first at missingmeasurementpublication after initialpublicopen; sourcecontrol mayalready pass. Record observed compileerrors separately frombehaviorRED, not guesscounts now.

## Runtime classification and next gate

Allsix require explicitactualRAWcopy/prefixgeneration; they are not purememorytests. The coldnegativecontrol performsvalidationfailures before GPUselection. Five newintegrationtests currently reach realuncached publicopen before defaultProbe fails: the RED phase therefore **may initialize/calibrate realMetal**, including first fixture priming. Do not schedule these beside GUI/inspector. Controlledselection becomes deterministic only after later actualtesthook wiring; such results establish routing/ownership contracts, not realperformance. Proposed optin command afterindependentreview/exclusivegrant: `cargo test -p tessera-ffi --lib --release proxy_cache_contracts -- --ignored --nocapture --test-threads=1`, existingBetterSSDtarget/deployment15/jobs2, explicitfixture. Runner must scrubinheritedTESSERAflags andset auto preference. Preserve source/fixturebeforeafter andallfailures.

No IOSurface frames or timedbenchmark in these contracts. WeakShared/Renderer checks coverthoseowners; fixedentrytype and source review must ensure no separatelyretainedheavyimage/backend; actualGPUoperator/rings/sourceidentity lifecycle remains existing realEngine qualification plus Task3 finalcache-hit extension. This scaffold does not claim those omitted per-frame proofs. Cache countertests supplement, never replace actual eligibleMetal/zero-readback/fallback proof or baseline/candidate first-final-surface thresholds.

## Independent contract review corrections (source-only)

The winner matrix now asserts the actual returned session.info().backend on both initial and hit materialization (CPU/Metal), independently of Probe. Cycle retains only a Weak Renderer after verifying Shared/Renderer drainage; keeping the first Weak alive makes pointer inequality on the second cycle a valid fresh-allocation check without retaining heavy objects or allowing allocator address reuse. This is still selected-backend materialization evidence, not per-frame Metal submission proof.

Probe now includes publication count. Explicit overrides and unversioned-external routing must leave lookups, hits, publications and entrycount unchanged; explicit CPU/Metal also leave measurement count unchanged and return their actual requested backend. Real mapped-geometry edit must bypass lookup/publication. Device unavailable/lost and failed-calibration controls assert actualCPU fallback; device failure clears entries. Failed-calibration on emptycache must publish nothing. Counts must later be emitted at actual cache insert/lookup and measurement boundaries, never assigned from a control label.

Full identity oracle now also compares copied owner bytes, original digest/length (independently against both decoded container and disposable original), proxy dimensions, tier, encoding, and container format version from the validated fixture header. These complement fullcontainer/incarnation/generation/documentdigest assertions. Header/original reads occur only in the test oracle; no extra production decode/journal/path read is authorized by this change. All six tests remain UNRUN, and production wiring remains absent.


## Authorized instrumentation-only checkpoint (source, unrun)

Attempt `01-red` remains instrumentation RED: five failures reached the first
actual-backend assertion because the no-op MeasuredCpu control allowed automatic
Metal selection. The existing cold-validation group passed. No cache behavior
assertion was reached, and its immutable runner/logs remain unchanged.

The coordinator subsequently authorized Engine-local cfg(test) instrumentation
without cache integration. A scalar-only control/probe mutex is added to test
Engines; ordinary selection passes no observer. Original selection still passes
None. Actual CPU/Metal constructors and existing winner rule remain shared with
ordinary selection. Only test measurement outcomes/preference/device availability
are controlled. No mutex is held across device construction, capability queries,
measurement, or rendering. DeviceLost currently means controlled unavailability
at a later selection, not a simulated driver failure while a frame is in flight.

Validated identity is copied from the already-decoded container and already-open
journal into a stack observation, committed only after process/prefix and sidecar
validation succeeds. No original/container/journal reread was added. The existing
full recipe HDR policy is recorded separately from the real candidate GPU's
normalized capability query. `key_hdr` fields are policy observations here; no key
or cache exists in the Engine. No cache counter is incremented or populated.

One additional ignored test checks real CPU and Metal materialization with two
uncached opens for both SDR and HDR, fresh/released Renderer allocations, two
validation/measurement/capability events, and zero lookup/hit/publication/entry
counts. It also checks corruption cannot reach selection, real override backend
materialization, unavailable-device CPU fallback and calibration-error CPU fallback.
Controlled samples are not GPU execution/performance evidence.

The original six contract bodies and their assertion ordering are retained. Their
next expected failure is the first missing entries/publication assertion in prime;
missing reuse is a later, still-latent requirement. New control GREEN and unchanged
contracts RED must be observed under a new immutable runner/source pin when runtime
is granted. No compiler, formatter, test, app or GPU was run for this checkpoint.

The external dependency checklist remains mandatory before cache wiring:
`/tmp/tessera-proxy-cache-external-dependency-checklist.md` (host-local review).
This stage never reuses any decision, so every external/geometry route bypasses
reuse. UnversionedExternal currently leaves ordinary uncached calibration intact;
it does not pretend to inspect an actual mutable resolver. No eligibility guard,
external-content guarantee, device-loss eviction, or cache speedup is claimed.


## Authorized decision reuse implementation — source checkpoint, UNRUN

After independent acceptance of instrumentation e4f573d2 and observed missing
initial-entry RED, the coordinator authorized production cache wiring source-only.
No timing claim, runtime gate, or rollout acceptance follows from this checkpoint.

Engine now owns `proxy_decision_cache::Store`, a fixed 16-entry Cache plus Mutex
and monotonic epoch; compile-time bounds cover both Cache and Store <=8 KiB.
Entries contain only key/decision/fixed samples/ordinals/timestamps. Full container,
original digest/length, owner, dimensions, explicit tier AND encoding/format,
journal incarnation/generation/document digest travel by copied AssetIdentity from
the original successful load into resource selection. Existing public tuple callers
retain a wrapper. No second original, container or journal read was added.

Lookup occurs after stable-ID admission and all existing source/sidecar/document/
prefix checks; current GPU operators must also construct successfully and the
shared device must report healthy. Only normalized actual candidate GPU capability
permits reuse, including measured-CPU hits. Full presentation settings remain in
the key alongside ordered graph/config and explicit adapter/capability descriptor.
Engine's OnceLock cannot replace its device; generation1 is valid only under that
existing lifecycle. A future replacement API must change generation and clear.

External admission requires default camera profile, no LUT/proof profile, no
locals/retouch and no lens blur, plus NativeCurrent (currently Native2), and real
resident capability (which rejects mapped geometry). Captured Auto lens prefix
remains container-bound. Explicit overrides return before lookup/publication.
Unavailable/lost device/context failure clears advisory records and retains CPU
fallback. Calibration errors/invalid samples never publish. Key failure bypasses
without introducing a new open error. Poison clears records and leaves the Store
poisoned so subsequent operations bypass, rather than authorizing recovered state.

Hits return newly constructed CPU/Metal Backend operators; no warmed render cache
survives a closed session. Misses preserve existing six measurements and winner
rule; completion timestamp is captured before the bounded publication lock, and
hits do not extend it. Duplicate concurrent misses remain allowed. No mutex is
held during file reads, key encoding, capability, operator creation or rendering.

Scalar validation observation now preserves cumulative lookup/hit/publication/
entry counters. The unchanged six original integration bodies remain intact.
The separately accepted uncached control explicitly disables only decision reuse
through an Engine-local cfg(test) bit; its two-measurement/zero-cache assertions
remain unchanged. New tests cover encoding tag key sensitivity, Store byte bound,
poisoned-store bypass, actual external settings admission, and persisted named
profile/LUT bypass through public proxy open. Pure external tests include proof,
disabled locals/lens blur, and nonnative process; these are dependency admission
contracts, not a claim that mutable external assets are resolved or content hashed.

Source-only: no formatter/compiler/test/GPU/app/benchmark run. Required next step
is independent code review, then root-granted compile plus pure cache tests, eight
ignored integration groups, existing native workflows/full/strict/fmt as assigned.
Historical six-group runner must not silently omit the new external group; retain
prior runners and freeze a new test inventory. First-frame performance, actual
uncontrolled calibration, broad lifecycle, archive and Swift qualification remain
subsequent tasks. Original default and public FFI signatures are unchanged.
