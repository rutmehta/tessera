# History height accessibility — source-only handoff

Request81b15b38-4187-4190-8485-0e7f1e37099e, 2026-09-28.
Branch codex/b5-16-history-accessibility, exact base
`ddb2810196d5492c9258840af9a431e706d9318f`.

Tests `553dc70d0d065be7ce5c5d3955b274a934028795`.
Product `288d728ac7403c8d55d363a163728437ed8ee271`.
All five new tests and all new AX/keyboard/layout interactions are **UNRUN on B**.
A owns compiler, runtime, actual CUA and main. A's observed f1089ba5 pointer
clamping/reset success and missing AX handle motivate this change; neither those
observations nor A's separate runner12 pass qualify this new source.

## Bounded implementation

Native NSButton decrease/increase/reset actions and a native current-height
readout sit beside the History collapse toggle within its existing fixed header.
The SwiftUI bridge passes the existing $historyRequested AppStorage binding;
there is no second preference, migration, settings toggle or changed default.
Increment/decrement use one existing Theme.Height.row step from the *displayed*
clamped height, then the same DocumentInspector.budget.historyHeight function as
the pointer path. Reset writes raw historyDefault, like double-click, even when
the current column must clamp its rendered size. Initial layout/updates never
write back a clamped preference. Buttons disable at their bounds.

The controls expose native button roles and press actions, descriptive labels,
current value in points and a labelled text readout; value-change notifications
follow action commits. A KeyOwningControl NSButton subclass accepts focus and
handles local Space/Return/keypad Enter without routing those keys to viewport
panning. No global shortcut, key monitor, VoiceOver/system setting, timer or
poller is added. Other keys retain existing native/control routing. The bridge
refreshes its binding callback on update and releases it on dismantle.

Pointer handle/drag/double-click implementation and identifiers are untouched.
Header height, History minimum/clamp, footer budget and collapse preference are
unchanged. All existing ShellLayoutTests assertions remain byte-for-byte intact.
Only DocumentView.swift plus new DocumentHistoryHeightControl.swift and its new
test file differ from ddb28101, excluding this handoff/hash manifest. There is no
shared resource, Save As, Smart Preview, Rust, generated or runner change.

Frozen branches remain codex/b5-16-current-main at ddb28101 and
codex/b5-16-runner-admission at1972f603. This branch intentionally starts at the
requested ddb28101; A must retain its separate runner increment when composing
integration. No source rebase or original candidate mutation.

## Tests authored, not executed

DocumentHistoryHeightControlTests has five tests:

1. Native AX roles/labels/current value and actual accessibilityPerformPress
   calls change state through increase/decrease/reset targets. Missing actions
   fail outcomes, rather than merely checking identifiers.
2. Clamped increment/decrement, disabled bounds, and reset-to-default while layout
   preserves the original requested preference (including oversized requests).
3. Native focus and Space/Return/keypad Enter actions; KeyRouter defers to the
   focused control and does not start viewport panning.
4. Reconfiguration replaces callback without a write; dismantling stops future
   callbacks from the removed control.
5. Actual hosted inspector discovers native controls; AX action updates the
   existing UserDefaults key, recreating the inspector restores the value, reset
   writes the default, and column containment still passes as an assertion.

No compile, test, Python runner/test, app, GPU, benchmark, heartbeat restart or
writer change on B. Only Git mailbox utility execution for required coordination,
source reads/edits, SHA256 and git diff --check. Diff-check passed. No new passing
runtime claim is made. Exact source/test bytes: HISTORY-ACCESSIBILITY-SOURCE.sha256.

## Required A acceptance — all pending

- Compile the focused new file/test and run the five tests against coherent base
  artifacts, then existing inspector layout/shortcut and relevant keyboard suites.
  Retain warnings/errors; full strict/full suite belongs to A's admission lane.
- In isolated actual app at960x600,1280x800,1440x900 (light/dark), expand History
  in each inspector tab. AX must expose Decrease/Increase/Reset History height as
  buttons and a meaningful current value in points. Check enabled states at both
  bounds, no duplicate misleading AX element, and readable/announced updates.
- Traverse focus using the user's existing keyboard navigation configuration;
  Space/Return should actuate the focused control once. Confirm visible focus,
  native traversal to/from collapse/footer controls and no viewport pan, document
  switch or culling action. Do not toggle global keyboard/VoiceOver settings to
  hide a reachability failure. Record any actual focus/AX failure as a blocker.
- Increase/decrease at normal and clamped sizes; resize the window after storing
  an oversized request and verify layout alone did not overwrite it. Reset then
  reopen/quit-relaunch should retain default request and restore display clamp.
- Repeat pointer drag and double-click reset from A's earlier successful scenario;
  verify new readout follows it and keyboard edits change the same history body.
- Collapse hides height controls; expand restores them and saved value. Verify
  snapshots/History footer remain reachable, minimum tab content and controls do
  not overlap, and document/editor selection survives tab changes.
- Real VoiceOver usability/speech, physical keyboard focus and layout remain
  distinct from unit-level press actions. No acceptance is inferred from labels,
  source tests, previous pointer success or a clean build alone.
