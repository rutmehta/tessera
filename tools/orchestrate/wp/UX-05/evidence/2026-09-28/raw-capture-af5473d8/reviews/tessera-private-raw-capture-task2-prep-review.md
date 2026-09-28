# Private RAW capture Task2 test preflight

Source-only review of18 synthetic stream tests and cfg(test) signatures against approved Task2 plan. No builds, source edits, CUA or runtime. Task1 remains reviewed; public capture still Unsupported and the new private hook method delegates to it. No streaming implementation or passing test claim.

## Sound contracts already authored

Tests cover public exact-domain identity/length at chunk boundaries, uppercase suffix and explicit route, readonly descriptor mode plus charged owner, empty/exactlimit/oversize, invalid suffix before nonexistent-source lookup, source growth limit+1 without excess write, short read/write and Interrupted source read, copy/write/sync/reopen/hash errors, authoritative completed-stage mutation, copied-length disagreement, cancellation after copy/hash read, both expected identity fields, observed replace/remove/grow conflicts, legitimate symlink versus directory/FIFO admission, and honest unchanged-metadata mixed stream.

The readonly-reopen+unlink failure test is particularly important: it asserts primary reopen error, secondary cleanup diagnostic, retained exact sealed bytes, charged quota, one failed unlink and one final pool retry. It prevents losing cleanup authority when converting NamedTempFile into disabled TempPath. Implementation must install sealed path into the same StageGuard before any fallible readonly reopen; no temporary unowned path or disarmed reservation is acceptable.

The mixedstream hook substitutes only the byte reader while real source metadata remains stable; expected AABB digest is an honest injected stream oracle, not an atomic-source claim. No public generic callback/decoder or fallback-original claim is introduced.

## Concrete remaining test gaps

1. **Hash growth after admission:** stage_hash_read_is_bounded_independently_of_copied_source enlarges the stage in after_copy and explicitly allows zero hash bytes. A stat-only rejection could pass even if the subsequent hash loop reads an unbounded tail. Add a hook that grows the stage on the first hash_read, after readonly metadata admission, and record consumed bytes/request size. Require <=limit+1 and no owner. This tests the actual streaming bound independently of early length checks.
2. **Same inode, same length observed mutation:** replacement/remove/grow can be rejected using only inode and length, leaving the planned mtime/ctime+nsec checks untested. Add an after-copy same-file same-length write with deliberately different metadata timestamp (explicit timestamp setting where supported; avoid sleeps) and assert Conflict/cleanup. Keep the unchanged-metadata mixedstream success as its distinct negative control.
3. **CHUNK_BYTES ceiling:** current public chunk-boundary cases prove data integrity, while injected bounded cases use limit64 and only check buffer<=65. A loop allocating/requesting limit+1 for a large configured limit could pass these. Add source and hash hooks with limit>CHUNK_BYTES and enough bytes for multiple calls, asserting each buffer<=CHUNK_BYTES and complete identity/cleanup.

Useful additional retry/error coverage: zero-length stage write must produce Io without spinning; Interrupted write should retry without truncation; Interrupted hash read should retry and observe cancellation. Current short-write/source-Interrupted cases do not establish these separate branches. These can be tests-first additions before observed RED; do not implement product prematurely just to satisfy review.

## Implementation review obligations not satisfied by scaffold

All hooks must call the same production capture core and retain real reservation/metadata/sealing; test-only alternative ownership would invalidate coverage. Recheck nonblocking regular-file admission, source-handle/path metadata before and after, checked arithmetic, cancellation between every bounded operation, closed writer before readonly seal/hash, no stage publication on any error, and preservation of primary error if cleanup fails. A readonly accessor returning O_RDONLY alone does not prove no other writer handle was retained; inspect the final StageGuard transition directly. No decoder/consumer integration is in this slice.

FIFO test includes a rescue writer for a regressed blocking open and joins the worker; do not interpret its two-second timeout as a filesystem cancellation guarantee. Source suffix normalization is route metadata, not proof the bytes decode as ARW.

## Reviewed source hashes

- `crates/raw-decode/src/capture.rs`: `aae7c72348f860f58e199a2d1b26020040cc161347457d0e9e66f48349f202c9`
- `crates/raw-decode/src/capture/tests.rs`: `a434e19d89e2b20069900aa2dbadc18bba509874bdd1ef02b89b4befb9ed3b1b`
- `crates/raw-decode/src/capture/tests/stream.rs`: `6b439b092ee647b745c771999c2d4fdd63c980b003ef6d67763fccd8c589291e`

## Revised23-test source follow-up

Inspected five added tests; the three identified preflight gaps are satisfied at source-contract level. Growth now occurs inside the first readonly hash read after its admission, requires exactly65 consumed bytes at limit64, and rejects without an owner. Same-file overwrite explicitly asserts unchanged inode/length and deterministically changes mtime100→200; Conflict and cleanup are required. A >2chunk payload with4chunk allowance asserts every source/hash request is nonempty and <=CHUNK_BYTES, both streams make at least3calls, and exact identity survives. Additional zero-write Io/cleanup and Interrupted writer/hash reread retry controls retain exact bytes/digest.

No compile/runtime success inferred; Task2 implementation and observed RED/gates remain pending. No builds or source edits by reviewer. Revised stream.rs SHA-256: `eaa975ab563e411fa7ef11cf6296c7fe649fc2919f2a5118701292e9d218a614`.
