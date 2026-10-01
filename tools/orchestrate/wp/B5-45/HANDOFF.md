# B5-45 — Library/Develop background self-tests

Completed on Machine B, branch `wp/B5-45`, based on `445131fc`. Local commits only.

## Commits

- `93193058` — `test(B5-45): require background host for Library Develop self-tests`
- `127aa0c0` — `fix(B5-45): run Library Develop self-tests in background host`
- This handoff and evidence are the subsequent `docs(B5-45):` commit.

## Change

SelfTestHost recognizes the exact boolean flags `--develop-selftest`,
`--develop-panels-selftest`, `--hdr-selftest`, and `--masks-selftest`. It initializes
the existing tool observers, waits for an indexed photo, and enters Photo Edit /
opens Develop in the existing never-key, never-main background host. Develop and
HDR flush pending controls when background display links pause.

The four tests now emit explicit completion lines. Missing frames, a lost initial
controller, missing startup input, and missing mask measurements/groups fail.
The runner adds `develop`, `develop-panels`, `hdr`, and `masks` cases, requires the
matching successful completion, rejects explicit FAIL output, and does not run
its capture protocol for these cases. Early exit/timeout is FAIL.

Keyboard routing requires a key window and is explicitly reported as:

```
N/A key-window keyboard routing (never-key host; controller paths exercised)
```

These existing tests exercise controller/tool paths, not physical keyboard or
mouse input. HDR's SDR fallback reports EDR float rendering as N/A and checks
that HDR remains enabled with non-float surfaces. This run exercised real float
surfaces; it did not take the SDR branch. No screen capture or foreground launch
was performed for these runs.

## Verification

All builds were serial, with:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR=$HOME/.cache/tessera-target/B5-45
cd apps/mac && ./build-ffi.sh && cd ../.. && tools/orchestrate/swift-gate.sh
```

- Red: 5 SelfTestHostTests, 4 expected assertion failures, one per new flag.
- Green: 5 SelfTestHostTests, 0 failures. Includes rejection of lookalike flags
  and unsupported `--flag=value` forms; existing window test checks never-key /
  never-main behavior and resizing without ordering or capturing it.
- Full gate: **SWIFT GATE OK**. 892 XCTest tests, 3 skipped, 0 failures; plus
  5 Swift Testing tests in 2 suites passed.
- No Rust source changes, so package-specific Rust test/clippy/fmt gates were
  not applicable. Required FFI builds passed (existing LibRaw warnings).
- Shell syntax and `git diff --check` passed.
- Six controlled runner cases passed: missing log, partial metrics, failure
  count, another test's completion, exact success, and FAIL followed by success.
  Capture was not invoked.

Packaged using `apps/mac/Support/make-app.sh release`. Its provenance verification
passed before the fix commit. After committing, full verification correctly
reports a commit mismatch; independently checked source manifest, FFI archive,
bindings and packaged executable hashes still match exactly. See
[evidence/source-match.log](evidence/source-match.log). These timings are engine
sink observations, not app input-to-display benchmarks.

### Live runs

Each used the worktree's release app, a fresh isolated app directory, and a copied
`fixtures/raw/sample.dng` through:

```sh
SP="$PWD/apps/mac/build/B5-45-runs/$name" TIMEOUT=150 \
  bash tools/orchestrate/wp/B5-selftest-window/run-background-selftest.sh "$name"
```

All four exited 0. Frontmost app was `loginwindow` before and after each run.
The host window was created in each run. Result lines:

```
develop-selftest: done, 0 failures
develop-panels-selftest: done, 0 failures
hdr-selftest: done, 0 failures
masks-selftest: done, 0 failures
```

| Run | Observed work | Evidence |
| --- | --- | --- |
| Develop | 60 tone frames, L1, median 2.9 ms, p90 4.1 ms | [develop.log](evidence/develop.log) |
| Panels | tone curve/HSL/grading/detail/vignette/grain: 40/38/39/31/21/39 frames | [develop-panels.log](evidence/develop-panels.log) |
| HDR | RGBA16F, 4 headroom frames, engine 1.2x, peak 1.20 | [hdr.log](evidence/hdr.log) |
| Masks | gradient/exposure/brush/saturation: 3/39/40/10 frames; 2 masks | [masks.log](evidence/masks.log) |

A real early-exit launch used the same runner with `develop --stub 0
--bundle-selftest`. The app exited before completion, frontmost stayed unchanged,
and the runner exited **1** with:

```
develop-selftest: FAIL missing successful completion (early exit, timeout, or failed check)
```

See [early-exit.log](evidence/early-exit.log). Compact logs are checked in under
`evidence`; full build logs and isolated run data remain in ignored
`apps/mac/build/B5-45-logs` and `apps/mac/build/B5-45-runs`.

No board.json or Cargo.lock changes. No user Lightroom catalog was opened. No
push, merge, or installed-app replacement was performed.
