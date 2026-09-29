# B5-16 bounded current-base reconciliation

Request `5d86ed1a-53ad-40eb-a8db-dca97e44f94e`, source-only on B, 2026-09-28.
Branch `codex/b5-16-current-main`; frozen base `1e45baaead70b1bcd208afba800bcacd92213ee8` from
origin/codex/save-destination-a-qualification. Source checkpoint `374ea0ede8ef9fc8ca0206e8d33dbefafefffcdf`.
**All combined compile, tests and interactions UNRUN. No main acceptance.**
A reports native/full Swift703 (1 skip, 0 failures) +5 and strict for the BASE;
that evidence does not qualify this inspector source. Actual checked Save As GUI
remains A-owned and pending. A owns sole compiler/runtime/main integration.

## Commits and bounded scope

- `787f935ac2754df239602bbfd0dbbeb4db2bc4ca`: targeted layout/tab source and authored layout tests from 5d456ad9
- `3953e730fa84e7d55a4fce10382d5318ed9d1f4e`: persisted Auto/LUT editor deltas from 3f81f55b; no model duplicate
- `f17bc349717f792d9c4547780a7856c215949c7d`: 8184da17 Neutralize implementation/three regressions and runner/seven regressions
- `374ea0ede8ef9fc8ca0206e8d33dbefafefffcdf`: current shell matrix count reconciliation and honest runner isolation comment

Only sixteen source/test/harness files differ from the frozen base:

- `apps/mac/Sources/Tessera/Document/AdjustmentEditors.swift`
- `apps/mac/Sources/Tessera/Document/Channels/ChannelsPanel.swift`
- `apps/mac/Sources/Tessera/Document/DocumentController+Adjustments.swift`
- `apps/mac/Sources/Tessera/Document/DocumentHistoryPanel.swift`
- `apps/mac/Sources/Tessera/Document/DocumentView.swift`
- `apps/mac/Sources/Tessera/Document/DocumentWorkspace.swift`
- `apps/mac/Sources/Tessera/Document/Filters/DocumentFilters.swift`
- `apps/mac/Sources/Tessera/Document/LayersPanel.swift`
- `apps/mac/Sources/Tessera/Document/Tools/ToolsPalette.swift`
- `apps/mac/Sources/TesseraCore/Document/DocumentInspectorLayout.swift`
- `apps/mac/Tests/TesseraCoreTests/DocumentAdjustmentAnalysisTests.swift`
- `apps/mac/Tests/TesseraCoreTests/DocumentInspectorTabsTests.swift`
- `apps/mac/Tests/TesseraCoreTests/ShellLayoutTests.swift`
- `tools/orchestrate/wp/B5-16/run-selftests.sh`
- `tools/orchestrate/wp/B5-16/selftest_runner.py`
- `tools/orchestrate/wp/B5-16/test_selftest_runner.py`

`CURRENT-MAIN-INSPECTOR-SOURCE.sha256` pins all sixteen file bytes. The original
5d456ad9/3f81f55b/8184da17 and all old branches remain unchanged. Reused the clean
completed B5-16 checkout; no active build/runtime process was found using it.
No B builds, compiler, tests (including Python), apps, GPU, benchmarks, heartbeat
restart or writer replacement occurred. Source inspection and diff-check only.

## Reconciliation findings

- Stack/Properties/Channels sub-tabs and remembered selection; budgeted History
  with its own scroller/footer and persistent requested height; compact document
  tab overflow/status; bounded Layers controls and scrollable Tools palette.
- Auto uses persisted independent shadow/highlight clips for changes and analysis;
  LUT source filename/dither are model fields instead of ephemeral editor state.
- Legacy Match Color disabling Neutralize reanalyses only the legacy zero-chroma
  representation, fails honestly with missing source, and retains modern frozen
  statistics when flipping the persisted flag.
- Only conflict: ShellLayoutTests near old fixed24 case count/expected-failure
  block. Kept current Masks assertions, CI rule and dynamic whole-shell count;
  inspector-only count is sizes × tabs ×2. Former Document overflow now fails
  normally in the candidate, rather than being hidden as an expected failure.
  This is a stronger authored assertion, NOT evidence it passes.
- DocumentWorkspace diff is only eight lines for inspector-tab preference.
  Save/load/status/activation/ownership/cancellation implementations are untouched.
- Checked Save As presenter, sheets, backend protocols/adapters/commit helper,
  Stub backend, models, AppModel and generated Smart Preview binding bytes match
  the frozen base. No Rust or generated/archive updates; no stale replacement.
- DocumentView/ToolsPalette retain the later Transform attachment/ownership/options
  integration. AdjustmentEditors retains the inline synchronous MainActor
  Sendable checkbox setter (no direct isolated-function conversion).
- No duplicated B5-16a model fixes. Deliberately excluded old sheet-size changes,
  background window-host fallback and activation changes, historical screenshots,
  acceptance claims, broad old shared-file replacement and old runtime evidence.

## Authored tests (all UNRUN on this combination)

- DocumentInspectorTabsTests: five pure tests for labels/shortcuts, height budget,
  History clamping, opacity/fill width threshold and capped document strip.
- ShellLayoutTests: three restored inspector-specific tests (every tab/History
  state/size with region/footer bounds; eight-document overflow; key equivalents),
  plus stricter existing whole-shell containment preserving current Masks checks.
- DocumentAdjustmentAnalysisTests: three restored8184da17 regressions: real native
  legacy reopen/disable/undo/save; missing source refusal; modern frozen statistics
  without pixel read. Existing base Auto unequal-tail/model tests are preserved.
- test_selftest_runner.py: seven restored tests for exact completion, hidden FAIL,
  missing/duplicate/wrong summaries, exit/timeout, Transform inclusion, alternate
  checkout routing and direct-child ownership. NO Python test was executed.

Runner limitation: --nonactivating requests accessory launch, but current app
self-tests can independently raise windows. The runner does not enforce desktop
isolation. Old self-tests may assume Properties/Channels are simultaneously
visible; this bounded reconciliation does not transplant their broader legacy
background window/timeout/navigation changes. A must review/adapt that harness or
use its isolated GUI procedure; do not run the default heavy suite blindly.
Neither runner inclusion nor a zero-failure summary proves Transform completion.

## Interaction acceptance matrix — every row UNRUN

| Area | Required observed scenario | Status |
| --- | --- | --- |
| History sizing | Expand/collapse all tabs at960×600,1280×800,1440×900,1728×1117; drag both extremes, preserve usable tab minimum | UNRUN |
| History reset | Double-click resize boundary restores default requested height; no overlap or accidental collapse | UNRUN |
| History persistence | Change height and expanded state, switch document/tab, quit/relaunch; stored request survives while current window clamps rendered height | UNRUN |
| Scrolling/footer | Long layer/channel/history/snapshot lists; scroll Properties/Tools; New Snapshot and Layers/Channels actions stay reachable and outside scrollers | UNRUN |
| Compact overflow | Eight documents, long/dirty names, current first/middle/last at960/1280; open overflow, switch/close; toolbar and compact status remain inside window | UNRUN |
| Selected editor/tab continuity | Switch tabs and documents during normal edits; preserve selected layer, correct editor/value and global remembered tab; no stale controller write | UNRUN |
| Auto clips | Distinct shadow/highlight values, reanalyse/mode change, undo/redo, save/reopen; values and frozen statistics remain coherent | UNRUN |
| LUT editor | Load named LUT, toggle dither, reset, undo/redo, save/reopen; filename embedded provenance and dither persist; cancelled picker makes no edit | UNRUN |
| Neutralize | Legacy reopen/on/off and missing source, modern on/off without resampling; undo and actual saved reopen | UNRUN |
| Shortcuts | Control1/2/3 with each tab and focused editor, document shortcuts and tool shortcuts; no text-field leakage or newer Transform ownership regression | UNRUN |
| VoiceOver | Tab names/selected state, History expanded value/resize discoverability, footer and overflow navigation; actual keyboard/AX usability not inferred from identifiers | UNRUN |
| Checked Save As compatibility | Existing typed collision/Replace, cancel/queued successor and window ownership continue to satisfy A GUI gates | UNRUN; A-owned |
| Transform completion | Separate tiny-fixture start/apply/cancel/PSD completion and clean process drain under A resource admission; preserve historical timeout/missing-start failures | UNRUN; not cleared by inspector work |

## A handoff gates and limits

Review source/hash manifest; use coherent artifacts from pinned base as a starting
point (no FFI API change here), but record exact source/artifact inputs for fresh
strict compiler, focused inspector/analysis/adjustment tests, adjacent Document
save/load/resource tests and full Swift. Python runner regression tests also
remain UNRUN. Actual small-window/footer/History/AX and Transform acceptance are
separate gates above. Preserve original failed evidence and skips; do not infer
performance, large-image, cross-camera or whole-package completion. A alone may
integrate main after its fresh gates.
