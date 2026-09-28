# Terminal-probe validation — failed GUI

Candidate `e8ddabfa71fd6f4243b6bcaac0f529a15a1cf379` is the B terminal-probe candidate `88de50ea` plus one A-only SDK compilation repair. The initial frozen `88de50ea` gate exited 1 with zero tests because `NSWindow.didBeginSheetNotification` is absent from the installed SDK; raw log and JSON remain under `/Volumes/betterSSD/tessera-validation/document-save-settlement/terminal-probe-88de50ea`. Installed `NSWindow.h` declares `NSWindowWillBeginSheetNotification` and `NSWindowDidEndSheetNotification`. The repair removed only the invalid begin observer, retaining the existing probe attachment capture, didEnd/willClose observers, and terminal parent-clear fallback.

The repaired frozen e8dd gate passed five Release stages with direct exit 0 and no timeout: 30 `DocumentSaveSettlementTests`, 3 `DocumentSaveSheetProbeTests`, 3 `DocumentSaveSheetAttachmentTests`, 4 `DocumentLoadSettlementTests`, and 3 adjacent tests, 43 total. Raw logs, exact commands/results, and source manifest are in `/Volumes/betterSSD/tessera-validation/document-save-settlement/terminal-probe-e8ddabfa`. The tested Release executable SHA-256 was `54542971c13275c73f087eafcd002e238ccb8a2b724afd92265758af34f487be` before signing. Unique package details are in `gui/package-exact.sh` and `gui/package-hashes.txt`; package bundle ID `dev.tessera.document-save-terminal.e8ddabfa`, isolated `--app-dir` `/Volumes/betterSSD/tessera-validation/document-save-settlement/terminal-probe-e8ddabfa/gui/app-support`.

Real GUI failure on the isolated package:

1. File > New Document created a generated 32 × 32, 8-bit sRGB, one-layer Untitled document.
2. File > Save As showed the actual native Save As sheet with name, format, folder, Cancel, and Save controls.
3. Cancel returned to the Untitled editor; it remained open and unsaved.
4. Two subsequent File > Save As menu invocations left the editor visible with no sheet in accessibility state or screenshot. The menu action stayed enabled. Escape followed by a third Save As also produced no sheet.
5. No output document was written. The generated Untitled document was closed through its normal “Don’t Save” alert, and the isolated app quit. The normal editing preview and earlier failed validation packages were untouched.

This fails repeated Save As / queued successor after Cancel. Native Replace, Escape *on the sheet*, confirmed overwrite, reopening saved output, abrupt parent loss, and preappearance races were not reached in this run. The focused unit cases prove their simulated contracts only; they do not waive the observed native ordering failure. No product repair was made during GUI validation.
