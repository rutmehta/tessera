# History failure diagnosis and test-isolation correction — UNRUN

Request f8417c10-0861-4a6a-946b-87020147df30.
Separate branch codex/b5-16-history-test-isolation from e2c3cd03.
Test correction: 12e9f1ff4205b6a173abd9e85a4575ee51a61fb2.
No production changes, rebasing or changes to A's combined checkout.
Inspector ddb28101, runner1972f603 and accessibilitye2c3cd03 remain preserved.

## Evidence retained and interpretation

Main3a395c69 retains the original exact A1d361fa3 run:65 tests,22 failed
assertions, raw log/exit/command/freeze/review at
`tools/orchestrate/wp/B5-16/evidence/2026-09-28/history-1d361fa3-red`.
The log includes14 History assertions and8 later DocumentKeyRouting assertions.
A separately reports keyboard-alone15/15 passes and plain-AppKit reproduction of
AXUnknown from NSButton's direct role getter, false direct accessibilityPerformPress
return despite one target call, and NSTextField returning its actual displayed
`168 pt` value instead of the manually assigned `168 points` string.

Source traces the original hosted test through ShellHarness.window -> ContentView
-> DocumentView.onAppear into DocumentTools, Channels, Vector, Text and Transforms
shared workspace attachment. Tools also starts outline work; Text installs
activity observation. Those global attachments were not restored by the old
window teardown. This is a concrete source exposure consistent with the combined
failure, NOT independently proved runtime causality on B. No KeyRouter or shared
ownership production fix is justified by this evidence alone.

## Bounded correction

Only DocumentHistoryHeightControlTests.swift changes:

- Each test captures prior activation policy, five shared workspace owners (held
  strongly), and the height/expanded/tab preference values including absence.
  Method defers remove hosting controllers and close windows; tearDown then checks
  owner identity and restores captured state, never blindly nils an existing owner.
- The preference test hosts actual DocumentInspector directly with a local stub
  DocumentWorkspace and Stack tab, avoiding unrelated DocumentView lifecycle,
  viewport and singleton attachment/outline activity. AppStorage/control wiring
  remains real. It checks shared owners stayed unchanged, write/recreation/reset
  still work, and local column containment remains an assertion.
- A new state-restoration regression begins with non-nil prior owners, replaces
  them and preferences, then checks identity/value restoration including an absent
  preference. Existing prior outer state is restored in defer.
- Direct AX press calls remain, but assert actual target callback/value sequences
  rather than return Bool. Missing actions still fail. The ungrounded direct
  NSButton role getter assertion is removed; external role is still mandatory.
- Label checks remain; current value checks use native display `N pt`. All four
  redundant String downcasts are removed. This is not an assertion that VoiceOver
  speaks a particular expansion; external speech/value must be checked by A.
- Keyboard focus/Space/Return/keypad Enter, clamp/reset/no layout write, callback
  replacement/teardown, preference restoration and containment coverage remain.
  Existing ShellLayoutTests and other keyboard tests are unchanged.

Production control/readout/KeyRouter/shared ownership code stays byte-identical
to e2c3cd03. No role override, forced press success or cosmetic AX workaround is
introduced. There is no new spoken-value product refinement without external
native evidence. Direct in-process methods do not prove external AX behavior.

## Pending gates

All SIX corrected/new History tests are UNRUN on B. A must rerun the original
combined filter/order (including the later keyboard suites), compare keyboard
alone/combined and verify restored owner identities. Source inspection is not a
passing isolation test. Retain original65/22 evidence and any new failures.

Fresh compiler/strict checks should confirm the four downcast warnings are gone;
other warnings in the preserved log were not silently fixed or suppressed here.
Then actual external AX role/action/name/current value, keyboard traversal and
visible focus, preference persistence, pointer reset/clamp and compact footer
layout remain mandatory GUI gates under HISTORY-ACCESSIBILITY-HANDOFF.md.
The narrow inspector fixture does not replace whole-shell layout/GUI acceptance.

Only source reads/edits, diff-check, SHA256 and required Git-mailbox coordination
ran on B. No compiler/tests/apps/GPU/benchmark/heartbeat/writer changes. No Python
runner/test execution. A owns serialized runtime qualification and main merging.
`git diff --check e2c3cd03` passes. Exact changed-test and unchanged-product hashes
are in HISTORY-ISOLATION-SOURCE.sha256.
