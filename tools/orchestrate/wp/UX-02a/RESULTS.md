# UX-02a — navigable Review (focused validation complete)

Scope: session-local destination/navigation only. Queue relaunch persistence is
not implemented; UX-02b remains required for the full UX-02 goal. No Rust, FFI,
Document implementation or foreground activation changes are part of this slice.

## Implementation

Review has a virtualized queue, Current preview and per-photo inspector actions,
stable cursor/draft state, explicit editing targets outside the Library filter,
and lossless Library → Review → Edit → Review → Library navigation. Keyboard and
undo routing preserve text/control/Document priority. People/Tether inspection
continues to open Library Loupe. Captured-owner mutation guards prevent opening
Develop during matching recipe operations. The existing layered-copy handoff now
awaits pending saves and cancels when its owner/selection context changes; its
request-owned progress status clears without overwriting a newer status.

## Evidence

- Original navigation RED: release build 23.29s; 1 test, 4 expected assertion
  failures, 0 unexpected. `swift-navigation-red.log` preserves the former Review
  route changing source, visible IDs, selection and focus. The final route has
  an explicit Review edit helper; the legacy inspection helper is preserved.
- Frozen `4fcb802` focused GREEN: release build 121.02s; **43 XCTest, 0 failures**,
  5.132s. Includes 10 navigation tests, 4 cursor tests, workspace/native anchor,
  keyboard/Document priority and theme checks. Real JPEG regressions cover pending
  save ordering, filtered target round trips and first/reopened layered-copy
  pixels. `swift-focused-1.log`; exact source/FFI hashes and command are in
  `focused-source.json`.
- Frozen layout checkpoint: **1 test, 0 failures**, 34.195s, 32 background captures
  (empty, populated long-name, real missing-file failure, labelled synthetic busy;
  4 sizes × 2 appearances). `swift-layout-1.log`. Populated captures show loading;
  these establish layout/loading coverage, not loaded-preview acceptance.
- Cancelled-handoff status RED: **1 test, 1 expected failure**, 2.701s, build21.24s.
  Selection change left “Saving photo before opening Layers…” indefinitely;
  owner change already cleared it. A later unrelated status was preserved.
  `swift-status-red.log`.
- Final status/ready GREEN: release build85.10s; **2 tests, 0 failures**, 40.273s,
  finished 2026-09-27 19:01:37 UTC. `swift-status-ready-green.log` covers the
  corrected cancellation cases and layout matrix plus 2 loaded-preview captures.
  The real Review view starts from an empty preview cache; the harness yields
  MainActor until real image delivery, then captures at 960×600 light and
  1440×900 dark. No injected image, placeholder or synthetic operation is used.
  The toast is dismissed using its normal model action before ready captures.
  Both were visually inspected: image pixels and saved-adjustments caption are
  present; no toast obscures them. 34 final captures total.

Final tested Sources+Tests digest:
`432fff1dae551112cf435e2b24e8d72e9cf052f964e0e5a579bdb9d6f4ed9e15`.
`status-ready-source.json` records every file hash, command and FFI archive hash.
A post-interruption comparison found zero changed source/test hashes. All capture
locations and SHA-256 values are in `capture-manifest.json`; PNGs remain in the
ignored local build directories and are not part of the source commit.

Representative final captures:
- `apps/mac/build/ux02a-layout-status-fix/reviewReady-960x600-light.png`
- `apps/mac/build/ux02a-layout-status-fix/reviewReady-1440x900-dark.png`

## Remaining acceptance and limits

A full final-source suite and current-main integration gate remain pending root
review/resource scheduling. The 43-test checkpoint preceded the narrow status
repair; only the affected cancellation/layout checks ran on the final source.
Existing CoreDocument Sendable warnings and a macOS26.5-vs15.0 archive linker
warning remain in logs. Ownership's separate results do not substitute for this
navigation slice's gates.

Busy captures are explicitly synthetic presentation only; actual operation/race
behavior comes from real controller tests. The prohibited-activation harness
cannot materialize SwiftUI's full accessibility tree, so source AX labels and
visual disabled styling do not establish screen-reader traversal. No relaunch
persistence, before/after baseline, presentation latency or P11 performance claim
is made. UX04 photo-overlay/Mask Components polish remains separate. Tests never
activated the app; the user's separately opened Tessera process was untouched.
