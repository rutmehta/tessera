# Dither-origin Tab checkpoint — failed

Actual GUI at exact d0191c41859171dc28ef4114891ed4a4c51084b6 reproduced unexpected panel hiding on Tab originating from visibly focused Dither. Stopped at the first unexpected result; Dither Shift-Tab/Space and History were NOT run. No local correction or broader probes.

Used existing independently qualified strict package; source executable SHA ed910972bcaa6b18c323c7fc5e03e4ec36969634f786073271ffd1821a4fa390. Full-suite relink executable was not used. PREPARATION.json binds unchanged package files and exact new copied fixtures. Explicit app-dir `/tmp/tessera-inspector-dither-d0191c41/profile`; copied document `/tmp/tessera-inspector-dither-d0191c41/outputs/warm-gradient.tessera-doc`. Raw new exclusive trace `/tmp/tessera-inspector-dither-d0191c41/evidence/dither-01.jsonl`, copied here after quit. Launch.json records exact open-n argument array.

Original PID741 was verified for exact path and explicit app-dir/trace arguments after every Tab and before CUA observation. Opened copied document through native UI, selected Properties for Color Lookup; initial Dither value1, loaded owned-warm.cube, clean document. No document mutations. One initial attempt referenced an AX element ID invalidated by a full-tree refresh; tool rejected it before input, then current IDs were used.

Observed sequence (separate AX snapshots):

1. Name→Tab: Load focused.
2. Load→Tab: Reset focused.
3. Reset→Tab: **Dither checkbox focused**, ID document.properties.colorLookup.dither, value1. `tab3-dither-focused.txt` and `dither-focused.png` capture pre-event state.
4. Dither→Tab: inspector and sidebar disappear; inspector toolbar toggle becomes off. `tab4-dither-origin.txt` and `tab4-panels.png` capture result.

Trace event4 records native SwiftUI.KeyViewProxy, `status=unknown`, `incomplete=true`, `primaryMembership=unproven`, empty semantics, `handled=true`; document/ownedKeyWindow/fullKeyboardAccess=true and blockedWindow=false. This correlates actual external AX Dither focus with an intercepted Tab, **not proof that the diagnostic semantic resolver identified the checkbox**. Native adapter events2/3 report handled=false; no broad claim that all native/SwiftUI controls share this issue.

Four-event raw trace SHA e429d84efe93662767ffb997b58b3cf4531c08fe0dfebfe5722b5529845ccd91. No crash or replacement process observed. After capturing first failure, ordinary Cmd-Q, then shell-only ps returned1/empty (quit.json). No CUA after quit. Runtime explicitly released.

Strict source/package/copied PNG/LUT/document hashes remain identical; GUI-RESULT.json records before/after. No global settings, other apps, user photos or History expansion operated. Existing completed LUT-native-button evidence remains preserved and is not invalidated by claiming this separate control passed.
