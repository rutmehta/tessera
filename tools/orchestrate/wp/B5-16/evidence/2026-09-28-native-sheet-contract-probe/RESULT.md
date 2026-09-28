# Native sheet contract probe — A diagnostic receipt

Frozen diagnostic source: `5ebeb17a6754adde2132a0ef17de5a2481964804` (B `codex/native-sheet-contract-probe`). The three files under `source/` are byte-for-byte `git show` copies; `command-environment-manifest.json` is the pre-run **PREPARED_UNRUN** command/environment snapshot. Execution used the standalone disposable AppKit probe only, never Tessera or a user document. The compiled binary remained outside the repository at `/Volumes/betterSSD/tessera-validation/native-sheet-contract-probe/5ebeb17a/probe` (SHA-256 `a9cbc640b207a8c82ea5cad72a18ccc6910613c0cfaa8f189f96fe391c305789`).

Strict Swift compile (`-swift-version 6 -strict-concurrency=complete -warnings-as-errors -parse-as-library -framework AppKit`) returned direct **0** in 1.586 seconds. `attempt-1/compile-argv.json`, stdout, stderr, direct exit, and duration are preserved. The three source SHA-256 values matched the pre-run manifest before compilation and after all eight runs (`attempt-1/source-pre-sha256.json`, `source-post-sha256.json`). The installed Swift compiler version was not separately captured; no version is inferred from source or binary.

| Scenario | Raw | `orderOut` in owned completion |
| --- | --- | --- |
| Owned queued behind unrelated attached sheet, then exact owned `endSheet` | **Pass, exit 0** | **Pass, exit 0** |
| Parent `close()` with no owned `endSheet` | **Fail, exit 1** (5s internal watchdog) | **Fail, exit 1** (5s internal watchdog) |
| Exact owned `endSheet`, then parent `close()` | **Pass, exit 0** | **Pass, exit 0** |
| Parent `close()`; `willClose` handler ends exact owned sheet | **Pass, exit 0** | **Pass, exit 0** |

Each scenario/mode ran in its own child process, serially. All eight child PIDs were checked gone after direct exit. Every run emitted `references.released`; six emitted exactly one `owned.completion`, one `owned.join.confirmed`, a passed result, and direct exit 0. `attempt-1/run-summary.json` is a derived index; the raw JSONL, stderr, argv, and direct exit for each run are retained alongside it.

The queued-only raw sequence is decisive for the narrow AppKit contract: `queued-only.confirmed` records the unrelated sentinel attached and the owned sheet later in `parent.sheets`; the exact owned `endSheet` produced its own completion and removed it from `parent.sheets` while the sentinel stayed attached/uncompleted. The sentinel's completion appears only after the **cleanup** phase begins. The `orderOut` mode passed separately. This establishes the observed standalone native behavior on this host, not Tessera Save As acceptance.

Both unassisted parent-close logs show `parent.willClose` and return from `parent.close`, but the owned sheet remained `attachedSheet` and in `parent.sheets` with zero owned completions until the five-second failure. The later `owned.completion` appears only **after** the `result.passed=false` line, when cleanup explicitly ends that captured sheet. `--order-out-in-completion` could not help without a completion to enter. Conversely, explicit end in the `willClose` handler joined successfully in both modes. This distinguishes a real native close-alone failure from probe timing or a synthetic success. It does not by itself validate the Tessera bridge/host-loss wiring.

Probe scope excludes SwiftUI sheet lifecycle, nested folder chooser, form actions, writer results, same-window bridge rebinding, and native object deallocation. The separate full Save As tests and tiny-document GUI gates remain required. No parent-close failure was waived, no timeout was treated as success, and no product code was changed for this receipt.
