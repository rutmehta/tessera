# Standalone native sheet contract probe

Request ecf80308-fa67-4ecd-a3d1-aad877e3a81d. **SOURCE ONLY / UNRUN on B.**
No compile, app launch, test or benchmark was performed. This branch adds diagnostic source only
above adapter93244f8f; it does not change that adapter under A review. A reviews/runs this after
its current native gate frees its lane. No product acceptance or B workload authorization.

## What is measured

`NativeSheetContractProbe.swift` is a standalone AppKit executable with no Tessera/FFI imports.
Each invocation creates a 300x120 parent and 220x70 owned sheet. The queue scenario additionally
creates a 230x75 unrelated sentinel. No documents, catalog, user photos, network or app settings.
The probe uses accessory activation policy and does not explicitly activate another application.
It does show real disposable windows when A runs it; execute in A's isolated GUI validation lane.

Choose one scenario per process:

| Argument | Native action after verified setup | Required outcome |
| --- | --- | --- |
| `queued-cancel` | Sentinel is actually attached; owned sheet is actually later in parent.sheets. Call endSheet on exact owned window only. | Owned completion exactly once, owned absent from attachedSheet AND sheets, sentinel still attached/member with original parent and zero completions before cleanup. |
| `parent-close` | Owned sheet actually attached; call captured parent.close, with no endSheet assist. | Actual willClose, exact owned completion and absence from parent membership. A failure is evidence about close alone, not an adapter failure verdict. |
| `end-then-parent-close` | End exact owned sheet, then close captured parent in the same action. | Owned completion plus membership clear and observed parent close; records any reentrancy. |
| `close-handler-end` | Close captured parent; willClose observer ends only the captured owned sheet, once, if still a member and not completed. | Same join, recording the close-handler reentrancy matching the adapter's shutdown route more closely. |

By default, the owned completion callback does **not** orderOut the sheet. This isolates native
endSheet/close membership behavior. Repeat with `--order-out-in-completion` to explicitly perform
and log the exact owned sheet's orderOut inside its completion, matching the source adapter's
completion action. This is a real AppKit call, not fabricated completion/detach. Raw and orderOut
results must remain separate; a raw failure does not automatically imply adapter failure.

Each JSONL event records sequence, phase, monotonic uptime, scenario/mode, logical invocation and
parent tokens, physical ObjectIdentifier strings, actual parent.attachedSheet, complete parent.sheets,
child sheetParent metadata, membership and callback counts. The closure captures its invocation UUID
before beginSheet. Parent didEndSheet only triggers a membership re-read; it never identifies the
owned sheet or increments owned completion. There is no polling or delayed-success inference.
The join is evaluated after the explicit action returns and after nested completion/orderOut calls.

## Proposed A compile/run commands — not executed on B

Run from a checkout containing this source. Use a disposable output directory on A. Preserve the
exact Swift source, compiler/version/OS information, compile direct exit and logs before execution.
For example (the directory below is just an A-side proposal):

```sh
mkdir -p /tmp/tessera-native-sheet-probe
xcrun swiftc -swift-version 6 -strict-concurrency=complete -warnings-as-errors -parse-as-library \
  tools/coordination/native-sheet-contract-probe/NativeSheetContractProbe.swift \
  -framework AppKit -o /tmp/tessera-native-sheet-probe/probe \
  > /tmp/tessera-native-sheet-probe/compile.stdout 2> /tmp/tessera-native-sheet-probe/compile.stderr
probe_compile_status=$?
printf '%s\n' "$probe_compile_status" > /tmp/tessera-native-sheet-probe/compile.exit
```

Only after reviewing a direct zero compile exit, run scenarios separately, serially. Example:

```sh
python3 tools/coordination/native-sheet-contract-probe/run_probe.py \
  /tmp/tessera-native-sheet-probe/probe queued-cancel \
  > /tmp/tessera-native-sheet-probe/queued-raw.jsonl 2> /tmp/tessera-native-sheet-probe/queued-raw.stderr
probe_run_status=$?
printf '%s\n' "$probe_run_status" > /tmp/tessera-native-sheet-probe/queued-raw.exit

python3 tools/coordination/native-sheet-contract-probe/run_probe.py \
  /tmp/tessera-native-sheet-probe/probe queued-cancel --order-out-in-completion \
  > /tmp/tessera-native-sheet-probe/queued-orderout.jsonl 2> /tmp/tessera-native-sheet-probe/queued-orderout.stderr
probe_run_status=$?
printf '%s\n' "$probe_run_status" > /tmp/tessera-native-sheet-probe/queued-orderout.exit
```

Use analogous unique filenames for each close scenario/mode. No shell command here runs another
scenario automatically after failure. Preserve failures before investigating; do not silently replace
a failing log with a later run. Do not run the adapter or Tessera as part of this probe.

## Watchdogs, cleanup and interpretation

A 5-second one-shot main-queue watchdog emits a failed result if the native join never arrives.
It never increments completion or clears membership. The Python runner supplies an independent
15-second process watchdog, because a main-thread AppKit hang can also block the internal timer.
On external timeout it reports failure and kills only its captured child process; no kill-by-name,
process group kill, global modal abort, or other-app interference. Exit124 means incomplete/timeout,
not native success, even if an earlier result line existed before cleanup hung.

The result is logged BEFORE cleanup. Cleanup removes observers, cancels the diagnostic timer,
ends only known captured live invocations (and never reissues the measured owned end), orders out
and closes only disposable captured windows, then nils probe-owned references and records retirement.
Late cleanup callbacks are tagged cleanup and cannot convert failure to success. The sentinel is
allowed to change only in this clearly separated cleanup phase.

Accept a probe scenario only with all of: observed valid setup, actual owned completion+membership
join, result passed, explicit retirement record, and process direct exit0 without external timeout.
`references.released` means probe-held fields/observer tokens were cleared; it is NOT proof of
AppKit deferred deallocation or absence of a leak. Local call-stack/autorelease references may remain
until process exit. The source intentionally makes no heap/profiler claim and process exit is not
used as native completion evidence.

Scope limits: no SwiftUI lifecycle, Escape/Return/focus/AX, nested NSOpenPanel, same-window bridge,
workspace save outcomes, actual filesystem write, or full Tessera validation. Those adapter tests
and GUI gates remain required. Programmatic parent.close is distinct from user close-button handling.
A must report native ordering/contract failures to the coordinator before changing safety guards;
never repair a missing completion with a synthetic callback, timer, nil attachedSheet alone, or
unrelated parent event. Source and compiler errors remain possible because B has not run this code.
