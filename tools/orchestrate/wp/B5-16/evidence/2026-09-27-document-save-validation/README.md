# Document save and Layers settlement validation — failed GUI acceptance

Candidate `5ad1cbcc078cefeb6945533cba8637a974f0de65` is based on `fee032e4`, with B's three save-test commits, save product and handoff, then Layers tests/product/handoff cherry-picked in order. The four changed Swift product/test files were byte-identical to B's `1c691c12` source checkpoint. The worktree was clean at gate launch; `git diff --check` was empty. The frozen inputs and exact hashes are in `source-manifest.json`. The existing ignored FFI archive remained a regular file with SHA-256 `8ab43f64cf8bd510ee17c4cb19c5fff58e6e488015905c0b84b2ddfa0dce3a03` before and after the gates.

## Focused Release gates

`run-gates.py --run` used `swift test --jobs 2`, the dedicated betterSSD scratch path, Release configuration and `-Xswiftc -enable-testing`. It required positive test counts and stopped on the first failure. Direct results, not inferred from command launch:

| Filter | Executed | Failures | Direct exit | Timeout |
| --- | ---: | ---: | ---: | --- |
| `DocumentSaveSettlementTests` | 9 | 0 | 0 | no |
| `DocumentLoadSettlementTests` | 4 | 0 | 0 | no |
| `testSaveAsNames\|testSaveOpenAndExport\|testEngineSessionThroughTheAdapter` | 3 | 0 | 0 | no |

The third filter includes the tiny native engine adapter test and is not described as CPU-only. Raw `.log` and direct `.json` files are retained beside this note. The log digests are in `evidence-manifest.json`. These unit results do **not** establish GUI acceptance.

## Isolated GUI attempt

The exact Release executable SHA-256 was `6fc86ac45796f1de33468ea95ee35ea8fe22063007977f48a466e11d18832ea5`; manual bundle wrapping, rpath addition and ad-hoc signing yielded executable SHA-256 `a304a1f2db5ea9ccea241fde4430d7ca065fb1d688b82ffba12ac38ad8e510b7`. `codesign --verify --deep --strict` passed for the unique bundle `dev.tessera.document-save-validation.5ad1cbcc`. `open -n --stderr …` returned LaunchServices error `-10810`; directly executing the bundle's `Contents/MacOS/Tessera` launched it successfully, and CUA identified the unique running bundle. This launch issue remains separate from the Save As failure.

The app used an isolated betterSSD `--app-dir` and a generated 32 × 32 JPEG. No existing preview app was activated, closed or repackaged. In CUA, the initial document window showed `tiny.jpg – 1 layer`, 32 × 32, 8-bit sRGB.

1. `⇧⌘S` opened the Save As sheet with `document.saveAs.name`, `document.saveAs.cancel` and `document.saveAs.save`. Clicking Cancel dismissed the sheet and left `tiny.jpg` open.
2. Reopening the sheet and pressing Escape once left the focused name field and sheet in place; the second Escape dismissed it. The document remained open. The first press appears consumed by the text field; no stronger cause is claimed.
3. Reopening, naming `tiny-validation.tessera-doc`, choosing the isolated `saved` folder in the native folder panel, then clicking Save dismissed the sheet. AX showed window title `tiny-validation.tessera-doc` and status `Saved tiny-validation.tessera-doc`. The file existed at 2,967 bytes, SHA-256 `d91487db16339f39fe6b82669a2d1be3080706c8afc1590df02763f93f5b7742`.
4. Reopening Save As with the same existing name and folder, then clicking Save dismissed the sheet. Subsequent CUA AX state and screenshot showed only the ordinary document window, no Replace/Cancel alert. The prior Saved status remained; no new write was observed.
5. A second controlled target, `already-exists.tessera-doc`, was created in that isolated folder as a byte copy of the saved file. Reopening Save As, entering that existing distinct name and clicking Save again dismissed the sheet without a Replace/Cancel alert. Its SHA-256 remained `d91487db16339f39fe6b82669a2d1be3080706c8afc1590df02763f93f5b7742`.

The exact GUI observations are also in `gui/GUI-RESULTS.json`. CUA screenshots were displayed inline in the validation conversation; this interface did not provide a file export API, so no portable screenshot file is claimed. The isolated app was quit and its process was absent afterward. Reopening the Save As sheet after a prior dismissal was observed, but **Replace Cancel could not be exercised** because the replace alert never appeared. The candidate therefore failed GUI acceptance. Source inspection suggests a possible timing interaction between setting `presentedSaveAs = nil` and immediately starting `NSAlert.beginSheetModal`, but that is an unproven hypothesis, not the recorded result. No source correction was made in this checkpoint.
