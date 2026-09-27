# M2-45d export audit

Audited 2026-09-27 at `wp/M2-45d` head `49d8e0f` in `/Users/rutmehta/Developer/tessera/.worktrees/M2-45d`. Initial tracked status clean. Read the brief, DNG/HDR/METADATA/METADATA-PROGRESS checkpoints, four-commit diff, metadata/parser/export paths, relevant tests, and gain-map diagnostics. No heavy build or source mutation occurred during the audit. Follow-up authorization now permits a narrow keyword fix; its validation status is separate below.

## Finding requiring correction

**[P2] Explicit sidecar keyword deletion is undone by native IPTC merging.** `crates/export/src/lib.rs:912–916` first applies `with_sidecar_overrides`, then unconditionally calls `with_native_keywords`. The latter (`crates/sidecar/src/export_policy.rs:115–145`) unions old native keywords and synthesizes hierarchy paths. Thus an explicit empty `dc:subject`/`lr:hierarchicalSubject` in a sidecar can reacquire deleted keywords from the original's IPTC. `Native::filter` (`crates/export/src/native.rs:517–525`) only reconciles keyword membership when person removal is enabled, so ordinary exports also keep stale native keywords. This contradicts `with_sidecar_overrides`' documented explicit-empty replacement contract and its existing empty-title regression. Fix by merging native source context before applying explicit sidecar properties and reconciling retained native keywords with the final filtered XMP. Preserve absent-versus-empty behavior and unrelated rights/contact/qualified values. Tests must cover absent, empty, replacement, partial-property overrides, and native carrier readback without relying on person filtering.

No additional confirmed DNG/PQ/HLG correctness defect emerged from this bounded review. This is not proof of universal interoperability. I did not promote speculative issues (legal unusual JPEG marker padding, TIFF IPTC LONG byte-order variants) into findings without a targeted reproduction.

## Evidence quality and limits

The committed `native-verified-gate.log` independently sums to **552 passed / 0 failed / 18 ignored**, ends `GATE_EXIT=0`, and includes the late segment-limit regression. It records the required Rust/Clippy/fmt/licenses/workspace/binding/Swift gate; Swift ends at 8.81 seconds. No gate was rerun during this audit. Earlier failed/interrupted logs are correctly distinguished in final METADATA.md.

DNG has bounded streaming original embedding, descriptor validation, version/tag audit, independent ExifTool extraction, and LibRaw color comparison. Native metadata tests exercise 240 policy/privacy/hierarchy exports and format input roundtrips. PQ/HLG tests inspect carrier signaling and independently invert file-decoded samples; AVIF pixel decoding is macOS-specific and no independent AV1 sequence-header parser is present.

METADATA-PROGRESS.md is a historical worker handoff: its “uncommitted / parent gate still required” text is stale relative to METADATA.md and the later committed gate. Treat METADATA.md as final evidence. Non-TIFF proprietary RAW metadata extraction, BigTIFF, Extended XMP, and compressed JXL metadata remain documented boundaries; the broad brief is not universally fulfilled. Gain-map JPEG remains absent from source and host controls.

## Gain-map diagnosis

The removed encoder and its test can be recovered as text from the committed `codex-implementation.log` around lines 10248–10406. It generated an sRGB JPEG base, full-resolution grayscale logarithmic gain JPEG, ISO APP2 metadata and MPF offsets. Its independent numerical reconstruction and ExifTool MPImage2 extraction executed before the failing ImageIO assertion. The test asserted `CGImageSourceCopyAuxiliaryDataInfoAtIndex(...ISOGainMap)` was non-null; this alone is not a pixel interoperability test.

The later manual diagnostics are stronger: original/ISO-only variants produced no provider pixel data for either requested HDR or SDR decoding; removing ISO metadata restored SDR pixels. ISO headroom recognition therefore does not establish successful decoding. Moving the ISO marker, dropping Adobe compatibility XMP, and removing MPF did not establish a working ISO output. The reference `kCGImageDestinationEncodeToISOGainmap` encode returned false; adding explicit content headroom and enlarging dimensions did not fix it. A comparable SDR request returned true. The attempted remote reference download failed; no accepted independent known-good ISO file is documented.

The existing `/tmp/tessera-gainmap-diagnostic.jpg` remains on disk. Fresh read-only `exiftool -validate -warning -a` returned `Validate: OK` during this audit. That validates ExifTool's structural checks, not ISO conformance, Apple pixel decoding, or Adobe interoperability.

Apple publicly describes both HDR decode requests and writing a supplied SDR base plus gain-map dictionary via `CGImageDestinationAddAuxiliaryDataInfo` with `kCGImageAuxiliaryDataTypeISOGainMap`. Its dictionary contains pixels, dimensions/format, and CGImageMetadata describing gain conversion. This supports a second reference-construction route; it does not show that the failed custom prototype is valid. [Apple WWDC24 HDR session](https://developer.apple.com/videos/play/wwdc2024/10177/), [ImageIO encode request API](https://developer.apple.com/documentation/imageio/kcgimagedestinationencodetoisogainmap).

## Smallest next gain-map work item

Build a **capability and reference test first**, without enabling a new export option:

1. Record OS/SDK/runtime, destination types, full decode options, non-null buffers and errors in a small standalone diagnostic. Use correctly scoped/owned pixel storage and explicit content headroom. Never force-unwrap failed reference creation.
2. Establish one known-good ISO JPEG from an independently verified encoder or licensed fixture. If obtaining a new dependency/fixture requires download or installation, that is a separate permitted step, not part of this audit.
3. On the same process/host, verify plain JPEG, known-good ISO SDR decode, known-good ISO HDR decode, and the Tessera prototype. Compare actual rendered extended-linear pixels, not just headroom or auxiliary presence.
4. If native implicit gain-map generation still fails, try Apple's documented explicit SDR+gain auxiliary-data reference path. Keep its output and a parsed metadata/MPF audit. Do not skip a failing interoperability check based only on the failed reference attempt.
5. Once a valid reference establishes the host's capability, repair the smallest mismatching prototype field/association and require an independent decoder plus reconstruction tolerance. Start with sRGB SDR base, one full-resolution grayscale gain plane, fixed quality, positive recipe headroom, and the existing unsupported-feature checks. Only then wire CLI/FFI/MCP and restore the full gate.

The actual blocker is the absence of a successful independent ISO HDR control on this host, coupled with no pixel decode of Tessera's ISO output. It is unresolved whether environment support, reference API usage, or bitstream construction causes the failure. The brief says ImageIO decode “where supported”; a capability-based skip must be established by a valid control, not inferred from the encoder under test failing. No new library or speculative ISO implementation is justified in the keyword fix.

## Authorized follow-up

Parent authorized the narrow sidecar/native keyword correction only, preserving source ownership and all evidence. Pre-change tracked diff is backed up at `/tmp/tessera-M2-45d-pre-keyword-fix.patch` (empty because starting tracked tree was clean). A small standalone test against the already-built sidecar library is being used for the red check; full export regression and Cargo validation wait for the parent's heavy-build slot. No commit until validation passes; no merge/push or host changes.

### Follow-up implementation checkpoint

Confirmed red with `/tmp/tessera-keyword-override-red.rs` linked against the existing sidecar rlib: the old export composition yielded `["Old"]` after explicit empty sidecar properties. Reordering the same real sidecar API calls passed (`/tmp/tessera-keyword-override-green.rs`). These are lightweight composition checks, not the export integration test or rebuilt library validation.

Prepared changes are confined to `crates/export/src/lib.rs`, `crates/export/src/native.rs`, and `crates/export/tests/native_metadata.rs`. Reconcile native source keywords before applying sidecar overrides; then retain native IPTC keywords only when present in final flat XMP. New sidecar keywords stay in XMP; this fix does not rewrite all native text encoding to add native duplicates. Tests explicitly expect original native values still present to retain their encoding/bytes.

Chosen partial-property semantics follow the existing expanded-name contract: an explicit empty dc:subject clears flat XMP and native IPTC keywords, but absent lr:hierarchicalSubject retains source hierarchy; an explicit empty hierarchy remains empty while absent flat subject/native entries survive. Source-derived hierarchy is constructed before sidecar overrides, so it cannot recreate a cleared property afterward. Regression matrix has five cases across six formats and independent ExifTool native/XMP carrier readback, source-byte preservation, rights/contact preservation, and person removal disabled. Formatting and diff checks pass. Cargo tests and commit remain pending the parent-controlled build slot.

### Validation progress after build slot grant

Targeted command: `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M2-45d cargo test -p export --test native_metadata --test native_review_regressions --release`. First run failed because ExifTool reports an explicit empty Bag as JSON `""`; the reader now treats only that empty representation as zero values. All nonempty keyword lists are still compared exactly. Retry passed 12 native-metadata and 4 review-regression tests. Logs: `/tmp/tessera-keyword-export-tests.log`, `/tmp/tessera-keyword-export-tests-retry.log`.

The broader command `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M2-45d cargo test -p export -p sidecar --release` then exposed `xmp_policies_and_privacy_read_back_in_every_export_format`: the sidecar-only AVIF/JXL reader could no longer find its existing XMP wrapper after an unnecessary merge with synthetic source context. Production was corrected to return the caller's sidecar directly when there is no embedded XMP or native keyword context. No existing assertion was weakened. Failure log remains `/tmp/tessera-keyword-export-suite.log`.

The complete rerun passed **149 tests, 0 failures, 7 ignored**, including the existing wrapper test and the new 30-output keyword matrix. Log: `/tmp/tessera-keyword-export-suite-retry.log`. `cargo fmt --check` and `git diff --check` passed. Strict `cargo clippy -p export -p sidecar --all-targets -- -D warnings` is the final pending build check at this checkpoint.

### Final narrow-fix outcome

Committed as `69bcd3e` (`Fix explicit sidecar keyword overrides in native exports`) on `wp/M2-45d`. Only three export files changed; no host/FFI changes, source fixture downloads, gain-map changes, main merge, or push. Final tracked worktree is clean.

Final validation all passed:

- `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M2-45d cargo test -p export --test native_metadata --test native_review_regressions --release`: 16 passed, 0 failed (targeted retry before the sidecar-only compatibility fix; the final suite reran these).
- `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M2-45d cargo test -p export -p sidecar --release`: **149 passed, 0 failed, 7 ignored** on final source.
- `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M2-45d cargo clippy -p export -p sidecar --all-targets -- -D warnings`: exit 0, 20.39 seconds. Existing vendored LibRaw build-script warnings remain; no suppression added.
- `cargo fmt --check` and `git diff --check`: exit 0 on final source before commit.

Heavy-build slot released to parent immediately after the test/Clippy process exited. Full five-host/Swift WP gate was not rerun for this narrow internal export change and is not claimed. Gain-map interoperability and documented unsupported-container boundaries remain unresolved as above. Parent supplied two pinned upstream Apple gain-map control files separately under `/tmp/tessera-gainmap-reference-control`; this worker did not decode them or treat them as ISO proof.
