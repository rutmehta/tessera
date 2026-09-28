# Current-main checked Save As qualification — prepared, UNRUN

Checkout /Users/rutmehta/.codex/worktrees/render-resource-bounds/tessera, branch codex/save-destination-a-qualification at B cfb511e5. No compiler/app/GPU workload executed in preparation. Root must explicitly hand off execution lane before any --execute invocation. One gate at a time, inspect exit/failures before continuing. Commands are argv arrays in gates.json; run.py without --execute only prints selected command.

Example after lane handoff: `python3 run.py 01-native-commit --attempt 01 --execute`. Every attempt creates a new directory, refuses overwrite, records exact command/env/source/artifact before+after manifests, direct exit, wall time and source drift even for failing commands. New repairs require new attempt number and fresh hashes. Do not treat compilation failure as behavioral RED. Gate commands use Release, locked Cargo, shared assigned BetterSSD target, jobs2/Rayon2/deployment15; Swift uses this directory's separate swift-build scratch and --jobs2. Inherited TESSERA_* flags are removed to avoid accidental benchmark/GUI/probe activation. The preserved Sony TESSERA_SMART_PREVIEW_RAW is explicitly restored for test gates after inherited flags are removed; full Swift must report both SmartPreviewNativeWorkflowTests passed, otherwise runner fails. The fixture is hashed before/after and unexpected changes fail. Other opt-in probes, including20k performance, stay disabled and skips are reported honestly.

Preparation preserved all preexisting ignored apps/mac/build/ffi files under inherited-ffi/ and verified their hashes against prepared-source-and-artifacts.json. Existing ignored .superpowers and fixtures/raw remain untouched. No historical archive should be mistaken for current combined qualification. Regeneration will intentionally replace checkout ignored FFI outputs, but originals are recoverable from this evidence directory. For every non-regeneration gate, any ignored FFI artifact hash drift fails the runner. No clean/reset command is part of the plan.

## Gate order

1. 01-04: private native exclusive destination publication; public real-session create/replace/collision and dirty identity; native roundtrip; PSD/PSB preservation. These are concrete behavior tests, not only API compilation. Inspect actual test counts and no-match/ignored output before accepting each exit0.
2. 05-08: full document integration, full FFI lib, all-target FFI strict Clippy and workspace format check. Retain any existing unrelated failure distinctly; don't silently widen fixes.
3. 09 regenerate bindings+archive together via actual build-ffi.sh. Only three tracked generated files are allowlisted for change, and all changes are recorded for source review. Check saveAsChecked/typed enums/checksums AND all current Smart Preview methods survived. Inspect ignored archive/generator hashes; accept the combined generation explicitly before Swift gates. Never reuse81cc08eb binding/archive bytes.
4. 10 focused Swift: DocumentSaveDestinationCommitTests, DocumentSaveSettlementTests, EngineDocumentBackendTests. Includes actual Darwin collision/fallback/stage identity, captured history-head/serialization and typed real-engine mapping. Review failures rather than altering tolerances/expectations to pass.
5. 11 adjacent presenter/sheet attachment/load settlement/status, outline lifecycle and transform suites. Engine backend history coverage is already in10. Actual native-sheet probes have their own opt-in conditions; never infer executed from skipped probes.
6. 12 full Release Swift and13 strict complete-concurrency warnings-as-errors product. Capture final executable hash after the LAST relink; strict and test product binaries can differ. Package only a proven exact final artifact, without invoking another build.

Meaningful regression evidence: current tests reference new API, so reversing implementation wholesale can produce compile failures. If root requires fresh behavioral RED, prepare a separate isolated source snapshot with the checked signature/typed result still compiling but a narrowly reverted replacing-publication seam. Run only real sentinel/inode collision tests in that explicitly authorized source wave, preserve failure and restore exact qualified production bytes before green. Do not modify the active checkout opportunistically or claim historical/compile-only RED.

## Later actual GUI acceptance

Unique owned bundle/profile and disposable small document source only; --app-dir local profile avoids conflating the known external-volume preferences startup issue. All actual form submission, replacement confirmation, retry and reopen via native CUA. Preserve existing apps/globalsettings/protected prompts. Do not use backend test hooks as a substitute for GUI evidence.

- Save As choose a new owned name; introduce a sentinel file only at an authorized deterministic pre-publication boundary if one can be observed. Preserve sentinel bytes+inode when create-if-absent conflicts. If an actual late boundary cannot be controlled without instrumentation, label it native-test coverage and do not claim GUI race coverage.
- Confirm collision does not advance document path/title/folder, Saved status, continuation or clear dirty state. Retry a different owned name, then reopen actual saved contents.
- Repeat with an already-existing owned sentinel and affirm actual Replace prompt. Verify new saved content and expected path/title/dirty/continuation outcomes. Reject/cancel remains non-success.
- Record stage leftovers/diagnostics honestly; no universal cleanup, hostile-directory identity-CAS or parent-directory power-loss claim.
- Ordinary quit only owned app, preserve test artifacts/manifests and report lane release.

Ready for root review and serialized execution; no gate has run yet.
