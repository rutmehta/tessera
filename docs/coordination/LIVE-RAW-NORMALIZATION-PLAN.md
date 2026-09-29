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
