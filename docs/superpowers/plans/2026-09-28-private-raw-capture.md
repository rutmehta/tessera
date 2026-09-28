# Private RAW Captured-Stream Ownership Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. No subagents or runtime work without root assignment; FFI owns the current compiler lane.

**Goal:** Implement a bounded ephemeral RAW byte-capture owner whose verified digest names the private bytes subsequently offered to a controlled consumer.

**Architecture:** A raw-decode capture pool reserves disk allowance and a live-stage slot before reading. It copies one opened regular-file source into a private exclusively created stage, closes writable ownership, re-reads that stage to compute its domain-separated identity, and returns a non-cloneable owner. Descriptor parsing, catalog lookup, recipe capture, actual decoding and rendering remain outside this primitive.

**Tech Stack:** Rust2024, std filesystem/I/O/synchronization, existing engine-api CancellationToken/Digest/EngineError, workspace tempfile3 and blake3 1. Existing locked libc0.2.189 supplies Unix nonblocking open flags; no new decoder binding or package version required.

**Spec:** `/Users/rutmehta/Developer/tessera/docs/coordination/LIVE-RAW-CAPTURE-POLICY.md`; `/Users/rutmehta/Developer/tessera/docs/coordination/LIVE-RAW-PINNED-SOURCE-PROPOSAL.md`. Prerequisite descriptor `codex/pinned-raw-descriptor` at083018a967d2ba1b07570824e885baca1e9d4314 is already integrated; do not rebuild its schema boundary.

## Global Constraints

- Source-only plan, no implementation/build/GUI/B adapter changes in this task. Root reviews this plan before execution; Smart Preview/Save As retain priority.
- Captured RAW and recipe are independent inputs, never an atomic RAW+recipe transaction.
- Captured bytes may be a mixed stream from concurrent in-place writes. Their digest identifies the frozen staged stream, not a guaranteed source state at one instant. Metadata checks detect some observed changes only.
- All future probe/metadata/unpack reads must use the held capture owner. No fallback to the original path after capture or mismatch.
- Ephemeral stages are not durable reopen storage, not rendered cache, and not a public locator. No document schema, dependency admission, ICC, pixel fidelity or speed claim.
- Limits must be configured explicitly. No implicit unlimited policy, guessed process memory budget, or unbounded RAM duplicate.
- Existing generic EngineError is the only error crossing crate boundaries. Use its typed ResourceExhausted/Conflict/Cancelled/Io/InvalidArgument variants; do not add serialized engine-api variants or version changes in this slice. A later resolver can map these into its richer public outcomes.
- Private directory ownership is cooperative process isolation, not protection against privileged/same-user malicious namespace mutation. No arbitrary hostile-directory CAS guarantee.

## Review Focus

1. A source growing after metadata inspection must exceed the streaming limit without reading/writing an unbounded tail (Task1).
2. Quota and slot charges must survive cleanup failure and concurrent capture, including failure before ownership transfer (Task1/2).
3. Original replacement after freeze must not redirect any consumer read, including delayed reads (Task3).
4. Cancellation/read/write/hash errors must never invoke consumer or silently use original bytes (Task2/3).
5. Renamed original suffix, digest mismatch, and a mixed-read source must not create false route or atomic-snapshot claims (Task2/3).

## Scope and file map

- Create `crates/raw-decode/src/capture.rs`: limits, pool, reservation/owner lifetime, staged copying and verification; private nested helpers.
- Create `crates/raw-decode/src/capture/tests.rs`: deterministic unit tests with private injectable I/O/cleanup/consumer seams.
- Modify `crates/raw-decode/src/lib.rs`: `pub mod capture;` only; existing RawSource/open/decoder behavior unchanged.
- Modify `crates/raw-decode/Cargo.toml`: `tempfile.workspace = true`, `blake3.workspace = true` dependencies. They are existing workspace dependencies. Add target cfg(unix) dependency `libc = "0.2"` (already locked0.2.189) for O_NONBLOCK. Cargo.lock updates only if normal Cargo requires it.
- No engine-api changes/getters: primitive accepts explicit capture route/suffix/optional expected identity. Do not deserialize/rebuild the already validated descriptor inside this slice. Caller integration later will add narrowly reviewed descriptor accessors if needed.

## Concrete public interface

```rust
pub const ASSET_DOMAIN: &str = "tessera pinned RAW asset v1";
pub const CHUNK_BYTES: usize = 64 * 1024;
#[derive(Clone, Copy, Debug)]
pub struct CaptureLimits {
    pub max_asset_bytes: u64,
    pub max_staged_bytes: u64,
    pub max_live_captures: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapturedAssetIdentity {
    pub digest: engine_api::id::Digest,
    pub byte_len: u64,
}
pub struct CapturePool { /* Arc<PoolInner>, private fields */ }
pub struct CapturedRaw { /* private TempPath, readonly File, reservation, identity, route */ }
impl CapturePool {
    pub fn create(parent: &std::path::Path, limits: CaptureLimits)
        -> engine_api::EngineResult<Self>;
    pub fn capture(&self, source: &std::path::Path,
        route: engine_api::pinned_raw::PinnedRawDecoderRoute, suffix: &str,
        expected: Option<CapturedAssetIdentity>,
        cancel: &engine_api::jobs::CancellationToken)
        -> engine_api::EngineResult<CapturedRaw>;
}
impl CapturedRaw {
    pub fn identity(&self) -> CapturedAssetIdentity;
    pub fn route(&self) -> engine_api::pinned_raw::PinnedRawDecoderRoute;
    pub fn suffix(&self) -> &str;
    pub fn close(self) -> engine_api::EngineResult<()>;
}
```

Use engine_api::pinned_raw::PinnedRawDecoderRoute::LibRawCfaV1 (module export verified). Do not offer Clone on CapturedRaw, writable file handles, path getters, or `into_temp_path`. Pool may implement Clone to share one accounting authority. `create` requires an existing parent directory; creates one unique private TempDir beneath it, permission0700 on Unix and stages0600. After successful permission setup, disable TempDir automatic cleanup and transfer it immediately into PoolInner, whose explicit Drop owns teardown; any setup failure uses one explicit cleanup attempt and reports its error alongside the primary failure. The caller chooses trusted app staging parent; this does not adopt arbitrary pre-existing stages. Initial capture implementation is Unix, including the supported macOS host; non-Unix capture returns Unsupported before creating a stage until its nonblocking regular-file admission is separately implemented. Do not use a weaker generic open fallback.

Concrete policy example for a future caller (not a silent library default): `max_asset_bytes=512*1024*1024`, `max_staged_bytes=1024*1024*1024`, `max_live_captures=2`. Tests use64-byte/128-byte budgets. Reject zero limits and assetlimit>stagedlimit. A reservation charges the entire max_asset_bytes, not metadata length, until stage removal: conservative, simple, immune to source growth. Checked-add all accounting. Slot/byte exhaustion returns ResourceExhausted immediately; no waiting queue. At most max_live_captures stage owners (copying, sealed, or quarantined) coexist. Pool creation itself is not a system-free-space guarantee; ENOSPC is a normal Io failure.

## Task1: Accounted private stages and failure cleanup

- [ ] Add public interface and tests first. Record compile-only missing-symbol failures as such, not behavioral RED. Once minimal constructors compile, reproduce meaningful quota/cleanup tests before filling the protocol.
- [ ] Use `Arc<PoolInner>` containing TempDir, immutable limits and Mutex<Accounting>; Accounting has chargedbytes, chargedslots, and bounded quarantined TempPaths with automatic cleanup disabled. Obtain reservation under mutex, release mutex before all I/O. Reservation owns an Arc so the directory cannot disappear while a stage is held.
- [ ] Before reserving, call cancel.check; under lock use checked_add and reject limits. Allocate stage with `tempfile::Builder::new().prefix("raw-").suffix(&format!(".{suffix}")).tempfile_in(pooldir)`; private builder validates suffix first. Immediately call NamedTempFile::disable_cleanup(true) and move it into a private StageGuard containing the reservation before any fallible copy/sync operation. StageGuard owns either the writable NamedTempFile or sealed TempPath/read-only handle. Its explicit cleanup first closes handles and converts NamedTempFile with into_temp_path (which preserves the disabled-cleanup flag), then follows the removal protocol below. Reservation Drop returns counts only when no stage was created or removal succeeded.
- [ ] Use actual tempfile API semantics: TempPath::close consumes the path and returns io::Result<()>, so it cannot support recovery and must NOT be used here. Keep the private TempPath with disable_cleanup(true), close all owned file handles, then call std::fs::remove_file(&path) while retaining that owner. Success or NotFound means absence and releases its charge exactly once; dropping the disabled TempPath causes no second deletion. On any other error, move the still-owned disabled TempPath plus numeric byte/slot charge into bounded quarantine, then disarm the reservation without decrementing accounting. Subsequent admissions fail closed while that capacity remains charged. No retry loop in CapturedRaw/StageGuard Drop. CapturedRaw::close returns the deletion error; Drop uses a private best-effort diagnostic helper implemented with writeln!(std::io::stderr().lock(), ...) and ignores write failure, never println!/unwrap or a callback under the accounting mutex. Preserve the primary copy/consumer error and report a secondary cleanup error through that helper. Avoid a reservation->pool->quarantine->reservation Arc cycle: quarantine stores path and numeric charge, never a Reservation owning Arc<PoolInner>.
- [ ] PoolInner::drop runs only after all live stage reservations have released their Arc. It explicitly takes quarantined entries, tries remove_file once per entry (NotFound accepted), reports each remaining error through the same diagnostic helper, then calls remove_dir (NOT remove_dir_all) once on its private directory. TempDir automatic cleanup stays disabled, so failure does not trigger an unreported recursive second attempt. An undeletable stage/directory may remain after process teardown: this is reported best-effort, not returned from Drop or guaranteed removed. This final bounded retry is distinct from stage close and occurs only at pool teardown; no crash scavenger or universal cleanup promise. Test helpers capture diagnostics through a private test-only sink, with no public callback API.
- [ ] Deterministic tests use private per-call cleanup function injection, not globals. Force unlink failure; assert next capture remains ResourceExhausted despite owner Drop. Two independently held captures consume both slots; third fails before source read. Successful close releases exactly once. Test create failure releases reservation and leaves no stage. Count injected remove calls to prove stage failure attempts exactly once before quarantine and only one further attempt at pool teardown; test NotFound releases charges, failed pool removal emits diagnostics, and disabled TempPath/TempDir drops perform no hidden retry. Assert own TempDir mode0700 and stage0600 on Unix.

Representative test contract:
```rust
let pool = pool_for_test(64, 128, 2);
let a = capture_bytes(&pool, b"a");
let b = capture_bytes(&pool, b"b");
assert_resource_exhausted(pool.capture(&source, route(), "arw", None, &token()));
a.close().unwrap();
let c = capture_bytes(&pool, b"c");
drop((b, c));
assert_eq!(pool.accounting_for_test(), (0, 0));
```
`pool_for_test`, `capture_bytes`, `token`, `route`, and `assert_resource_exhausted` are test helpers in capture/tests.rs: create temp parent/write tiny file; construct CancellationToken::new; select LibRawCfaV1; match EngineError::ResourceExhausted. No fake quota accounting in production assertions.

- [ ] After lane grant run focused capture tests, then commit this independently reviewable ownership unit with no original decoder change.

## Task2: Bounded stream, sealed ownership and verified identity

- [ ] Validate suffix before disk writes: lowercase ASCII alphanumeric1..16, no separators or dot; stored route is closed enum. Normalize uppercase once, exactly like descriptor. Use stored suffix even if source basename has another extension.
- [ ] On Unix open source once with `OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(source)`, then fstat require regular file and positive length before any read; O_NONBLOCK avoids waiting for a FIFO producer and has no effect on regular-file reads. Keep original symlink-following behavior because the locator may legitimately be a symlink; compare opened inode against path metadata at admission/end. No directory/FIFO/device stream is captured. Metadata length>limit rejects early. Reserve quota before copy. Capture best-effort before/after handle metadata and path metadata identity; on Unix compare dev/ino/len/mtime+nsec/ctime+nsec, on other targets documented available metadata. Observed changed/missing path returns Conflict. Never claim complete detection of in-place writers or hard wall-time cancellation of regular-file filesystem I/O: cancellation is cooperative between bounded operations. Test a FIFO with no writer and a directory; both reject before byte reads, with no staged file publication.
- [ ] Internal core reads at most min(CHUNK_BYTES, limit+1-total) bytes each iteration, checking cancellation before/after read and before write. Retry Interrupted; zero means EOF. If total+n>limit reject without writing excess. EmptyEOF rejects InvalidArgument. Write_all with checked offsets, cancellation between bounded chunks. No read_to_end. Checked limit+1 overflow: reject limits==u64::MAX at construction.
- [ ] sync_all completed stage, close the writable handle using NamedTempFile::into_temp_path; reopen private stage read-only. Re-read complete stage with the same bounded buffer and cancellation, hashing with `blake3::Hasher::new_derive_key(ASSET_DOMAIN)`. Compare actual length to copied length. This completed-stage digest is authoritative, not a digest of caller declarations or original stat data. No writable handle escapes or stays alive after sealing.
- [ ] Verify optional expected digest AND byte_len before returning CapturedRaw. Mismatch returns EngineError::Conflict; caller descriptor remains unchanged. No decoder/consumer call exists on failure. On successful return retain read-only stage descriptor plus TempPath and reservation; no file-lock claim.
- [ ] Tests: exact `Digest::derive(ASSET_DOMAIN, bytes)` for0/1/chunk−1/chunk/chunk+1 sizes (0rejects), limit andlimit+1, source grows pastlimit, injected short reads/Interrupted/readerror, writeerror, syncerror, hashreaderror, cancellation at copy and hash barriers, same-size mismatch and different length, uppercase suffix normalization and invalid suffix no stagecreated. Private reader/writer traits or closures live only in module; tests invoke real accounting/staging, replacing only the failing operation.
- [ ] Mixed-stream test supplies deterministic reader whose first half comes fromA and second fromB with unchanged synthetic metadata. Expect success naming Digest::derive(domain,Afirst+Bsecond), not SourceChanged. This pins honest captured-stream semantics. Separate observed-metadata-change test expects Conflict and no returned owner.
- [ ] Gate focused tests and full raw-decode Release/strict/fmt, commit bounded identity implementation after review. No real RAW fixture needed: capture handles bytes, not format decoding.

## Task3: Held consumer seam and scope documentation

The production API deliberately does not expose a raw stage path or actual decoder yet. Add a private `#[cfg(test)]` consumer seam exercising the same owner/path lifetime intended for the later controlled raw-decode adapter:
```rust
#[cfg(test)]
impl CapturedRaw {
    fn consume_for_test<T>(self, cancel: &CancellationToken,
        consumer: impl FnOnce(&Path, PinnedRawDecoderRoute) -> EngineResult<T>)
        -> EngineResult<T> {
        cancel.check()?;
        let result = consumer(self.stage_path_private(), self.route());
        // Owner stays alive through all consumer reads. Cleanup error wins only
        // if consumer succeeded; otherwise preserve primary error + log cleanup.
        let cleanup = self.close();
        match result { Ok(value) => { cleanup?; Ok(value) }, Err(e) => { report_cleanup(cleanup); Err(e) } }
    }
}
```
Private helpers `stage_path_private` and `report_cleanup` belong to this test-only seam; do not ship a generic callback that can return a PathBuf or lazy decoder while pretending T:'static proves decoded ownership. The later production adapter must be a closed implementation returning fully owned RawImage/CFA+metadata, or explicitly transfer CapturedRaw into a lazy-decoder owner. That adapter requires its own review/tests and is not added here.

- [ ] Same-path replacement: captureA, rename original/recreateB, invoke consumer barrier; every staged read returnsA and digestA. Verify originalB unchanged and stage removed on completion.
- [ ] Delayed consumer: thread moves CapturedRaw into consume_for_test; channel barrier blocks inside consumer, not sleep. Assert charge/path remain until release and then cleanly disappear. Hold no external cloned file/path beyond the test probe. Separate error/panic-unwind controls prove RAII stage retention then cleanup; no consumer on digest mismatch/cancel.
- [ ] Route preservation: original location ends.bin, requested stored suffixARW/LibRawCfaV1; consumer sees private.arw and explicit route. No decoder call or format validity claim.
- [ ] Document distinction: `CapturedRaw` means verified ephemeral bytes under held ownership; future `PinnedRawDecodedInput` means owned decoded pixels/metadata plus separate immutable recipe snapshot. Descriptor or digest alone is never proof of bytes/decoder success. Pinning catalog/recipe gate remains future FFI integration.
- [ ] Final review, freeze source and run final gates serially. Commit only qualified raw-decode component and dependency wiring. No merge by implementer; root integrates.

## Commands after explicit compiler grant

Use the assigned worktree, never main's active checkout. Every Cargo command must explicitly include:
```sh
CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/depth-histogram-readonly-77eb68d0-relocated MACOSX_DEPLOYMENT_TARGET=15.0 CARGO_BUILD_JOBS=2 cargo test -p raw-decode --release capture::tests
CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/depth-histogram-readonly-77eb68d0-relocated MACOSX_DEPLOYMENT_TARGET=15.0 CARGO_BUILD_JOBS=2 cargo test -p raw-decode --release
CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/depth-histogram-readonly-77eb68d0-relocated MACOSX_DEPLOYMENT_TARGET=15.0 CARGO_BUILD_JOBS=2 cargo clippy -p raw-decode --release --all-targets -- -D warnings
CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/depth-histogram-readonly-77eb68d0-relocated MACOSX_DEPLOYMENT_TARGET=15.0 CARGO_BUILD_JOBS=2 cargo fmt --all --check
```
Preserve commands/direct exits/full tracked+new source manifests, before/after hashes and failures under a new BetterSSD validation directory. Existing opt-in decoder tests may skip without fixtures; report exact counts and do not call them exercised. No GUI, original-photo mutation, stress allocations or real decoder execution is required by this primitive plan.

## Self-review and handoff

The plan covers ephemeral storage ownership, conservative quota/concurrency, bounded byte reads, completed-stage digest, cancellation, failure cleanup, route preservation and held-consumer tests. It deliberately defers catalog/recipe admission, descriptor resolver/accessors, actual decoder/environment identity, dependency bundle/render admission, durable store/crash scavenging and B adapter work. The only consumer seam is test-only until a closed owned-output decoder adapter exists. No rendered-image or memory/RSS guarantee follows.

Root review should accept the conservative full-limit reservation and quarantine-on-cleanup-failure policy before implementation. This document proposes no runtime action while FFI owns the lane and does not supersede Smart Preview/Save As priority.

Root review: approved the bounded engine-only implementation scope and conservative reservation/quarantine policy after correcting the tempfile cleanup API and explicit teardown diagnostics. Diagnostic writes in Drop must ignore their own I/O failures rather than panic. Implementation remains queued behind Smart Preview and Save As, with a separately assigned safe checkout and serialized runtime lane. No product or decoder acceptance is implied.
