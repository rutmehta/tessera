# Private RAW normalization arithmetic implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. No new workers or runtime without coordinator authorization.

**Goal:** Share the existing packed-u16-to-camera-linear-f32 arithmetic through one private, owned-buffer helper, with explicit fallible allocation and bounded cooperative cancellation.

**Architecture:** Keep the helper inside raw-decode and route existing private `linearize` through it. Preserve scalar arithmetic, full-sensor phase, clamp and legacy Bayer/XTrans admission semantics. No image-core consumer, new public conversion API, file access or renderer is introduced.

**Tech stack:** Rust, Vec, existing engine-api EngineResult/CancellationToken, existing raw-decode/libraw-ffi data types. No dependency changes.

**Spec:** `docs/coordination/LIVE-RAW-NORMALIZATION-SEAM.md`, with `LIVE-RAW-CPU-ADMISSION-PLAN.md` as the broader stop-line context. Accepted pure admission is main47d987c2; reread current source before execution.

## Global constraints

- SOURCE-ONLY plan now: no tests/scaffold or production edits are authorized by this document alone.
- Runtime belongs to the coordinator's exclusive lane. Later Cargo commands use --locked, Release, jobs2, explicit shared BetterSSD target and deployment15; preserve every failure and source/fixture freeze.
- No public API, decoder classifier/native lifetime change, metadata extraction change, rendering, ICC/environment authority, numerical memory cap or reservation policy.
- No authentication claim from public decoded fields, no native/process allocation bound, no signed/HDR sensor contract. This preserves existing [0,1.2] normalization.
- The existing metadata path already calls sensor_info and does not make a second sample copy. Native unpack/cfa_data allocations are unchanged and outside this helper.
- Keep existing CfaImage::from_linear semantics unchanged; do not send integer samples through it.

## Review focus

1. Phase at offset active crops: normalize full sensor before crop; test asymmetric/non-square input and per-channel black values.
2. Legacy XTrans and permissive channel-layout validation: preserve existing <4 Bayer/<3 XTrans constraints, not the narrower image-core Bayer profile. Add equivalent valid/invalid controls.
3. Allocation failure/cancellation after partial work: consume/drop owned input and unpublished output; retain primary operation error and never leak a partial image.
4. Capacity versus sample count: record actual capacities and checked byte arithmetic; allocator rounding is not a proven requested-byte cap.
5. Very wide rows/large dimension arithmetic: bounded checkpoints within rows; no `i as u32` index truncation or giant allocation in overflow tests.

## Files and interface

- Create `crates/raw-decode/src/normalize.rs`: private plane validation/conversion helper, private checked-size logic, no unrelated refactor.
- Create `crates/raw-decode/src/normalize/tests.rs`: synthetic numerical/ownership/allocation/cancellation contracts.
- Modify `crates/raw-decode/src/lib.rs`: private module declaration and minimal `linearize` delegation only. Keep existing public signatures and `decode_cfa_u16` behavior intact.
- Existing raw-decode tests remain regression coverage; add a narrow legacy entry test only where needed to prove adapter wiring.

Proposed private interface (subject to source review before scaffold):

```rust
// Visibility at most pub(super), inside the private module.
struct PackedPlane {
    width: u32,
    height: u32,
    layout: CfaLayout,
    samples: Vec<u16>,
    black: [f32; 4],
    white: u32,
}
fn normalize(plane: PackedPlane, cancel: &CancellationToken) -> EngineResult<CfaImage>;
```

The helper moves the input Vec without cloning. It returns CfaImage owning one float Vec, constructed internally after all validation/checkpoints. No caller-supplied native/path access, generic production callback, exposed iterator or mutable reference escape. Legacy linearize creates a fresh never-cancelled token because its current public caller has no cancellation parameter; do not claim public decoder cancellation improves in this slice. Internal cancellation tests are honest seam capability only.

## Task 1: Private owned normalization helper

**Consumes:** reviewed synthetic PackedPlane and existing CancellationToken.
**Produces:** owned private CfaImage or existing EngineError, with no public caller yet.

- [ ] Read repository instructions/current code and preserve an exact source checkpoint. Prepare tests plus Unsupported helper only; no implementation before observed meaningful RED.
- [ ] Add independent expected-value tests using explicit f32 subtraction, division and clamp in that order. Test all Bayer phases and green1/3 conventions, valid XTrans phases, non-square dimensions, differing black channels, samples0/u16max/below-black/at-white/above-white. Compare output bits with a frozen independent scalar oracle; never compare two callers that both delegate to the new helper as the only parity proof.
- [ ] Assert full sensor extent/count and unchanged full-sensor channel origin despite an external offset crop control. Do not add crop to helper inputs. Include above-white result1.2 and interior nontrivial values to distinguish normalization from wrapping or double normalization.
- [ ] Add admission-before-reservation tests: empty dimensions, mismatched packed count, unsupported/out-of-range layout, NaN/Inf black and black>=white. Preserve legacy negative finite black behavior and do not invent a new white>0 rule beyond existing black<white semantics. Use private checked-size arithmetic to test usize/isize allocation overflow without giant vectors. Pointer-width-dependent cases must be explicit.
- [ ] Add deterministic allocation-failure test through a cfg(test)-only narrowly injected reserve outcome; return a typed existing allocation/decode error, not panic. A fake failure validates error control, not actual OS OOM recovery. Real Vec::try_reserve_exact remains production allocation.
- [ ] Add entry cancellation, cancellation at an interior bounded chunk boundary in a very wide row, and cancellation after final converted chunk/before publication. Use cfg(test) phase hooks/observations only, no global mutable state. Pin precedence: entry cancellation before new work; once validation/reservation fails that error is returned; after successful phase cancellation wins at the next checkpoint. Assert no returned partial image.
- [ ] Observe actual input/output Vec capacity and pointer transfer in test-only scoped instrumentation. Record 2*C16 and4*C32 checked bytes, verify no input clone and output allocation is moved into CfaPyramid. Scope instrumentation must not retain buffers. Do not advertise Vec-drop counters as allocator/RSS proof; separately assert normal Rust-owned values cannot escape on errors. No budget policy is added.
- [ ] Under separate runtime grant, compile and run these tests against Unsupported. Preserve compile issues separately; establish behavioral RED at missing helper, noting already-passing independent arithmetic/error controls.
- [ ] Implement minimum helper: entry cancellation; checked count/byte validation before output reservation; try_reserve_exact; bounded chunk loop with full-sensor coordinates; exact scalar expression; final cancellation check; move output Vec into CfaPyramid. Choose and document a small fixed checkpoint work quantum during source review, independent of any byte-cap policy. No full-plane uncancellable validation scan after conversion.
- [ ] Run focused GREEN under grant. Source-review owned lifetimes, actual capacities and error precedence before moving on. Commit bounded helper/tests checkpoint; no renderer claim.

### Exact live ownership for Task 1

On entry helper owns one Vec<u16> (capacity C16); metadata remains owned by its caller and is not copied into helper beyond scalars. While converting it owns input2*C16 plus output4*C32 and fixed loop/struct bookkeeping. Output length grows to S; capacity may exceed S. Checked arithmetic records capacity bytes but does not reserve/account a global quota or bound allocator overhead. On success input drops before return and output moves without clone; caller retains metadata M separately. Error/cancel drops both local Vecs. Native N has already ended only for a future closed-decoder caller; legacy RawSource::decode_cfa still retains RawFile while this helper runs, so its peak also includes N. Do not conflate these two lifetimes.

## Task 2: Delegate legacy linearize and qualify unchanged arithmetic

**Consumes:** Task1 helper.
**Produces:** legacy private linearize uses the shared arithmetic, preserving public behavior.

- [ ] Add a legacy-entry control with a synthetic libraw_ffi::CfaImage and independently expected pixels; do not reopen a file. Add cfg(test) scoped call observation if needed to demonstrate delegation. Existing numerical results may already pass before wiring; only an actual delegation assertion can be claimed as new RED.
- [ ] Run the narrow RED under coordinator grant, then destructure/move width,height,layout,data,black,white into PackedPlane and delegate using a fresh uncancelled token. Leave native unpack, cfa_data, path/classifier, metadata extraction and public APIs unchanged.
- [ ] Ensure the helper does not narrow legacy Bayer/XTrans policy or change error categories gratuitously. Validate no dependency/Cargo.lock drift. Compare before/after numerical controls at exact f32 bits; no tolerance widening.
- [ ] Run focused normalization/legacy tests, full raw-decode Release regression, strict all-target Clippy and formatting under sole runtime grant. Inventory any default-discovered external fixtures upfront and after; absent/early-return fixtures are not coverage. Do not require a new photo fixture solely for arithmetic tests.
- [ ] Freeze final source, direct exits and test executable digest; preserve RED/failures, independent expected-value source and fixture metadata. Commit reviewed owned files only, then independent review and coordinator integration.

## Stop line after Task 2

The private helper cannot be called from image-core across the crate boundary. Stop before adding any public conversion method, public generic closure, captured-normalized output, descriptor accessor or renderer route. A separate interface review must choose the smallest closed owned-value seam and trusted continuation. Actual post-decode admission/reservation limits, final rendering live buffers, fixed ICC bytes, verified environment manifest and source-provenance orchestration remain unresolved. This slice qualifies only shared scalar normalization and its internal ownership/cancellation behavior.

## Task1 source preparation ledger (UNRUN)

Ruling: checkpoint quantum is1024 converted samples, including within wide rows; this is a work quantum, not a numerical allocation budget. Event sequence for future observed helper is Validated -> Reserved(actual pointers/capacities) -> Chunk(completed cumulative count, at most1024 since previous) -> BeforePublish. Check cancellation on entry, immediately after each test observation boundary and before reserve/publication; a reserve failure returned together with cancellation remains the primary error. Invalid input returns before reserve/observations. No allocation or loop is implemented in this checkpoint.

Ruling: initial private module is cfg(test), retaining the exact proposed PackedPlane/normalize shape without a production call. normalize, normalize_observed and checked_sizes are all Unsupported stubs; the latter exposes only pure checked arithmetic for huge-dimension tests. Hooks are scoped references and test-only writable reserve seam, not a production callback or public API. They permit deterministic failure and deliberate excess capacity observation without unsafe allocators or global fault flags.

15 tests authored, compiler/tests UNRUN. Table bodies may stop at their first Unsupported failure during RED; do not claim all rows executed. Capacity test records actual pointer/capacity facts and verifies the returned float allocation is moved, with extra input and output capacity; it does not prove global allocator/RSS behavior. Error/cancel tests require no published partial CfaImage, while actual local Vec cleanup still needs implementation source review, not fake drop counters. Legacy linearize, public from_linear, native decode/metadata, Cargo files and fixture bytes unchanged. Scoped rustfmt only is permitted before source pin; no implementation/delegation until observed RED and separate authorization.

### Independent pre-RED review additions (UNRUN)

Preserved the original15 tests and added2 controls: cancellation at Validated must prevent reserve; cancellation at successful Reserved must prevent any converted chunk/publication. The isolated capacity control uses64-bit S=2^61 (width2^31,height2^30), or32-bit S=2^29 (width2^29,height1):4*S fits usize but exceeds isize::MAX, without allocating a plane. Other pointer widths require an explicit reviewed case rather than silent skip.17 authored tests; all Unsupported operations/legacy code remain unchanged. Scoped formatting only; no compiled RED/GREEN claim.

### Task1 implementation checkpoint (SOURCE ONLY, not GREEN)

Observed and independently reviewed RED at bf75fe66: compile0; exact executable dc7a46b10ed9abc3ecc911471f49748e916611685c25581c6ac47b23ffc97db3 ran all17 required names,0passed/17Unsupported failures/0ignored. Later table rows stopped at first failure. Evidence: BetterSSD raw-normalization/bf75fe66/RED-OBSERVATION.md; independent /tmp/tessera-normalization-red-review.md. Root subsequently authorized only private-helper implementation and scoped formatting; inspector retains runtime.

Implemented checked nonzero extent/count and usize/isize output size, legacy channel ranges and finite black<white validation, fallible exact reservation, actual input/output capacity byte arithmetic, and full-sensor exact f32 subtraction/division/clamp in1024-sample chunks. Entry and successful phase boundaries check cancellation; reservation failure remains primary. The scoped reserve hook must leave an empty output with sufficient capacity, preventing accidental infallible growth. Owned Vec input is neither cloned nor exposed; output is moved directly into CfaPyramid without an additional full-plane scan. Errors/cancellation drop local Vecs through ordinary ownership. No memory quota/RSS/native allocation bound is claimed.

All17 contract bodies are byte-unchanged; lib.rs/legacy/public behavior and Cargo files are unchanged from the reviewed RED source. Entire helper still belongs to the existing cfg(test) private module. Scoped rustfmt was run before source pin. No implementation compile/GREEN/full/strict run yet; source review and another runtime grant required. Task2 legacy delegation is not started.

### Task2 tests/observation preparation (SOURCE ONLY, UNRUN)

Task1 exact0434ff8d qualified17focused and88full passes/2explicit ignored, strict/fmt0; independent verification approved all9140source inputs and five-family original preservation. Root authorized Task2 tests/scaffold only, before runtime and delegation.

Four new synthetic legacy-entry tests: one requires the actual helper entry exactly once with identical owned-input pointer/capacity and an uncancelled token, after checking independent expected bits; three existing-behavior controls cover Bayer/XTrans full-sensor arithmetic despite crop offset, permissive layouts/negative black/zero white, and typed raw Decode errors. A cfg(test) thread-local scoped scalar observation records calls only from normalize itself; Scope Drop clears it, and it retains no input references or native handles. It neither redirects nor overrides computation. Legacy linearize remains byte-unchanged, so expected future RED is one missing-delegation failure (observed calls0 instead of1) plus three passing existing controls, not four new failures. No RED/GREEN has yet run for this checkpoint. Original17 tests remain byte-unchanged. No native lifetime change, public API, caller delegation or Task2 production implementation. Scoped rustfmt only.

### Task2 delegation implementation (SOURCE ONLY, UNRUN)

Observed and independently verified2fa62cef RED: compile0; exact4tests yielded3existing controls passed and1delegation-specific failure (actual helper calls0 versus1 at line118, after numeric equality). Later pointer/capacity/token assertions were not reached. Root then authorized minimal source-only delegation.

Legacy linearize now moves its data Vec and width/height/layout/black/white scalars into the private helper with a fresh uncancelled token. Native RawSource/RawFile/unpack/metadata lifetimes and public APIs are untouched. Private module is now compiled in production; PackedPlane/normalize visibility is parent-only. Production normalize_inner has no hooks parameter or callback; reserve calls a concrete fallible function. Event/Hook definitions, observer calls, thread-local entry observation and legacy tests are all cfg(test). The shared arithmetic/validation/cancellation loop is unchanged except conditional compilation of those observations. Existing packed-plane/layout/black-white errors retain raw Decode category and legacy strings; checked overflow/allocation failures retain the new bounded helper errors. Output Vec remains moved, never rescanned/cloned.

All17+4 test bodies remain byte-unchanged from RED. No Cargo drift, public path/resolver/conversion API, memory-cap policy, captured continuation, renderer or ICC/environment changes. Scoped formatting only; delegation implementation is not compiled or tested yet. Source review must precede a new GREEN/full/strict/fmt runtime grant.
