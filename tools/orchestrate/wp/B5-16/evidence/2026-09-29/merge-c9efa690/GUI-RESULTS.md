# B5-16 on-screen checkpoint — exact c9efa690 (main dd2cf300 + wp/B5-16-dither-ax 619676d0)

Isolated profile (--app-dir), focus trace focus-01.jsonl, Full Keyboard Access ON (AppleKeyboardUIMode=2). Real input via full-screen control.

PASS
- D8: Name → Tab → Load 3D LUT → Tab skips disabled Reset → Dither (focus ring on Dither). Trace seq 1-2 handled=false.
- D1/AX: Dither exposed as AXCheckBox "Dither", identifier document.properties.colorLookup.dither (nativeType DocumentDitherNativeCheckbox).
- D2 (decisive, previously FAILED at d0191c41): Tab from Dither → focus to Color section header; inspector AND sidebar stay visible. Trace seq 3: nativeType DocumentDitherNativeCheckbox, handled=false.
- D3: Shift-Tab returns to Dither, panels visible (seq 4 handled=false).
- D4: Space toggles Dither on, exactly one "Color Lookup" History row, no pan (seq 5 handled=false).
- D6: Cmd-Z restores Dither off and removes the row.
- N1 (implicit): Tab with non-owning focus toggles panels; Tab again restores.

NOT VERIFIED
- History H1/H2/H-act/H3 keyboard traversal: keyboard focus could not be placed on the History −/+/↺ buttons. Clicking the History header or rows does not make them first responder (SwiftUI KeyViewProxy, status unknown), and Tab from the Layers row eye NSButton is intercepted (seq 17 handled=true) by KeyRouter.swift:58 policy (only NSText/NSTextField/KeyOwningControl pass Tab) — pre-existing on main, not a B5-16 change. DocumentHistoryKeyboardTraversalTests pass in the automated gate.
- D5, D7, D9 (persistence), H4-H10 not run.

SEPARATE PRE-EXISTING FINDING (main): Tab from focused non-KeyOwningControl native controls (e.g. Layers eye button, toolbar tool buttons, titlebar sidebar toggle) toggles panels instead of moving focus.
Fixtures unchanged; app quit cleanly.
