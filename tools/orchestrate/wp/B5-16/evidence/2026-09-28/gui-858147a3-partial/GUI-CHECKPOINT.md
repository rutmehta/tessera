# Inspector GUI checkpoint — 858147a3

Bounded actual native CUA qualification; incomplete acceptance because keyboard traversal failed. No source edits or builds during GUI work. Runtime lane released after ordinary owned-app quit and anchored shell absence verification. Root assigned the keyboard follow-up to B (request 7d3c60f6); remaining rows are pending.

## Exact artifact and isolation

Candidate: 858147a3183c98b9e15118495a75da3bc2ddd0d0. Preserved strict05 Release executable SHA256 f0f676ce47483531916923684b7c2ab817596d29b9c15c91c7e51363ec9d1171. Package provenance is strict05, not a claim that the separate full04 relink is byte-identical. Package script, input and output hashes, dependency/rpath listings, and signing commands remain in this directory. Copied bundle has a unique identifier, executable-relative Frameworks rpath and ad-hoc signing; original executable unchanged. All package-file hashes still match the packaging manifest.

Both launches used exactly:

```sh
open -n '/Volumes/betterSSD/tessera-validation/b5-16-history-858147a3/gui-858147a3/Tessera Inspector 858147a3.app' --args --app-dir /tmp/tessera-inspector-gui-858147a3/profile
```

Observed process identities: PID 99187 first launch, PID 7423 explicit relaunch. Each was quit using native CUA Command-Q. After the first quit an anchored package-path process query confirmed absence before the second explicit launch. After the final quit, only shell verification was used:

```sh
pgrep -fl '^/Volumes/betterSSD/tessera-validation/b5-16-history-858147a3/gui-858147a3/'
```

Exit 1, no output. No CUA request followed final quit. The package-preparation manifest's `launched:false` and external-volume `profile` describe its preparation snapshot; actual launch profile was the local path above. This checkpoint asserts observed explicit launches, not a global proof about every potential application state access. The earlier f1089 profileless automatic-relaunch exception remains disclosed in its original report and is not erased by this run.

Only owned procedural PNG, owned LUT and copied four-layer document were opened. `fixture-before.json` and `checkpoint-files.json` prove all three bytes unchanged after the run. The packaging script also hashed the existing Sony fixture read-only; this GUI run did not open that RAW. No user photo, other app, protected prompt or global setting was manipulated.

## Observed passes

- At an observed 1440×900 dark window, the 12-point History numeric readout fit with the chevron, decrease/increase/reset controls. AX exposed descriptive button names, point values and stable identifiers. AX click increased 168→192; pointer click increased 192→216. Subsequent individually observed increments reached 240 and 264. A repeated stale-index batch stopped after its first change, so it is not counted as 24 successful increments.
- Pointer drag reached the upper 492-point clamp with Increase disabled; downward drag reached minimum 80 with Decrease disabled. AX Reset returned to 168 with Reset disabled. History remained reachable while Properties content scrolled. No pointer double-click reset qualification is claimed.
- With requested height 492, shrinking to an observed 960×652 outer window displayed 244 and disabled Increase. `preferences-small-clamp.json` retained requested height 492: layout did not rewrite the preference.
- Collapsed History and selected Channels, ordinarily quit, relaunched as a separate process with the explicit profile, and reopened the same owned document through the native File/Open Document picker. Channels and collapsed History persisted. Expanding showed 244 in the small window; enlarging back to approximately 1440×900 restored 492. Final owned preferences record channels, height 492, History expanded.
- Reopened modern Match Color persisted source warm-gradient and checked Neutralize. This is actual saved-document reopen evidence, not only a model assertion.
- Reopened Color Lookup showed owned-warm.cube and Dither 1. Reset produced identity/None and an edited document; Undo restored the LUT and clean state; Redo returned identity/edited; final Undo restored the LUT and clean state. Dither remained 1. No save was performed, and final document hash equals the starting copied file.

Visible screenshots and AX states were observed through the native CUA conversation. This report does not claim those screenshots were persisted as external image files.

## Keyboard acceptance failure

Clicking History Increase changed the value, but established keyboard focus was not reported. Space did not change it, so keyboard activation cannot be accepted. From the Properties Name text field, Tab explicitly focused the `Load 3D LUT…` button in AX; the next Tab hid the inspector/sidebar instead of continuing traversal. Tab restored panels. Shift-Tab from Name focused Channels. No keyboard-navigation global preference was changed. One Alt-Tab trial inserted a tab in the name; it was immediately restored to `Color Lookup 1` and committed back, leaving a clean title and unchanged persisted file.

The decisive observed sequence is Name field → Tab → AX-focused Load 3D LUT → Tab → panels hidden. Cause and macOS keyboard policy interactions remain assigned for source diagnosis; this report does not assert a settled product cause. History keyboard acceptance and VoiceOver acceptance are not passing.

## Pending matrix

- Corrected History keyboard traversal/Space activation and focused-control AX recheck.
- Full light/dark tab/history layout matrix, including 1280 and 1728 widths; long Channels and snapshot content.
- Distinct long-name overflow tabs, settled selected identity, dirty-document Cancel retention, then ordinary close. Earlier rapid-close observations are not proof of an asynchronous product selection race; source audit found synchronous MainActor selection.
- Save As smoke on this candidate; legacy/missing-source editor cases; separate Transform workflow.
- Remaining interactions must be run under a new explicit runtime grant. No passing inference is made from older candidates or unrun rows.

Automated qualification remains independently verified separately: 67 focused tests, 4 layout tests, 720 XCTest cases (one expected performance opt-in skip) plus 5 Swift Testing cases, required Sony workflows and strict Release build. Those passes do not override this GUI keyboard gap.
