# B5-39 — unified layout harness on the colour stack

## 2026-10-01 follow-up — fixed 2× offscreen OCR

This follow-up is **commits on top of `992e0cbd`**, with no additional rebase.
The earlier reconciliation/rebase description below is historical.

### RED reproduction on Machine B

`00751ce7` (`test(B5-39):`) adds a test-only `NonRetinaCacheView` around the
real populated Masks host. It overrides only the legacy bitmap allocator to
return one pixel per point, simulating the failing non-Retina cache on this
Retina machine. Panel content, Vision settings, and the full-label predicate
(`words.contains(label) || words.contains(label + "v")`) are unchanged.

The release test exited 1 with **11 assertions failing**: eight pixel-dimension
assertions plus three OCR assertions. All four images logged scale `1.0`:
288×720 or 380×720 pixels for the corresponding point dimensions.

- 288-light: `full label 'Subtract' is not painted`; recognized `v)(Subtract`.
- 380-light: `full label 'Subtract' is not painted`; recognized `(Subtract`.
- 380-light also missed full-label `Add`; recognized `Addv)`.

This reproduces A's failure class and the `Subtract` failures at both widths,
including `(Subtract`, but not A's exact set of missed labels: B recognized
`Intersect` in this simulated run. See `evidence/ocr-scale/red-1x.log`.

### Fix and repeat verification

`LayoutProbeHarness.bitmap` now allocates an explicit RGBA bitmap at twice the
view's point dimensions and sets its logical `size` before `cacheDisplay`.
Drawing therefore targets 2× pixels independently of the display's backing
scale, without upscaling an already rendered 1× image. Vision and optional PNG
evidence consume this same bitmap. The retained 1× fixture catches reversion to
the screen-dependent allocator. Every Masks capture asserts both pixel dimensions
and logs pixels, points, and scale. No full-label matching was loosened.

Five consecutive serial release invocations of `MasksPanelLayoutTests` passed:

| Run | Tests | Failures | XCTest duration |
| --- | ---: | ---: | ---: |
| 1 | 1 | 0 | 1.120 s |
| 2 | 1 | 0 | 1.055 s |
| 3 | 1 | 0 | 0.998 s |
| 4 | 1 | 0 | 1.018 s |
| 5 | 1 | 0 | 1.009 s |

Each invocation covers 288/380 points in light/dark: **20 captures total**, all
logging scale `2.0` (576×1440 and 760×1440 pixels). No retries or exclusions.
See `evidence/ocr-scale/green-1.log` through `green-5.log`.

Commands (from the repository root):

```sh
(cd apps/mac && swift test -c release -Xswiftc -enable-testing --filter MasksPanelLayoutTests)
# Repeat the command above five consecutive times.
export PATH="$HOME/.cargo/bin:$PATH"
cd apps/mac && ./build-ffi.sh && cd ../.. && tools/orchestrate/swift-gate.sh
```

The required FFI build and unmodified Swift gate exited **0** and printed
**SWIFT GATE OK**. Debug build: 8.63 s. XCTest: **878 tests, 3 skipped, zero
failures**, 168.123 s (168.193 s suite wall time). Swift Testing: **5 tests in
2 suites passed**, 0.024 s. FFI regeneration produced no tracked source changes.
Existing LibRaw and Swift weak-variable compiler warnings remain in the logs.
See `evidence/ocr-scale/ffi.log`, `swift-gate.log`, and `swift-gate-tests.log`.

Local sequence: `00751ce7` is RED; `30209036` is the fixed-scale helper;
the following `docs(B5-39):` commit records this handoff and verification logs.
All three commit messages end with the requested co-author trailer.

Only test sources, this handoff, and evidence change in this follow-up. Product
sources, Rust, `Cargo.lock`, and `board.json` remain unchanged. No GUI app launch,
concurrent builds, rebase, push, or merge. Native non-Retina Machine A verification
remains for A; the local regression uses the explicit 1× cache fixture.

---

Branch: `wp/B5-39`. Base: `origin/wp/B5-30b` at
`73461d02ba3180c7112996ac2ca841d64c3759a3` (B5-30 `69728b71` → B5-30c
`2f024d9c` → rebased B5-30b). Local commits only.

The requested plain rebase exposed 19 unrelated main commits before the two B5-39
commits. It was aborted and repeated with `git rebase --onto origin/wp/B5-30b
b1af2436 wp/B5-39`, replaying only B5-39's test/harness and documentation commits.
This keeps unrelated main product/Rust changes out of the colour-stack handoff.

## One shared layer

`apps/mac/Tests/TesseraCoreTests/LayoutProbeHarness.swift` now owns all fixture
infrastructure. `ShellLayoutHarness.swift` is superseded and removed. The `ShellHarness`
namespace remains in the unified file for shell-specific models, outer-window geometry,
AX audits and optional whole-window evidence; its window wrapper delegates creation and
settling to `LayoutProbeHarness`, with no independent constructor or wait loop.

- **Settling:** B5-39's single `Probe` engine flushes layout/display, compares exact native
  descendant identity, frames, bounds, visibility and `DocumentInspectorProbe.frames`,
  requires cleared layout/constraint flags and 50 ms of stable observations, and reports
  failure after a two-second polling budget. Sync and async entry points share that engine;
  async callers release MainActor between samples. Intrinsic size queries compare returned
  sizes without requiring unattached native hosts to clear layout flags. B5-30c's fixed
  three-pass sleeps and the busy-presentation/shortcut fixed waits are superseded.
- **Animation policy:** one shared implementation applies SwiftUI's disabled-animation
  transaction, zero-duration AppKit flushes and `.none` window lifecycle animation.
  B5-30c's complementary worker-safety guards are retained here: start NSAnimation on the
  main run loop, stop existing progress workers before selecting timer mode, and stop them
  at disposal. These address native animations outside SwiftUI transactions; they are not
  separate settle loops. There is no second policy in shell or Masks tests. The original
  timer/NSAnimation regression assertions remain unchanged.
- **Window lifecycle:** all 34 test-window construction sites delegate to the one factory,
  including B5-30c's progress fixture and B5-30b's display-colour fixture. The factory prepares
  the nonactivating process, disables lifecycle animation and retains windows across close.
  Layout fixtures share B5-30c's detach/order-out/close disposal helper. Shell windows establish
  requested bounds before ordering to avoid initial preferred-size geometry callbacks.
- **Preferences:** B5-30c's private per-process app-directory store remains, now owned by
  shared preparation. Shell, Masks, Review and other layout consumers use the same helper.
- **OCR:** one `bitmap` helper caches the real hosting view. Masks feeds its CGImage directly
  to Vision and reuses the same bitmap for optional PNG evidence. B5-39's duplicate
  content-only capture → PNG → sips → URL OCR route is dropped; B5-30c's inline bitmap
  implementation is moved into the helper. Optional whole-window screenshots remain distinct
  evidence, not an OCR fallback.

All layout tests use the shared layer, directly or through shell/intrinsic-sizing adapters.
Existing geometry assertions, tolerances and full-label OCR assertions are unchanged.
Model-operation waits (preview delivery, develop loading, queue completion) remain distinct
from layout settling. A stable layout does not establish completion of a future model task.

## Fresh verification on the reconciled branch

Evidence is recorded under `evidence/reconciled/`. The older files elsewhere in `evidence/`
describe the original main-based branch and are not verification of this reconciliation.

Fresh FFI regeneration succeeded with no tracked generated-source changes. The release
preflight passed 9 tests with zero failures (38.082 s).

| Loaded run | Tests | Failures | XCTest duration |
| --- | ---: | ---: | ---: |
| 1 | 56 | 0 | 100.110 s |
| 2 | 56 | 0 | 74.876 s |
| 3 | 56 | 0 | 85.499 s |
| 4 | 56 | 0 | 83.669 s |
| 5 | 56 | 0 | 78.391 s |

**280 executions, zero failures; 0/5 failed invocations, no retries or skipped tests.**
All 11 affected classes ran in each pass, including the two B5-30c animation checks and
B5-39's two polling checks. The deliberately unstable measurement remains an expected
failure regression, not an ignored failure. See `stress-summary.log` and `focused-1.log`
through `focused-5.log`.

The Rust workload ran 07:48:21Z–07:56:23Z on 2026-10-01; the five Swift invocations ran
07:48:21Z–07:55:30Z. Six Cargo release builds exited zero. The stress script itself exited
zero after waiting for its final build. See `cargo-stress-timeline.log`.

Fresh full verification ran the requested `build-ffi.sh` followed by the unmodified
`tools/orchestrate/swift-gate.sh`. It exited zero and printed **SWIFT GATE OK**:

- Debug build: 29.68 s.
- XCTest: **877 tests, 3 skipped, zero failures**, 178.445 s.
- Swift Testing: **5 tests in 2 suites passed**, 0.023 s.

See `ffi-gate.log`, `swift-gate.log` and the complete retained `swift-gate-tests.log`.
No test exclusions or gate overrides were used.

## Reproduction and scope

Build matching FFI and the release XCTest binary before stress:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
(cd apps/mac && ./build-ffi.sh && swift test -c release -Xswiftc -enable-testing --filter 'LayoutProbeHarnessTests|ShellLayoutTests|MasksPanelLayoutTests')
tools/orchestrate/wp/B5-39/stress.sh
(cd apps/mac && ./build-ffi.sh)
tools/orchestrate/swift-gate.sh
```

The stress runner uses five serial Swift test invocations and one concurrent Cargo release
build loop in `~/.cache/tessera-target/B5-39-layout-stress`. Only that isolated target's FFI
artifacts are cleaned between successful Rust builds. Normal FFI and Swift builds run serially;
the stress load is the explicitly requested exception. No GUI application launch.

No product sources, Rust, Cargo.lock or board.json changes relative to `73461d02`.
In particular, B5-30c's DocumentController/DocumentViewport AppKit animation worker changes
and AWA stale-preview fix remain byte-for-byte unchanged. No push or merge.

## Local commits

- `189dc58e` — rebased B5-39 harness commit with conflict resolution and unified helper.
- `8f696a12` — rebased original evidence/docs, retained as historical evidence.
- `20bbd600` — remaining shared factory/disposal consumers and separate stress evidence path.
- The following `docs(B5-39):` commit records this handoff and fresh verification evidence.

Final scope audit: `evidence/reconciled/scope-audit.log`. No push or merge.
