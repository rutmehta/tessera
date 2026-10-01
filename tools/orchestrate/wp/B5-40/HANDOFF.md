# B5-40 — Export Flat main-thread spans

Status: **BLOCKED before baseline measurement**. The required quiet-host condition did not
occur within the requested 30-minute wait. No performance improvement or P16 acceptance is claimed.
Branch `wp/B5-40`; unchanged app/engine base `2164d3708e05ac06423b55554d6d0eacdb6d02ce`,
matching local `origin/main` at inspection time.

## Quiet-host evidence

On 2026-10-01, polled at 30-second intervals from **05:56:20 through 06:26:28 EDT**
(30 minutes plus subprocess/scheduling overhead). Required both load1 < 5 and no process
whose executable basename was `cargo` or `swift-build`.

| Metric | Result |
|---|---:|
| Samples | 61 |
| Qualifying samples | 0 |
| Minimum 1-minute load | 5.6099 |
| Median 1-minute load | 7.7793 |
| Maximum 1-minute load | 79.9541 |
| Samples with cargo/swift-build present | 47 |
| Final load | 6.7427 |

Final sample still had cargo PID 35216. No other agent's processes were interrupted.
Raw timestamped evidence: `evidence/quiet-before.jsonl`. The initial preliminary `uptime`
read was 11.26, before the structured polling started.

## Measurements and gates

| Fixture | BEFORE (3 runs; median) | AFTER (3 runs; median) | <8 ms met? |
|---|---|---|---|
| 18 MP smart-filter | Not run | Not run | Unverified |
| Styled 14 MP | Not run | Not run | Unverified |

- Release packaging: not started, because its prerequisite quiet condition failed.
- RED test and fix commits: not created. No production code or test changes.
- Required FFI build / Swift gate: not run; **no SWIFT GATE OK**.
- Rust gates: not run; no Rust changes.
- No GUI launch, screen capture, push, board.json change, or Cargo.lock change.
- Documentation/evidence only; the requested test → fix → docs implementation sequence remains pending.

## Source review and continuation

Read B5-33's HANDOFF.md, run-perf.sh and analyze-profile.py. Historical measurements from
that package are not a clean BEFORE for this package and are not repeated as new measurements.

The two supplied leads remain present in this checkout:

1. `apps/mac/Sources/Tessera/Document/DocumentWorkspace.swift`, `startExportFlat` calls
   `DocumentFlatExporting.beginExportFlat` synchronously before creating the detached task.
   `crates/tessera-ffi/src/document.rs`, `DocumentSession::begin_export_flat`, takes
   `self.shared.lock()` to clone the state. Worker-side setup must also preserve immediate
   cancellation and export survival when the document closes immediately after launch;
   simply deferring the existing call can race session closure.
2. Each engine progress callback enqueues a main-actor task, then calls
   `updateExportAccessory` → `FlatExportProgressView.update` → `arrangeRows`.
   `Row.update` already avoids identical label assignments; its phase label already uses
   `Theme.NSFonts.labelNumeric` (monospaced digits) and a fixed frame. Coalescing publication
   and avoiding repeated geometry work remain candidates, requiring measurement and RED coverage.

These are source-level leads, **not attributed residual-span measurements**. No new outlier
call sites can be reported without the baseline/profile run.

Resume with the quiet check, then the release build and unchanged B5-33 fixtures; do not skip
BEFORE to implement a speculative fix. `wait-quiet.py <log.jsonl>` provides a reusable bounded
check (exit 0 = quiet; exit 2 = timeout). `run-perf.sh` is the B5-33 runner redirected to this
package, with a quiet check before launch. It has not been executed. Its fixture directory
`evidence/photos` must first be populated with the same `sample.dng` fixture. Use `PROFILE=0`
for the unprofiled comparison and fresh run suffixes. The inherited harness exports each
fixture twice per invocation; define the three-run aggregation explicitly before measuring.

All future builds must use:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/B5-40"
```

Then follow the required failing-test, fix, and final-results handoff commits with the requested
co-author trailer. Keep builds serial and GUI launches limited to the approved background self-test.
