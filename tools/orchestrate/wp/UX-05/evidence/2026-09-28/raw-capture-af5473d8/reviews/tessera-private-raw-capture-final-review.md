# Private RAW capture whole-branch final review

**Approve bounded capture component af5473d8e34f4a62201fbadedad6fa8b64aed20b**, base838c8e496bf4cb5bcecf74801e7402bedababfcf. No actionable correctness or scope blocker found. No merge or runtime work performed by reviewer. Approval covers the Tasks1–3 ephemeral capture primitive and private test consumer seam; it does not approve a decoder adapter or user-facing live-RAW feature.

## Whole-branch scope

Exhaustive delta contains seven files only: Cargo.lock dependency edges, raw-decode/Cargo.toml, lib.rs module export, capture.rs and its three test files. Existing decoder source/behavior is unchanged. No engine-api serialization, document schema, FFI/Swift, public callback or pathname getter is introduced. Earlier Task1 and Task2 final reviews remain applicable: conservative reservation under mutex; real staging outside lock; disabled cleanup and bounded quarantine; same guard through seal/reopen; fixed chunk/limit+1 reads; complete-stage digest; full observed Unix source stamps; no false atomic-snapshot guarantee.

The Task2→Task3 production delta is only ownership documentation; executable additions are cfg(all(test,unix)) consume_for_test and tests. Inspected exact finaldiff. The seam checks cancellation before callback and borrows its private stage path while self remains owned. Successful callback must also clean successfully. Consumer failure remains primary; a cleanup error is separately diagnosed. A temporary pool Arc allows diagnostics after consuming self.close and cannot escape or form a cycle. Cancellation/early path error/panic all retain ordinary field Drop cleanup. Generic T remains explicitly test-only; it cannot be promoted to a production owned-decoder API just because it compiles.

## Seven consumer contracts

1. OriginalA captured, original renamed/replacedB: two private-stage reads and digest still identifyA; explicit LibRawCfaV1 and normalized.arw retained; B bytes remain unchanged and stage removed.
2. Delayed callback uses channel barrier, retains stage and charged slot through final read, rejects a competing capture, then releases. No timing sleep masquerades as ownership proof.
3. Panic after stage read is caught only by test; RAII removes stage/releases quota.
4. Cancellation after capture invokes no callback and cleans owner.
5. Expected mismatch composes capture.and_then and never supplies an owner/calls consumer. This is the pre-existing control and correctly passed RED, not a new decoder-admission API proof.
6. Consumer failure plus failed cleanup preserves primary error and diagnostics; successful consumer plus failed cleanup returns cleanup error; both keep quarantined bytes/slot until one pool teardown retry.
7. Ordinary consumer failure returns unchanged after successful stage cleanup.

These tests model a held consumer; they do not invoke LibRaw or establish format validation. No fully decoded pixels/metadata or recipe ownership is returned by product code.

## Independent evidence verification

Recomputed **8,204** final source SHA-256 values against immutable Gitaf5473d8: zero mismatches. GREEN02, full03, strict04 and fmt05 captured source maps all exactly match that commit; every before/after source map and HEAD is unchanged. Test logs and direct exits independently read:
- RED01 exit101:6failed/1existingcontrolpassed, retained separate source hashes;
- GREEN02 exit0:7consumer cases;
- full03 exit0:57unit+3integration=60passed,0ignored/0doctests, including42capture contracts;
- strict04 and fmt05 direct0.

All five existing RAW fixture before/after file records are equal and current bytes independently rehash identically. Existing decoder regression reads them; new capture tests use synthetic bytes. Task1's earlier after04-only limitation remains historical and is not rewritten. Machine-readable per-gate/hash results: /tmp/tessera-private-raw-capture-final-review.json.

## Retained limits and next boundary

This is verified ephemeral staged-stream ownership on the tested macOS host. In-place source mutation may evade best-effort metadata detection; digest identifies captured bytes, not a single source instant. Cooperative cancellation does not preempt filesystem syscalls. Cleanup failure keeps capacity charged while pool lives; undeletable files may remain after bounded teardown with best-effort diagnostics. Private directory is cooperative isolation, not defense against malicious same-user/privileged namespace mutation. NonUnix capture remains Unsupported and unqualified.

No actual decoder integration, descriptor resolver, catalog/recipe transaction, immutable recipe snapshot, durable reopen store/scavenger, ICC/fidelity/render admission, public lazy owner, B adapter, or end-user feature completion. The next closed decoder adapter must retain capture for all probe/metadata/unpack/delayed reads and return owned output or transfer owner lifetime explicitly; require a separate design/source/runtime review. Root alone integrates the accepted bounded component.
