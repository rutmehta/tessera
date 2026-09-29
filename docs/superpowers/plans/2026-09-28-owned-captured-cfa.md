# Opaque captured-CFA post-decode owner

Status: approved interface, SOURCE-ONLY tests/Unsupported scaffold. Base55a241eedaf98633818a282651466003ff504971; qualified normalization2d7c00fa remains on codex/raw-normalization. No runtime/implementation authorization at preparation.

## Frozen narrow interface

Add CapturedRaw::decode_owned_cfa(self, &CancellationToken)->EngineResult<OwnedCapturedCfa>. OwnedCapturedCfa privately owns CfaU16, RawMetadata, CapturedAssetIdentity and PinnedRawDecoderRoute. Only identity()/route() copied values, metadata()->&RawMetadata and plane_facts()->CfaPlaneFacts are public. Facts have width,height,layout,sample_len,sample_capacity. They are descriptive copies, not authority or allocation permits. No public constructor, From/TryFrom, Default/Clone/Deserialize, mutable borrow, Deref, samples getter, parts projection or normalization/render method. No Debug dumping pixels.

Old decode_cfa/public mutable DecodedCapturedCfa stays compatible. Its public fields are untrusted input if a caller supplies them; no inverse conversion into the opaque owner. Capture identity describes sealed copied bytes, not an atomic original snapshot. Completed route describes successful closed CFA classification/decode, not calibration/environment/render eligibility. XTrans remains supported by the decoder; no narrower image-core policy is introduced here.

## Task1: tests/scaffold and meaningful RED

- Expose opaque definitions and read-only accessors; decode_owned_cfa and private test decoder seam return Unsupported. Leave existing decode_closed/decode_native and old decode_cfa unchanged.
- Five protocol tests: successful captured-A/original-B flow, actual input pointer/capacity/samples + full metadata/identity after stage cleanup and pool drop; cloned metadata/opcodes do not mutate owner; private compatibility projection moves ownership. Pre-cancel and invalid native bytes refuse/release. Decoder error and dimensions/layout mismatch clean up. Unlink failure prevents success, preserves primary decode/cancel errors, retains real quarantine charge and diagnostic precedence. LinearDNG/malformed classifier prevents decoder entry.
- Synthetic closure tests prove ownership/control/error flow only; no synthetic/native fidelity conflation. Existing tests of actual native Drop/unpack/capture lifecycle remain authoritative. New opt-in five-family owner test reuses the existing exact fixture inventory and direct RawSource comparison, edits only disposable original copies, verifies actual samples through cfg(test)-only private inspection, identity/metadata and cleanup. No public sample getter.
- Real downstream positive typecheck calls the exact public API and accessors. Eight negative rustc fixtures require intended diagnostics for private fields, public-bundle conversion, Default/Clone, mutation through metadata/opcode borrow, samples and parts access. Exact imports and compiler artifacts must first pass positive control. Missing crates/imports do not count. No new dependency.
- Under a future sole-runtime grant compile then observe bounded protocol RED, distinguishing Unsupported failures from already-passing API boundaries/classifier-refusal subcases. Table tests stop at first failure. Actual fixture qualification is separately granted; no silent missing-fixture skip.

## Task2: shared-core production construction, after observed RED

- Factor existing private closed decode to construct opaque output exactly where it currently constructs public DecodedCapturedCfa. Private helper/projection only; old decode_cfa projects by moving into unchanged public fields. Both public methods use one shared classify/decode/dimension-layout/cleanup path, no duplicate open/unpack.
- Keep decoder/native lifetime inside existing scope; RawSource ends before close. Cleanup succeeds before returning either public or opaque output. Success+cleanup failure returns cleanup error and drops would-be samples; primary decode/cancel failure wins while secondary cleanup is reported/quarantined. No new cancellation guarantee inside native operations.
- New test seam uses same core and only supplies the existing narrow private decoder closure. Do not add an unrelated mock path. Private sample/projection inspection remains cfg(test), inaccessible downstream. Preserve old test seam compatibility and all existing behavior.
- Opaque owner retains no stage/pool/path/file/native handle or callback; stage disk charge releases as before. Output owns only Rust data. No float allocation/private normalize call or memory reservation claim.

## Task3: exact qualification and review

After source review/grant run protocol and downstream API gates, existing affected raw-decode full suite, explicitly qualified five-family owner test, strict all-target Clippy and fmt. Pin source/runner/oracles/exact binary+rlibs and original fixture hashes before/after, preserve every failure and compiler diagnostic. Record absent/ignored tests honestly. Reuse existing fixtures read-only; replacement only inside owned temporary copies. No renderer/GUI/GPU workload. Independent final review and root integration only.

## Stop line

No normalized owner/projection, CfaImage/RawImage assembly, captured resolver, descriptor accessors, public generic callback, image-core admission promotion, reservation/byte caps, fixedICC or environment work. A later private trusted continuation must bind descriptor expected digest AND length, declared versus classified route, exact recipe, trusted artifact/environment and fixed output profile, and own reservation through work drain. Public facts do not supply those authorities.

## Preparation ledger (UNRUN)

New branch codex/owned-captured-cfa from55a241ee; prior normalization branch/ref preserved. Existing decode.rs/native/cleanup functions and normalization untouched. Added opaque getters/Unsupported methods, five protocol tests and one explicitly ignored internal actual5family test plus an explicitly ignored downstream5family public-API test; downstream positive integration typecheck and eight compiler-negative sources with exact intended diagnostic manifest. Read-only source inspection and scoped formatting only. No behavioral RED/GREEN/compile/native claim. The first LinearDNG Unsupported subcase can pass the stub, but its malformed-classifier row must not be credited without execution. Protocol table rows after first failure remain latent. Shared-core construction/projection is intentionally not implemented.

## Implementation ledger (SOURCE ONLY, UNRUN)

Independent c0993358 RED/API review approved: compile0;5protocol Unsupported failures; downstream positive0 and8intended negative compiler diagnostics verified. Original243Clone-oracle mismatch preserved; oracle-only c099 correction accepted. Exact RED binary/productionrlibs and257dependency artifacts preserved outside sharedtarget. Later table rows and successful-owned cleanup/sample assertions were not exercised by RED.

Root authorized minimum source implementation only. Shared decode_closed now retains temporary owned payload/identity/route through the existing close call and constructs OwnedCapturedCfa only after cleanup succeeds. Existing decode_cfa and existing private test seams move-project that same core result into unchanged DecodedCapturedCfa. New decode_owned_cfa and its private test seam call the same core without projection. The private constructor/projection are capture-module-visible only; no public constructor, inverse conversion, sample or parts API. No new native open/unpack, sample/metadata clone or capture ownership retention. decode_native function, classifier/cancellation/mismatch checks and primary/secondary error policy remain unchanged; successful decode plus cleanup failure drops the local payload and returns cleanup error as before.

All protocol/API test bodies and expected diagnostics are unchanged. Only decode.rs, owned.rs and this ledger change; native code, normalization, public compatibility fields, renderer/admission and Cargo untouched. Scoped rustfmt only. Implementation not compiled or tested; await independent source review and sole runtime grant. Actual5family qualification remains separate and unrun.
