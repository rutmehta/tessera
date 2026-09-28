# Closed Captured RAW Decoder Adapter Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans. No workers, implementation, compiler, decoder execution or GUI until root grants that scope/lane. This document is source-only planning.

**Goal:** Consume an already verified CapturedRaw, retain its private stage through classification, LibRaw open/unpack and metadata extraction, and return only owned CFA samples/metadata plus captured identity.

**Architecture:** Add one closed synchronous decoder method inside raw-decode::capture. It admits only the stored LibRawCfaV1 route, rejects recognized LinearRaw DNG before LibRaw, opens only the private staged filename, and destroys all native decoder handles before explicit stage cleanup. The returned concrete Rust type contains no path, stream, callback, decoder pointer or capture owner.

**Tech Stack:** Existing Rust raw-decode, libraw-ffi/LibRaw0.22.2, engine-api CancellationToken and errors; no new dependency, archive or FFI binding.

**Spec:** docs/coordination/LIVE-RAW-CAPTURE-POLICY.md; docs/coordination/LIVE-RAW-PINNED-SOURCE-PROPOSAL.md. Capture Tasks1–3 are already accepted at af5473d8 and root merged main1b073b0c/evidence1ad19650. Do not redo them.

## Actual source findings

1. `crates/raw-decode/src/lib.rs:179–232`: RawSource owns a PathBuf and live RawFile; open does not unpack. decode_cfa_u16 calls unpack, copies samples, validates plane, and returns CfaU16. Returning RawSource would be a lazy/path escape and is specifically unsuitable.
2. `crates/libraw-ffi/src/lib.rs:63–91`: RawFile::open calls libraw_open_file with a Unix CString filename. Native handle remains live; unpack is a later synchronous native call. No read-buffer API is exposed. The private stage filename is therefore necessary internally, and must remain available through the whole native lifetime.
3. `crates/libraw-ffi/src/lib.rs:93–115`: cfa_data copies native samples row-by-row into Vec<u16>. Metadata at147–194 copies strings, arrays and each opcode payload into owned Vecs (each native opcode <4MiB). RawFile Drop at196–199 closes native allocations/stream. Returned CfaU16 and RawMetadata contain no borrowed LibRaw references.
4. `crates/raw-decode/src/lib.rs:235–260`: existing decode_cfa float path normalizes black/white and clamps to0..1.2. The smallest ownership adapter should use decode_cfa_u16, preserving packed sensor integers and metadata without adding a normalized/rendered-output policy. This is not a RawImage/image-core adapter.
5. `crates/image-core/src/source.rs:57–75`: broad RawImage::open first opens a Rust File for LinearRaw classification, then may take an RGB path or separately open RawSource. Do NOT call it: it broadens the frozen CFA route, requires image identity, introduces dependency direction/render semantics, and can choose several filename-based backends.
6. `crates/raw-decode/src/linear_dng.rs:21–58`: bounded Read+Seek classifier checks the first IFD; CFA DNG remains on LibRaw, recognized LinearRaw requires another decoder. Run this classifier on the captured readonly descriptor (not original). A positive LinearRaw result returns Unsupported for this route; classifier error returns Decode, without fallback. No generic RGB/ImageIO recognition is needed; LibRaw CFA validation rejects non-CFA output.
7. `crates/raw-decode/src/lib.rs:193–206`: metadata reads dimensions/calibration from sensor_info. Call metadata after unpack, matching existing image-core ordering. Check decoded dimensions/layout agree with metadata; retain EXIF orientation unapplied, raw opcode bytes, black/white values, WB/matrices and crop verbatim. This is not proof calibration is usable for rendering.
8. Current libraw-ffi::RawFile::open directly imports std::os::unix. Accepted capture likewise admits Unix and returns Unsupported elsewhere. No non-Unix decoder build/support claim follows; don't introduce a weaker pathname workaround.

## Global constraints and explicit limitations

- Original pathname is not stored by CapturedRaw and is never an adapter argument. All probes/metadata/unpack use the private stage or native handle opened from it. Replacing original A with B after capture cannot redirect this adapter.
- Captured identity names the captured stream, which may be mixed under concurrent in-place source writes. It is not an atomic source snapshot. Decoding does not strengthen that guarantee or create atomic RAW+recipe capture.
- CapturedRaw is held bytes; DecodedCapturedCfa is owned decoded output. Neither implies recipe/catalog identity, dependency binding, render admission, pixel reproducibility or persistent availability.
- Capture byte quotas bound staged bytes, NOT decompressed memory or LibRaw RSS. Existing cfa_data reserves/copies width*height after native unpack; it has no caller allocation budget. This smallest ownership slice makes no decoder-memory bound claim. If a hard native/output allocation limit is required before enabling a caller, that is an additional reviewed libraw-ffi change, not satisfied by a post-allocation dimension check. LibRaw exposes rawparams.max_raw_memory_mb internally, but the Rust wrapper neither configures it nor offers a hard cross-decoder guarantee.
- Cancellation is cooperative between classifier/open/unpack/metadata/copy phases. No native progress callback exists in wrapper; a request cannot interrupt libraw_open_file or libraw_unpack mid-call. Check cancellation immediately after each, before publishing output. No hard wall-time or preemptive-cancel claim. RawSource::decode_cfa_u16 combines native unpack and owned sample extraction; without modifying that API, there is no cancellation hook between those two operations. Test hooks must describe the actual before/after decode_cfa_u16 boundary, not claim an intermediate native callback.
- No embedded-preview extraction, demosaic, float normalization, color transform, lens resolution, ICC, recipe, catalog, export, document node, public FFI or Swift change.
- At each completed phase, preserve its error before checking cancellation; if the phase succeeded, cancellation wins before the next native action or output publication. Pre-cancellation prevents all probing. Cleanup never overrides an existing decoder/classifier/cancellation error.
- Retain stage ownership on every success/error/panic path. Existing cleanup failure policy remains: return cleanup error on otherwise-successful decode; preserve primary decoder/cancel error and report secondary cleanup failure. Failed cleanup retains quarantine charge.

## Proposed API and files

```rust
// In raw_decode::capture; fields are owned and safe to move after stage deletion.
pub struct DecodedCapturedCfa {
    pub image: crate::CfaU16,
    pub metadata: crate::RawMetadata,
    pub identity: CapturedAssetIdentity,
    pub route: engine_api::pinned_raw::PinnedRawDecoderRoute,
}
impl CapturedRaw {
    pub fn decode_cfa(self, cancel: &CancellationToken)
        -> EngineResult<DecodedCapturedCfa>;
}
```

The method name documents CFA route, and result image is explicitly CfaU16 (not normalized CfaImage). No generic callback, PathBuf, RawSource, RawFile, std::fs::File, reader, lazy iterator or CapturedRaw escapes in the result. Result identity is the previously verified captured identity; no claim that caller-supplied digest proves bytes. Route remains explicit even though only one enum variant is currently accepted.

Owned file map (four files):
- Modify `crates/raw-decode/src/capture.rs`: private child module + public result re-export; retain existing capture behavior. Existing private pool/cleanup ownership accessible to child.
- Create `crates/raw-decode/src/capture/decode.rs`: concrete adapter and closed output type; no generic production consumer API.
- Modify `crates/raw-decode/src/capture/tests.rs`: wire new test module.
- Create `crates/raw-decode/src/capture/tests/decode.rs`: deterministic protocol/cleanup controls and explicit fixture qualification.
No Cargo dependency, libraw-ffi implementation, image-core, bindings or global RawSource behavior change in this minimal proposal.

## Task 1: Closed adapter and refusal/lifetime protocol

- [ ] Prepare type/method and tests first, record compilation-only failures separately, then observe behavioral RED before implementation.
- [ ] On entry, check cancel. Match stored route. Use existing readonly stage File for bounded LinearRaw classification. It may seek; decoder subsequently opens its own independent stage stream. Check cancellation after successful classification and before any native open. Positive LinearRaw -> Unsupported; malformed classifier -> Decode, no fallback.
- [ ] In an inner function/block returning only owned `(CfaU16, RawMetadata)`, construct RawSource from `self.stage.path()`. Check cancel after open, call decode_cfa_u16, check cancel, extract post-unpack metadata, check cancel, validate dimension/layout consistency without inventing rendering allowlists. Ensure RawSource/native handle is dropped by leaving this inner scope before self.close runs.
- [ ] Assemble DecodedCapturedCfa only after successful decode and cancellation checks. Explicitly clean stage with existing error-precedence semantics; on cleanup error discard otherwise-successful decoded output and return error. Handle cancelled/error branches through one finishing path rather than early-returning past secondary cleanup reporting. Panic still relies on ordinary unwind/RAII.
- [ ] Private cfg(test) phase hooks may coordinate/cancel or report a chosen failure, but the output type remains concrete and production always calls the real decoder. Do not use a fake generic callback as evidence that real LibRaw decoded. Test-only fake decoder lifecycle can exercise destruction ordering separately, named as such.
- [ ] Commit only after focused RED/GREEN and review; public API is unavailable until supported actual fixtures pass Task2.

Deterministic contracts:
1. Capture malformed synthetic bytes, invoke actual adapter -> Decode; stage removed/charge released and no original reopen. Capture a minimal LinearRaw DNG (reuse existing tests' valid TIFF construction) -> Unsupported before native CFA open; malformed classifier -> Decode. No fallback.
2. Pre-cancelled owner -> Cancelled, no probe/native open; phase cancel after open/before decode_cfa_u16 and after decode_cfa_u16 (unpack plus sample copy) -> no returned output, decoder dropped before stage unlink. Use deterministic hooks/channels, not sleep-based timing claims.
3. Inject decoder error while unlink fails -> primary Decode preserved, secondary diagnostic captured, retained stage/charge, bounded final retry. Otherwise-successful synthetic closed output plus unlink failure -> Io, no result published. These are protocol tests, not actual-pixel fidelity tests.
4. Hold a test decoder at a named read phase; original replaced with B; all private stage probes still read A and stage/quota remain until decoder destruction. Assert drop-order using operation events: decoder-drop precedes remove-file. No test publishes a generic reader/path.
5. Panic/error controls clean or quarantine the stage via existing RAII. Negative expected-identity capture yields no CapturedRaw and therefore cannot enter adapter (existing gate composition; don't claim it newly fails RED).

## Task 2: Actual decoder qualification and owned-result proof

- [ ] Use copies of existing read-only fixtures in an isolated temp directory. Do not change fixture originals. Capture A, rename/remove that COPY, replace its locator with invalid B, then call actual adapter. Compare against direct existing RawSource decode of A performed before replacement: exact u16 plane, width/height/layout, all RawMetadata fields (floats compare bit representations where needed), opcode payloads, capture identity and orientation.
- [ ] Minimum representative positive CFA families: existing SonyARW, FujiRAF(XTrans), NikonNEF, CanonCR3, CFA DNG. Existing fixture loader supports these; log precisely which execute. Qualification test is explicitly ignored in ordinary suites; its dedicated --ignored gate requires the environment directory and all five named families, failing if absent. No silent early return or universal pass claim.
- [ ] After adapter returns, assert stage and private pool directory disappear after owner/pool drop. Then read samples/metadata/opcode bytes from result and compute a checksum; proves output still usable after native handle/stage destruction. Verify original fixture hashes before/after. No full RGB render or external calibration result expected.
- [ ] Normalized stored suffix + relocation `.bin` control: capture with original ARW route/suffix from a renamed synthetic fixture COPY, confirm real CFA decode independent of original locator spelling. The stage retains recorded `.arw`; LibRaw may inspect contents, but route selection does not change to RGB.
- [ ] Run focused adapter, all capture tests, full raw-decode Release, affected libraw-ffi existing tests only if wrapper sources change (not planned), strict raw-decode all-targets and fmt. Preserve failing attempts, source manifests, direct exits, fixture hashes. Same explicit shared BetterSSD target/MACOSX15/jobs2. No concurrent GUI/GPU work.
- [ ] Independent whole-change review before root merges; no implementer main merge. Scope acceptance is owned CFA output under held-byte identity only.

## Feasibility recommendation

Proceed with this raw-decode-only closed CFA-u16 adapter when root prioritizes it. Existing decoding already provides owned buffers; private filename retention solves the real delayed-unpack lifetime seam without exposing paths or pulling in image-core. The meaningful limitations are native decode memory/time and cancellation latency, Unix-only wrapper assumptions, unsupported LinearRaw/RGB routes, and absent recipe/render dependency admission. If the next product caller requires any of those broader guarantees, stop at this owned-output boundary and specify them separately rather than widening the adapter implicitly.

Review clarification: fake decoder drop events test common owner protocol only. Actual RawFile::Drop -> libraw_close ordering must be source-reviewed in the real decoder inner scope; real fixture owned-output tests do not directly instrument native close.
