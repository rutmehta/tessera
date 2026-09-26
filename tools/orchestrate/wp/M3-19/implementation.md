# M3-19 implementation and verification

## Delivered

- `ml-faces`: real `hdbscan` 0.12 (MIT OR Apache-2.0), precomputed cosine metric, mutual-reachability hierarchy / EOM selection, exact member medoids for each fitted cluster, singleton noise retained. Former complete-link and FFI greedy-centroid fallback removed.
- Configurable quality gate (default: confidence >= 0.9, both sides >= 32 preview pixels, sharpness >= 0.1). Excluded/missing-descriptor faces remain assignable, but cannot train clusters or become automatic medoids.
- `ml_faces::people::PeopleJob`: serialized, host-driven incremental nearest-medoid assignment; new identities for unmatched faces; periodic refit after 1000 newly observed faces, or explicit force. Named identities and confirmed assignments are protected. Merge/split invalidated medoids are reconstructed from catalog membership before matching, including members outside the active queue. Atomic index plan writes.
- Index migration 8: stable person IDs, optional names and medoids, composite face assignments, confirmation flags and covering indexes. Transactional merge/split, assignment and name joins; person predicates include indexed cluster names as well as legacy keywords.
- `cull::people::name_person`: cluster-wide joined names, synchronized library people names, optional MWG face regions and additive person keywords. Default options do no sidecar I/O. Preflight plus compensating rollback on ordinary write failures.
- MWG Regions / RegionList / Face / Area with normalized center geometry, standard namespaces and AppliedToDimensions. Alternative prefixes and RDF resource forms read correctly. Unrelated XML and non-face regions preserved. Names and regions round-trip through sidecar.
- UniFFI assist integration: stable people and face-strip identities, indexed person filtering with queue-order intersection; refresh, assignment metadata, manual assign, confirm/unconfirm, merge, split, opt-in naming and read-only name suggestions.

## Verification actually run

### Retry verification and corrections

- Re-ran the required release tests, Clippy `-D warnings`, and workspace formatting check successfully. Current output: `retry-validation.log`. LibRaw emits upstream C++ warnings; they do not fail these checks.
- Fixed stale medoids after manual face moves with transactional invalidation of both affected identities. Failed moves roll back invalidation. Added migration 9 to invalidate medoids on assignment deletion, including face re-detection and cascading image removal. Regression tests were observed failing before the fixes and passing afterward.
- Re-ran explicit ignored benchmarks: 100,000 embeddings in 1.029736792 s, 100 clusters, ARI 1.0, approximate=true. Full 100,000-image person search: 14.110167 ms unfiltered and 13.861542 ms confirmed-only. See `retry-benchmarks.log`.
- Cached model integration ran without skipping. FFI library tests (36), FFI assist tests (6), and library tests passed again. See `retry-extra.log`.
- Scope remains confined to the allowlist; no commits or engine-api edits.

Required command (final run exit 0, full output in `validation.log`):

    cargo test -p ml-faces -p cull -p index -p sidecar --release && cargo clippy -p ml-faces -p cull -p index -p sidecar --all-targets -- -D warnings && cargo fmt --check

Additional verification:

- `cargo test -q -p tessera-ffi --lib`: 36 passed, including inline persistent-people tests.
- `cargo test -q -p tessera-ffi --test assist`: 6 passed.
- `cargo test -q -p library`: passed.
- `cargo clippy -q -p tessera-ffi -p library --all-targets -- -D warnings`: passed.
- Explicit ignored clustering benchmark: 100,000 synthetic embeddings, 100 identities, ARI=1.0. Latest timed run 780.845916 ms. Earlier norm-corrected run 2.530545709 s. Both below 30 s.
- Explicit ignored indexed search benchmark returns ALL 100,000 image IDs, not just one page. Latest unfiltered 13.713667 ms; confirmed-only 12.941708 ms.
- One search timing run was 78.698667 ms and failed under a heavily loaded host (subsequently observed load averages 14.36/17.84/26.24). That failed output is retained in `benchmarks-contention.log`; repeat runs passed. The 50 ms gate is not an unconditional wall-clock guarantee under arbitrary contention.
- Cached YuNet + SFace generated multi-face integration actually ran, did not skip: two generated face crops embedded and clustered, low-confidence crop excluded. This exercises real embedding/model execution, not a claim that the detector recognizes drawn cartoons.
- Synthetic ARI, incremental-versus-batch known-identity agreement, persistent incremental/refit behavior, stable names/confirmation, rejected-face manual assignment, missing descriptors with permissive gates, merge/split, rollback, XMP roundtrip, MWG dimensions, and FFI integration covered by tests.
- `git diff --check` passed. All changed/untracked deliverable paths are within the allowlist. No commits made. All cargo builds used `/Volumes/betterSSD/tessera-cache/target/M3-19`.

## Important boundaries

1. Library-scale clustering is APPROXIMATE. For more than 1024 eligible embeddings, deterministic reservoir-sampled HDBSCAN is followed by thresholded nearest-medoid assignment and medoid recomputation. `ClusterResult.approximate` / FFI `PeopleJobResult.approximate` expose this. Rare identities absent from the sample can remain noise. This is NOT a full exact HDBSCAN hierarchy over all 100k faces; benchmark success must not be represented as such.
2. Incremental assignment uses the stored representative between refits; it is not mathematically equivalent to arbitrary future batch HDBSCAN. The agreement test is on controlled known-identity data, not a universal guarantee.
3. Periodic work is host-driven when the worker job is invoked. There is no new autonomous scheduler/thread. FFI force-refits operate on the session queue; whole-library workers should supply all catalog images. Session job counters reset on reopen.
4. XMP and SQLite are not crash-atomic together. Per-file atomic writes and compensation are implemented. Caller must serialize catalog/document edits. Keywords are additive; renaming does not remove old keywords. Replacing face entries drops extensions attached to those entries. Only normalized coordinates are supported; the service exports the oriented analysis-preview coordinate space. Actual Lightroom application import has not been exercised.
5. Face re-detection replaces detector ordinals and invalidates old assignments. Names survive in person rows, not as guaranteed matches to newly detected boxes. Empty person rows are retained. Manual split confirmations reset; confirm intended assignments to protect them during a subsequent automatic refit.

## Engine API / host fields

`engine-api` is unchanged. Needed data is currently represented in crate-local types and UniFFI records:

- composite face identity (`image_id`, `ordinal`), stable `person_id`, optional current name, `confirmed`;
- SFace medoid, quality eligibility, cluster approximation status;
- normalized face region center/size and coordinate-space dimensions;
- unnamed/named suggestion IDs, name and cosine similarity (not a calibrated probability);
- clustering options (cosine threshold, quality gate, refit interval), result assignment count/refit/approximation;
- explicit sidecar-write and person-keyword opt-ins.

An eventual engine-api consolidation should carry these fields rather than reintroducing queue-local `person-N` identities. Existing FaceChipInfo/PersonInfo layouts are preserved; additional metadata is exposed by separate FFI records. Host binding regeneration is outside this work package.
