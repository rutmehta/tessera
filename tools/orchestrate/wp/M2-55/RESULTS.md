# M2-55 — Preview cache lookup and maintenance

## Scope and implementation

P06 and P07 only. Read `tools/orchestrate/audits/perf/REPORT.md` in full before implementation. No app launched, no Swift changed, no commits made. Cargo target remained `/Volumes/betterSSD/tessera-cache/target/M2-55`.

- JPEG FFI requests now call a metadata-keyed disk lookup before opening the original or invoking a codec. Identity includes path, tier, length, device/inode, nanosecond mtime and ctime, explicit orientation, recipe hash, and a cache-format version. The post-render revision is checked before publishing. On Unix this assumes trustworthy filesystem revision metadata, as in the requested size/mtime/inode contract. Non-Unix conservatively hashes content instead; the zero-original-read performance claim is for the tested macOS path.
- RAW uses a persistent, versioned 35-byte revision-to-content-key alias. It preserves the existing content/tier/orientation/recipe keys, including compatibility with previews written by Develop. An alias hit precedes even RAW header probing or LibRaw open. Embedded/rendered provenance is retained. A missing, invalid, or evicted alias/target is a miss. A legacy cache without an alias, including a newly Develop-written legacy key, pays one cold lookup to establish it; subsequent revision-cache hits do no source work.
- JPEG miss pixels retain the previous resize, JPEG generation, orientation and color behavior. A regression test compares the complete output JPEG bytes with the old path. RAW rendering/color code is unchanged; Linear DNG tests cover calibrated upright RGB and edited-preview reopen.
- Cache bytes and eviction order are incrementally maintained in memory. Touches use a bounded 1,024-entry nonblocking queue; eviction is approximate LRU. Each maintenance batch processes at most 64 touches and 64 victims, releasing the writer-only mutex between batches. Large insertions may require multiple batches; they are not silently dropped merely because one batch was insufficient. Entries larger than the entire cap are not cached, as before.
- Readers never acquire the writer/maintenance mutex. Atomic temporary-file + fsync + rename publication prevents partial reads. Victim deletion removes both accounting/access state and empty directories.
- Startup is the only full reconciliation walk: remove abandoned temporary files, account published entries, and restore the cap before exposing the store. No journal correctness assumption is required after a crash. This intentionally moves O(F) work to startup, not puts/gets.
- Same-process handles for a canonical root share one index. A lifetime advisory file lock rejects another process trying to own the same cache, preventing conflicting accounting, temporary-file reuse, or startup cleanup during another writer. Simultaneously live handles must use the same cap.
- Optional RAW aliases are admitted only with available space, so publishing an alias cannot evict its own target preview.

## Measurements

Actual release runs on this shared macOS host, not fabricated or substituted historical results. See `environment.txt`. The host was heavily contended (load averages around 149 on the later check), so these are local observations, not clean-host speedup claims. The initial P07 after measurement ran while the FFI dependency build was still active.

| Boundary | Before | After | Evidence |
|---|---:|---:|---|
| JPEG 384 px warm preview-helper request p95, 100 samples | 16.435 ms | 0.024 ms | `before.log`, `after-p06-final.log` |
| JPEG warm source-work entries | 1 full read + 1 decode + 1 encode per request, source-verified old path | 0, asserted over 100 requests after store reopen | `tests/revision.rs` |
| Sony ARW 384 px warm preview-helper request p95, 100 samples including returned preview read | Not timed before | 0.295 ms | `after-p06-final.log` |
| RAW warm original/header/decode/render work | Original open/read/hash and embedded decode before lookup, source-verified old path | 0 source-work entries, asserted after reopen | `tests/revision.rs` |
| 80,000 seeded files, below-cap put/get p95, 20 samples | 3,556.128 ms | 8.209 ms | `before.log`, `after-p07.log` |
| 80k startup | 0.026 ms (no recovery/accounting) | 3,674.727 ms (one reconciliation) | same logs |
| 80k explicit eviction maintenance wall time | Not separately timed | 14.235 ms | `after-p07.log` |
| Maintenance hold of reader mutex | Entire old enumeration/sort/delete critical section | 0 ms: no shared reader mutex exists | structural oracle + held-writer-lock reader test |
| Full-directory enumeration sites reachable from put/get | Old put called full eviction scan | 0 | `verify_structure.py`, `structure.json` |

JPEG fixture: generated 1536×1024 RGB gradient/wrap pattern, quality-90 JPEG, 384 px tier. The baseline mirrors the old FFI path with orientation 1; the after test uses orientation 6 to exercise byte-for-byte orientation parity. Both process the same number of pixels. RAW fixture: checked-in `fixtures/raw/sony-arw.ARW`. P07 fixture: exactly 20,000 key directories × four 32-byte files, verified against incremental accounting after open and overwrite. Startup is excluded from per-request timings and reported separately.

`source_work_count` counts entry into the cold source path before the first original read/open; every codec/render invocation for these entry points is downstream of that boundary. Zero is thus an execution-path oracle for no source decode/encode/read, not an OS syscall trace. `verify_structure.py` likewise explicitly reports a structural, not syscall-timing, oracle. The maintenance wall time is not mislabeled as reader lock time.

## Verification and reproduction

Always preserve the external Cargo target:

```
export CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M2-55
cargo test -p previews -p tessera-ffi --release && cargo clippy -p previews -p tessera-ffi --all-targets -- -D warnings && cargo fmt --check
cargo test -p previews --release --test revision -- --nocapture --test-threads=1
cargo test -p previews --release cache_80k_measurement -- --ignored --nocapture --test-threads=1
python3 tools/orchestrate/wp/M2-55/verify_structure.py
```

Coverage includes restart cap reduction and abandoned-write cleanup, overwrite accounting, already-missing victims, more than one eviction batch, concurrent atomic replacements, bounded touch queue overflow, reads while the maintenance mutex is deliberately held, duplicate handle ownership/accounting, alias admission at the cap, all eight EXIF orientations, tier/recipe key changes, same-size replacement and in-place rewrites with restored mtime, JPEG FFI reopen, and 16/32-bit edited Linear DNG reopen.

Test-first evidence: `red-p07.log` records the old startup recovery assertion failing; `green-p07.log` records its initial fix. `red-p06.log` records the missing revision API before implementation. `red-disk.log` records the duplicate-owner failure; `red-admission.log` records a dropped large insertion; `red-shared.log` records the missing shared-owner API. `green-disk.log` records the isolated disk tests passing. `disk_harness.rs` includes the actual disk module, not a reimplementation, and was used while Cargo's FFI dependency build held the build lock.

An additional direct run during the FFI build passed all new unit regressions, but the existing cold `raw_without_jpeg_is_rendered` timing guard failed at 22.999 s against its 3 s local-only budget (`previews-direct.log`). Pixel mean/contrast assertions passed. This failure is retained rather than hidden.

## Gate status

The exact requested gate failed after successful release compilation (25m 15s): 15 preview unit tests passed, 1 failed, and 3 were ignored. The failure is the unchanged cold RAW rendering budget: 22.806 s versus `<3 s`. The `&&` chain consequently did not run FFI tests, clippy, or fmt in that invocation. See `gate-failure.txt`, transcribed from terminal process `proc_fe9085caf02b`.

A bounded-Rayon (`RAYON_NUM_THREADS=4`) isolated retry also failed at 24.177 s (`cold-raw-bounded.log`). The assertion was not weakened, deleted, or hidden. The source pixel statistics remained unchanged. This is outside the warm-hit P06 budget, whose final JPEG and RAW measurements both passed `<20 ms`.

The diagnostic run with `CI=1 RUST_TEST_THREADS=1` completed: **216 passed, 0 failed, 12 ignored** across 30 test binaries/doc-test suites (`diagnostic-tests.log`, programmatically aggregated in `diagnostic-summary.json`). That includes all 53 FFI unit tests and the new FFI JPEG-reopen/inode-invalidation regression. Existing CI timing guards apply, so this is explicitly not a pass of the exact gate.

Clippy initially identified two new private helper names as `wrong_self_convention`; these were renamed from `from_*_uncached` to `load_*_uncached`, without behavior or public API changes. After that naming-only cleanup:

- `cargo clippy -p previews -p tessera-ffi --all-targets -- -D warnings`: **exit 0** (`clippy-final.log`). Native LibRaw compiler warnings are dependency build-script output, not hidden Rust clippy failures.
- `CI=1 RUST_TEST_THREADS=1 cargo test -p previews --release`: **20 passed, 0 failed, 3 ignored**, exit 0 (`previews-final.log`). The full 216-test diagnostic run preceded this naming-only cleanup.
- `cargo fmt --check`: **exit 0** (`fmt.log`, empty).
- Final strict, no-CI cold RAW test on the final source still failed: **4.9748035 s versus <3 s**, exit 101 (`cold-raw-final.log`). The required gate therefore remains red; no green release-gate claim is made.
- `git diff --check` and the allowed-path scope check passed. `source-manifest.json` pins the final changed source hashes. No repo-local `target/` was created.

A further exact-gate retry after the final naming cleanup exceeded the terminal tool's 420-second timeout while compiling the FFI dependency graph (`gate-final.log`); no completed result is claimed for that retry. The earlier completed gate and final isolated cold RAW failure above remain the available strict-gate evidence.

## Verification retry after the reported failure

Reviewed the existing implementation and reran the exact requested command without
CI overrides, skipped tests, or relaxed assertions. No production source or test
threshold was changed in this retry. `CARGO_TARGET_DIR` remained the requested
external directory. No app was launched.

- `retry-cold.log`: isolated cold RAW test failed at **10.051080 s**.
- `retry-gate.log`: exact gate exited **101**, with **15 passed, 1 failed,
  3 ignored** in the preview unit suite. Cold RAW took **4.833834292 s** versus
  `<3 s`. Pixel mean and contrast passed. The chained clippy/fmt steps were not
  reached.
- `retry-p06.log`: all **3** revision integration tests passed. JPEG warm p95
  **0.047 ms**, RAW warm p95 **0.186 ms**, each with **0 source-work entries**.
- `retry-p07.log`: the explicit 80,000-file measurement passed. Startup
  **2,375.003 ms**, below-cap put/get p95 **5.004 ms**, maintenance wall time
  **11.934 ms**. Reader mutex hold remains **0 ms by construction**, not a
  measurement of maintenance wall time. The structural oracle passed again:
  both enumeration sites are confined to startup, with none in put/get.
- `retry-ffi.log`: a separate, unmodified `cargo test -p tessera-ffi --release`
  exited **101**. All **53 FFI unit tests** passed, including JPEG cache reopen
  and inode invalidation. The later Develop integration suite failed
  `export_batch_does_not_starve_slider_drag` at `tests/develop.rs:1198`:
  **70 of 120** frames remained at L2, violating its L2 assertion. This separate
  failure is not hidden by reporting only cache tests. Remaining test binaries
  after that failure were not reached.
- `retry-clippy.log`: separately executed required clippy command exited **0**.
- `retry-fmt.log`: separately executed `cargo fmt --check` exited **0**.

The host reported load averages **45.34 / 58.48 / 62.94** on **10 CPUs** during
the retry. That is evidence of contention, not proof of the cause of either
failure. The cold render path and its timing guard predate P06/P07. Fixing the
Develop/export scheduling behavior would be outside these two packages. A
quiet-host gate rerun or separately scoped rendering investigation is still
needed. The earlier before/after measurements above are retained with their
original logs rather than overwritten or presented as this retry's results.

## Latest verification of the reported 10.567 s failure

Read the audit in full and reviewed the cache implementation and FFI diff.
No production code, timing threshold, or test selection in the required gate
was changed. The cold render body is unchanged by this work package; the RAW
diff adds revision lookup and alias publication around it. No application was
launched. The external Cargo target remained set throughout.

- Isolated reproduction: **7.799169 s**, failed the existing `<3 s` assertion
  (`repro-cold.log`).
- Exact requested gate: **exit 101**, **15 passed, 1 failed, 3 ignored** in
  preview unit tests. Cold RAW took **8.711207 s**, with passing mean/contrast
  checks (`verification-gate.log`). The `&&` chain stopped before FFI tests,
  clippy, and fmt. No complete-suite pass is claimed.
- P06 release integration tests: **3 passed**. JPEG warm p95 **0.041 ms** and
  RAW warm p95 **0.145 ms**, both with **0 source-work entries** after reopen
  (`verification-p06.log`). The original measured JPEG baseline remains
  **16.435 ms** in `before.log`.
- P07 explicit 80k test: **passed**. Below-cap put/get p95 **7.296 ms** versus
  the original **3,556.128 ms** baseline. Startup **2,400.445 ms**; maintenance
  wall time **9.948 ms** (`verification-p07.log`). Maintenance holds no reader
  mutex. Structural verification again found **0 enumeration sites outside
  startup** and **0 reader acquisitions of the maintenance mutex**.
- Independently run required clippy and fmt commands: both **exit 0**
  (`verification-clippy.log`, `verification-fmt.log`). `git diff --check` passed.
- Host load at gate time was **29.82 / 40.48 / 53.25** on **10 CPUs**. This
  supports concern about contention but does not prove it caused the failure.

The previous separate FFI export/slider failure remains historical evidence,
not a newly rerun result. Resolving the strict cold-render gate requires a
controlled-host run or a separately scoped cold-render investigation, not
weakening the assertion or changing cache-hit acceptance criteria.

RESULT: FAIL — exact gate fails raw_without_jpeg_is_rendered at 8.711207 s versus <3 s. P06/P07 targeted checks, clippy, and fmt pass.
