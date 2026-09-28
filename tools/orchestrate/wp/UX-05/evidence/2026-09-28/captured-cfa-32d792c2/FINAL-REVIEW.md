# Closed captured-CFA adapter final independent review

**Verdict: approved for the stated closed owned-CFA boundary. No actionable correctness finding.**

Reviewed immutable `32d792c25d65e4f1c551f8abab360b52432b3d3d` against `3a395c69b6946660daf2d30a595149d5ef26e144` in `/Users/rutmehta/.codex/worktrees/export-integration/tessera`. Worktree was clean at review. Source-only inspection and evidence/hash verification; no compiler, decoder, GPU, GUI or benchmark execution by reviewer. Companion `/tmp/tessera-captured-cfa-final-review.json` contains direct gate exits, log hashes, exact five fixture hashes and immutable provenance results.

## Source and contract review

- The product delta is confined to capture module wiring, new closed decoder implementation and tests; existing RawSource/libraw-ffi decoding is unchanged. The approved plan is the fifth changed file. No Cargo, FFI/Swift, image-core, recipe, catalog or render integration was added.
- Public `CapturedRaw::decode_cfa(self, token)` always calls the real `decode_native` through the same private `decode_closed` classification/ownership/finish path used by test controls. The generic callable seam remains private; test entry points are cfg(test,unix). Returned `DecodedCapturedCfa` contains owned CfaU16/RawMetadata and copied identity/route only—no filename, native handle, reader, lazy output or capture owner.
- Classification reads the retained readonly stage descriptor. Existing classifier bounds first-IFD entries and file ranges. A recognized LinearRaw route is explicitly Unsupported; classifier I/O/malformed errors are Decode. No alternate decoder fallback or reopening of the original locator exists. The native path is exclusively the private retained stage filename.
- The concrete real decoder opens RawSource, calls its existing packed-u16 decode/plane validation, then copies post-unpack metadata. Both actual and fake paths converge on explicit image/metadata dimension and CFA-layout agreement checks. All RawMetadata fields are owned; LibRaw strings/opcode buffers are copied by the existing wrapper. No claim that these calibration fields are adequate for a rendered result is introduced.
- Cancellation is checked before classification and after successful classification/open/decode/metadata and before publishing. Completed call errors propagate before the next cancellation check. Open/unpack plus sample extraction are synchronous boundaries, not preemptible operations. Synthetic controls and explicit real Sony phase cancellation tests name those actual boundaries accurately.
- On success `drop(source)` closes RawFile before the outer stage close. On native error/cancel, ordinary function-scope unwind drops the local RawSource before returning to the common cleanup path; RawFile::Drop invokes libraw_close. The synthetic Drop ordering test supports owner protocol only; actual close order is established by this source structure, not a fabricated native telemetry claim.
- Cleanup on otherwise-successful decode must succeed before output escapes; failure returns Io and discards owned output. Decode/cancel errors remain primary, secondary cleanup errors are reported, and existing quarantine charge retention/bounded final retry semantics are unchanged. Panic uses existing RAII. No pool callbacks or cleanup lock policy changed in this delta.

## Actual fixture oracle

The dedicated ignored-test invocation requires every named fixture and runs both actual qualification tests (2 passed, 0 ignored). For each of Sony ARW, Fuji RAF, Nikon NEF, Canon CR3 and CFA DNG: direct existing decoder establishes the packed plane/metadata from a disposable relocated `.bin` copy; capture retains normalized original-format stage suffix; the disposable original locator is replaced by invalid B; actual captured decode still produces exact u16 A. The test drops external pool ownership before decoding, asserts stage and pool directory absent afterward, then reads/compares the complete owned plane and every metadata field, including float bit patterns and opcode bytes. Exact vector equality is stronger than an output checksum for these comparisons. The invalid B remains untouched. Five positive EXERCISED lines and dimensions/sample counts were independently verified from the actual log.

No fixture absence skip is accepted as family evidence. This qualifies these five fixtures, not all files/cameras/formats. Sony real cancellation after open and after successful decode is separately executed in the same dedicated gate.

## Immutable evidence verification

Evidence root: `/Volumes/betterSSD/tessera-validation/private-raw-capture/decoder` (host-local).

- Independently SHA256-hashed all **8,627 Git blobs** at final commit using git cat-file and compared with FINAL-CHECKPOINT.json: exact match.
- GREEN02, actual03, full04, strict05 and fmt06 source maps exactly equal all final Git blobs, before=after. Gates ran from the earlier scaffold HEAD with final working-tree bytes; this distinction is preserved rather than asserting that Git HEAD had already advanced.
- RED01 remains unchanged within its own run and differs from final implementation, as expected: direct101, 10 failing behavior contracts, 1 existing refusal control passing, 2 explicitly ignored qualifications. It is not represented as all-new-test RED.
- GREEN02 direct0: 11 ordinary adapter tests passed; 2 qualifications explicitly ignored.
- Actual03 direct0: 2 actual qualification tests passed, 0 ignored, all five positive family log lines present.
- Full04 direct0: 68 unit tests plus 3 integration tests passed; 2 ignored qualification tests were separately executed in03. This is 71 ordinary full-suite passes, not 73 ordinary tests.
- Strict05 direct0 (release all-target Clippy, -D warnings); fmt06 direct0. Existing vendor native warnings are retained in logs and not described as a warning-free native build.
- All six gates have equal before/after source maps and five fixture hash/length maps. Independently rehashed the five current fixture originals read-only; exact equality with all recorded gate maps. Commands retain the specified BetterSSD target, deployment15 and jobs2.

## Acceptance limits

Approved ownership adapter does not establish an atomic source snapshot, RAW+recipe/dependency binding, normalized/demosaiced/rendered output, persistent source availability, a native/decompressed allocation bound, interruptible native calls, universal format support or non-Unix support. No public consumer integration, native archive or generated binding work is claimed. Existing staged-byte quota does not bound LibRaw/Rust decoded buffers. Any broader product caller must separately satisfy those requirements.

No main merge or source modification performed by reviewer.
