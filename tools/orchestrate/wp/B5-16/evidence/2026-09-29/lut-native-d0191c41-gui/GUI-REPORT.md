# d0191c41 bounded LUT GUI acceptance

**Observed bounded checks passed.** No product failure or crash occurred. This is not full inspector acceptance. The runtime lane was released after ordinary quit and shell-only original-PID absence verification. No CUA access occurred after either quit.

## Provenance and isolation

Exact source `d0191c41859171dc28ef4114891ed4a4c51084b6`; independently verified strict executable SHA `ed910972bcaa6b18c323c7fc5e03e4ec36969634f786073271ffd1821a4fa390`. `package-exact.py`, `commands.json`, dependencies and manifest record copied Sparkle, executable-relative rpath insertion and ad-hoc signing. No rebuild or source changes. Original preserved executable, packaged bytes and copied fixtures match before/after hashes.

Unique bundle `dev.tessera.inspector-gui.d0191c41`; explicit launch app-dir `/tmp/tessera-inspector-lut-d0191c41/profile`. **Fixture base is `/tmp/tessera-inspector-lut-d0191c41`**, not the unused package `profile` directory in manifest.json. `fixture-before.json` and GUI-RESULT.json give exact paths and hashes for PNG, LUT and document. Existing apps/user photos were not operated on; no global settings were changed. Appearance changes used this app's View menu. This report claims only the observed explicit launches and preserved files, not a baseline comparison of unrelated default-profile state.

Disabled process 57471 and enabled process 63036 were launched with the exact arrays in disabled-launch.json/enabled-launch.json. Each original PID/path/args was checked after every Tab/Shift-Tab before UI observation. Both quit through Cmd-Q; disabled-quit.json/enabled-quit.json record `ps` exit 1 and empty output. No replacement launch.

## Actual observations

- Trace disabled: Name→Tab focuses Load; second Tab focuses Reset with panels visible. Shift-Tab returns Load.
- Trace enabled: same sequence. Event 2 reports native `Tessera.DocumentInspectorNativeActionButton`, focused matched AXButton, same owned document key window, `handled=false`. External AX confirms Reset focus afterward. Unlike the earlier unknown semantic trace, this event provides matched native/semantic evidence.
- Space, Return and keypad Enter on focused Reset each produced identity LUT, disabled Reset and one additional visible Color Lookup History entry. One Undo after each restored `owned-warm.cube` and clean document; History retains later states, so totals advance 1/2/3 while Opened becomes selected on Undo. This proves observable edit/Undo behavior, **not exact callback counts**; the separate unit tests establish callback counts. Trace key codes 49/36/76 all report native adapter and handled=false.
- With identity LUT, Name→Load→Tab skips disabled Reset to Dither without hiding panels. No Dither-origin traversal acceptance claimed.
- Return on Load opened the actual LUT chooser. Escape canceled without selecting any file; original LUT and clean document remained.
- Clicking the viewport then Tab hides panels; second Tab restores them. Trace events 14/15 are `DocumentViewportView`, handled=true.
- App-only Light and Dark show readable LUT labels and controls. Initial window screenshots are 1440×900; resized compact screenshots 1120×760 (right splitter at x824). Buttons fit without overlap/clipping; Load focus ring visible in dark and light screenshots. This is observed window geometry, not an asserted exact 288-point content measurement.
- Enabled final Shift-Tab from Load selects Name text. Trace event17 is native adapter, Shift modifier, handled=false.

The copied document was not saved; all edits were undone before ordinary quit and its bytes remain unchanged.

## Evidence and limits

Raw trace `focus-01.jsonl`: 17 events, SHA `f94d632a9a5153d6356717078c8efd0c5757db4d7bc60d73b806b6321f698b9a`. AX snapshots and per-event original-process guards are alongside the screenshots. Initial Go To chooser interaction had an input/clipboard timeout and several ineffective actions; Escape exposed the already-resolved owned file, then Open succeeded. This was setup friction, not classified as a product failure.

Not observed: held-repeat behavior/callback counts, transient hover/pressed frames, full VoiceOver, remaining History/whole-document matrix. These remain pending or covered only by separately identified automated tests. No broadened claims from the LUT fix.
