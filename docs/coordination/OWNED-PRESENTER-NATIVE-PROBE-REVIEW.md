# Native sheet contract probe — source review (2026-09-28)

Reviewed `5ebeb17a:tools/coordination/native-sheet-contract-probe/{NativeSheetContractProbe.swift,run_probe.py,README.md}` via `git show`. Read-only; no compilation, app launch, GUI, or product edit. Repository `/Users/rutmehta/Developer/tessera`.

## Verdict

No source-proven blocker or obvious Swift 6 compile error found. The probe is suitable for an **isolated diagnostic run** once the GUI/compiler lane is released; the run and its actual AppKit result remain unverified. It cannot by itself accept the Tessera Save As adapter.

* `NativeSheetContractProbe.swift:123-174` checks the sentinel is physically attached and, for `queued-cancel`, that the owned sheet is later in `parent.sheets` before issuing `endSheet` on the exact owned window (`:175-194`). This prevents a nonqueued setup from masquerading as the queued-only experiment.
* The owned `beginSheet` closure increments its own count (`:141-160`). `didEndSheet` only emits a parent event and rechecks membership (`:90-100`); it cannot fabricate completion. `evaluate` waits for the action to return, exactly one owned completion, and absence from both `attachedSheet` and `sheets` (`:199-214`). In the queue case it also requires the unrelated sentinel to remain attached/member with zero completions before cleanup. The optional `orderOut` is a real, logged AppKit call after actual completion and is reported as a distinct mode (`:148-158`).
* The 5-second in-process timer can only fail (`:79-86`). The Python 15-second watchdog kills only its captured child PID and exits 124 on timeout (`run_probe.py:23-36`). `finish` logs the result before cleanup, and `finalized` prevents cleanup callbacks from turning a failure into success (`NativeSheetContractProbe.swift:215-244`). The runner inherits JSONL output, so a blocked process does not lose earlier writes through a Python pipe.
* Each run creates only three small disposable windows at most, uses accessory activation, and keeps its delegate alive around `app.run` (`:66-74,247-260`). Cleanup removes only its own observers and ends/ closes captured probe windows (`:221-238`); there is no global modal abort, kill-by-name, document/catalog access, or other-app teardown.

## Interpretation limits / suggested execution checks

* Treat `queued-cancel` as accepted only if raw JSONL includes `queued-only.confirmed`, `owned.completion`, `owned.join.confirmed`, `result.passed=true`, `references.released`, and process direct exit 0. The unrelated sentinel must remain attached with no completion until the separately tagged cleanup. Run raw and `--order-out-in-completion` in separate processes/logs. A five-second failure is an unresolved native result for this setup, not proof that another timing would pass.
* For parent-close variants, `parent.close()` is programmatic and not the user close-button/SwiftUI lifecycle; the README correctly calls this out (`README.md:96-98`). The probe does not test nested `NSOpenPanel`, bridge identity, writer admission, Save As UI, or deallocation. `references.released` records probe-held fields cleared, not actual native object destruction (`README.md:89-94`).
* The README compile command is plausible: the repository already has a strict Swift 6 `@MainActor` `NSApplicationDelegate`, and the probe's synchronous AppKit calls have matching typed parameters. The compile gate must still be run and errors preserved; B reports the source as UNRUN.
