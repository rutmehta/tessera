# Visible capability: three unsuccessful launch attempts

Saved-evidence review on 2026-09-28; no replay, capture, GUI interaction, or process mutation performed by the reviewer. These attempts do not establish visible capability, presentation latency, or P11 acceptance.

| Attempt | Runner direct exit | Saved outcome |
| --- | --- | --- |
| 01: BetterSSD stdio | 1 | Launch Services failed before an owned app was recorded. Scoped kernel log identifies xpcproxy file-write-create denial for this attempt's stdout path. |
| 02: precreated BetterSSD stdio | 1 | Launch Services failed before an owned app was recorded. Separate scoped kernel log records xpcproxy(65044) file-read-data denial for the precreated stdout file at 08:51:31. Precreation alone did not resolve launch. |
| 03: fresh /tmp stdio relay | 1 | Exact isolated process recorded, but no foreground regular window within five-second startup allowance; 17 observations, zero target-foreground and zero target layer-0 window observations. |

All three lack trace.json and its presentation receipt. The app provenance in each run names f1d13c11ca1abe1c95b36176bb1e0c57d2b5b393, with identical fixture and probe hashes recorded in independent-saved-audit.json. The tested app/archive were unchanged; the runner was changed between attempts. Final relay runner bytes and Python test match commit 9423956c000a96b422441a429260cdd5e45d305a exactly. Included launcher-source files preserve those bytes, source hashes, patch, saved 8-test Python result, and app provenance verification. Earlier intermediate runner source was not separately frozen here, so those runs are not claimed to use the final runner bytes.

Attempt 03 recorded PID 67652, bundle dev.tessera.m258.visible.f1d13c11, exact bundle URL and launch date. Saved cleanup confirms graceful termination was requested and the process did not exit. Additional settlement observed the same identity through 80 polls. Its wait_limit_seconds=20 represents 80 times 250 ms sleeps, plus subprocess overhead and bounded probe calls; it is not a strict 20-second wall-clock deadline. The relay wrapper's exit 0 reports successful evidence settlement/copying, not successful capability: the runner direct exit is 1. Empty copied stdout/stderr hashes are retained.

The saved process sample independently places all 788 main-thread samples in mkdirat, reached through AppDefaultsIsolation.defaults(in:) and directory creation during TesseraApp.init, before a visible window. The sample includes only the target process call graph and loaded binary image paths; it contains no unrelated window titles or environment values. This localizes startup blocking without identifying protected dialog contents or proving the kernel wait cause.

Coordinator subsequently reported a protected system dialog and requested leaving PID 67652 intact while user input is pending. That diagnosis was not independently accessed in this audit. No alternate prompt access, permission request, force termination, or retry was performed by this reviewer. The recorded non-exit is historical evidence, not a live process-status assertion.

## Privacy and provenance

Original run.json and window-observations.jsonl remain at the local paths in REDACTION.json; they are not published here. Published JSON copies remove window title fields unless owner_pid equals the recorded test PID for that attempt (none for attempts 01/02; 67652 for 03). Other fields are retained to preserve foreground/window qualification evidence. REDACTION.json records original raw hashes, derivative hashes and exact rule. Independent audit reconstructed and compared every derivative from its raw original without printing unrelated titles. No screenshots are included.

All SHA256SUMS entries use relative paths and cover every payload except the manifest itself. Raw runner failures and warnings are retained. The package makes no success claim and does not alter the original M2 or HDR acceptance gates.
