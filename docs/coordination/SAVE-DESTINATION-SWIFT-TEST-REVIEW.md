# Save destination intent: Swift integration test matrix

Source-only test plan reviewed against accepted main `11b31be6` and:

- `docs/coordination/SAVE-DESTINATION-INTENT-DECISION.md`
- `docs/coordination/SAVE-DESTINATION-INTENT-REVIEW.md`
- `docs/coordination/SAVE-DESTINATION-RACE-PLAN.md`
- `tools/orchestrate/wp/B5-16/SAVE-DESTINATION-INTENT-DESIGN.md`

No product edit, generated binding, build, or runtime test was performed. All cases below are **UNRUN** and depend on A producing the checked FFI API and the corresponding generated Swift binding. Test names are suggestions; adapt only to the actual generated spelling while preserving the assertions.

## Contract to pin

The additive Swift surface is `DocSaveDestinationIntent.createIfAbsent` / `.replaceConfirmed` and `DocSaveAsResult.saved` / `.destinationExists`. `DocumentBackend` keeps legacy `saveAs(path:)` and `save()` behavior. `EngineDocumentBackend` maps the generated enum values explicitly; a typed destination conflict is never inferred from `BridgeError` text. `StubDocumentBackend` implements the same checked semantics with real filesystem publication.

The UI captures intent for one `DocumentSaveOperation`: absent when Save is submitted means `createIfAbsent`, retained across form-sheet completion/drain and writing even if a file appears later. An existing target stays uncommitted through form drain and the Replace prompt. Only the affirmative completion belonging to that operation's actual Replace presentation grants `replaceConfirmed`; the writer starts after that invocation's completion and physical sheet detachment. A destination conflict is terminal for that attempt and must not be upgraded or retried as Replace.

## Focused cases

| Area / suggested test | Deterministic setup and action | Required assertions |
| --- | --- | --- |
| `EngineDocumentBackendTests.testCheckedSaveAsMapsTypedResultsAndPreservesOtherErrors` | With a small real `DocumentSession`, exercise checked save to an absent path, then an occupied path, then a path whose parent directory does not exist. | First result maps to `.saved`; existing-path result maps to `.destinationExists`; missing-parent I/O remains a thrown error through `bridged`, not `.destinationExists`. No localized string matching and no change to legacy API signatures. |
| `DocumentSaveDestinationCommitTests.testStubCreateIfAbsentDoesNotReplaceAndLeavesDocumentUnsaved` | Dirty a stub document. Save checked to a path containing sentinel bytes, then to an absent path. | Existing target reports `.destinationExists` and sentinel bytes, path/title, and prior saved marker are unchanged. The dirty document remains dirty. Absent target reports `.saved`; PSD/PSB checked saves remain unsupported on the stub. Do not require history head to remain identical on conflict: committing a pending edit into history before the attempted snapshot is an accepted implementation detail. |
| `DocumentSaveDestinationCommitTests.testStubRecordsCapturedHeadNotEditsMadeDuringPublication` | Use a per-call test barrier after a checked save snapshots the document but before publication. While held, make one later edit, then release. | Published file represents the captured snapshot; backend saved head advances only to that captured head, so the later edit remains dirty. No global/shared mutable barrier. |
| `DocumentSaveDestinationCommitTests.testOrdinarySaveResolvesCurrentPathAfterTakingSaveGate` | Start checked/legacy Save As from path A to B and hold its writer after the save gate is acquired. Start ordinary `save()` while held, then release the Save As. | The queued ordinary save writes the now-current path B, not stale path A. Assert through distinct snapshot bytes or the narrow injected writer destination; the test must establish the order with a latch, not sleeps. This pins the accepted gate-before-path-read ordering. |
| `DocumentSaveSettlementTests.testAbsentIntentSurvivesFormDrainAndLateFileAppearance` | Use the new typed checked writer seam plus a fake native driver that independently controls Replace response, begin completion, and sheet membership. Submit Save while `saveFileExists` is false, then make it true before draining the form. | No write before both actual begin completion and membership clearance. The single eventual write carries `.createIfAbsent`; it does not prompt for Replace or upgrade intent. Return typed collision once. |
| `DocumentSaveSettlementTests.testExistingTargetRequiresMatchingAffirmativeReplaceDrain` | Submit with an existing destination. Keep the form invocation and then the Replace invocation separately controllable; replay stale form events and duplicate/wrong-token Replace completion before the real matching response. | No intent/write while form drains or Replace is pending. Cancel, stale, duplicate, wrong request, or wrong controller grants no Replace intent and starts no write. Only matching affirmative Replace completion **and** physical detachment admit one `.replaceConfirmed` write. |
| `DocumentSaveSettlementTests.testLateDestinationConflictDoesNotAdvanceSaveOrContinuation` | Let an absent-path attempt reach its checked writer after the native form is fully drained; return `.destinationExists`. Initialize `lastSaveFolder`, document path/title, dirty state, and a continuation callback to known values. | One `.destinationConflict(url)` outcome and visible latest-request conflict status; no Saved status, folder advance, model reload-as-saved, path/title/saved-marker advance, `then` callback, or close/quit continuation. The attempt does not automatically present Replace. A later fresh Save As may choose another path or affirmatively Replace. |
| `DocumentSaveSettlementTests.testConflictRemainsConflictWhenCallerCancelsDuringWrite` | Hold the checked writer after admission, call `cancelDocumentSave`, then complete the admitted write with typed destination conflict. | Exactly one conflict outcome; never `.saved(... continuationCancelled: true)` or synthetic cancellation. Verify folder/status/continuation follow the conflict path. Keep actual successful writer cancellation as a separate compatibility control. |
| `DocumentSaveSettlementTests.testLegacyHeadlessSaveAsRetainsReplacementCompatibility` | Keep the existing legacy headless Save As test and add a pre-existing destination control through the legacy writer/backend route. | Legacy caller continues using replacing `saveAs(path:)`, success callback only follows actual save success, and no checked intent is silently inferred or required. |

## Gate order and boundaries

1. Run mapping and stub filesystem/marker tests after generated bindings are refreshed from the actual A API. Preserve the old FFI archive; never test against a stale archive with new bindings.
2. Run the focused settlement matrix with independently latched native completion and sheet membership. A fake presenter driver is appropriate for the UI intent decision; it does not replace native presenter or native no-clobber acceptance.
3. Run adjacent backend, save settlement, and document-mode tests, then the full Swift Release suite on the same frozen source/archive if required by the integration gate.
4. Keep actual atomic no-clobber races, native/PSD/PSB bytes, and native saved-head checks in A's Rust/FFI suite. Swift cannot prove the filesystem publication primitive by inspecting a pre-write `exists` check.

## Source assumptions and limits

At `11b31be6`, `DocumentWorkspace.finishSaveAs` still records only `needsReplacement`; `admitDocumentWrite` accepts a `Result<Void, Error>` writer and updates `lastSaveFolder` on success. The checked intent/result types, explicit Engine adapter mapping, stub save gate, typed Swift writer seam, and `DocumentSaveOutcome.destinationConflict(URL)` therefore must arrive before these UI tests can compile against their final signatures. Do not add a default protocol implementation that forwards checked intent to legacy `saveAs(path:)`.

Accepted behavior does not require identity-CAS for `replaceConfirmed`. Legacy `save()` and `saveAs(path:)` keep their replacing behavior. Stub conflict tests should preserve destination bytes and not mark the document saved, while allowing the pending edit to have become a history node before snapshotting. None of these Swift cases claims power-loss durability, filesystem-wide identity comparison, or PSD/PSB parity on the stub.
