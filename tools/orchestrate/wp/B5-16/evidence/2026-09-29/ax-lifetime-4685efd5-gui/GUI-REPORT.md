# Actual Inspector trace checkpoint — reproduction captured, keyboard failure remains

Exact 4685efd5 strict executable 3a74be76ebe92126e11326e88a615f4593d5a36a346c4dae0e7a79b5a0eff2d6 packaged uniquely as dev.tessera.inspector-gui.4685efd5. All signing/rpath/plist commands, before/after input hashes and complete package hashes are in manifest.json/commands.json. No rebuild. New owned profile /tmp/tessera-inspector-focus-4685efd5/profile and copied owned document/PNG/LUT only. No photo imported, global preference changed, forced focus, or Load activation. File > Open Document was used only to open the owned diagnostic document; Load chooser was never activated.

## Disabled run

Explicit launch documented disabled-launch.json; PID90280. Opened outputs/warm-gradient.tessera-doc, Color Lookup 1 selected, clicked Properties, clicked Name, sent Tab. OriginalPID/executable/fullargs verified via shell BEFORE CUA observation; external AX reported focused document.properties.colorLookup.load (Load 3D LUT). Sent second Tab, again verified same original PID/path/args before CUA. Result: Show Sidebar and Inspector off; document still open. No trace option used. Ordinary super+q followed by anchored absence exit1, no CUA after quit.

## Enabled run

Explicit same package/same profile launch plus fresh exclusive --inspector-focus-trace /tmp/tessera-inspector-focus-4685efd5/evidence/focus-01.jsonl, PID94492. Opened same copied document, Properties persisted. Clicked Name→Tab; shell verified original PID/path/args before CUA showed Load focused. Sent second Tab; shell again verified original PID/path/args before CUA showed Show Sidebar/Inspector off. Both observed sequences reproduce the failure; no observer crash or replacement was observed. All four post-Tab liveness checks preserved; they reduce the known relaunch hazard, not a zero-TOCTOU guarantee.

Raw two-event JSONL preserved verbatim. Event1 before first Tab: owned key/document window true, blocked=false, fullKeyboardAccess=true, native _SystemTextFieldFieldEditor, semantic AXTextField document.properties.name focused/matched/incomplete=false, handled=false.

Event2 before second Tab: SAME window identity ObjectIdentifier(0x000000010bc8e320); ownedKeyWindow/document true, blocked=false, fullKeyboardAccess=true; native SwiftUI.KeyViewProxy; semantic status=unknown, incomplete=true, primaryMembership=unproven, no role/identifier, semantics empty. Handler returned true. This captures actual interception with native KeyViewProxy, correlated with preceding external AX Load focus and resulting panel hiding. It DOES NOT establish Load ownership from the incomplete internal semantic trace, authorize broad KeyViewProxy exemption, or prove a routing fix. Unknown stays unknown. No Shift-Tab/Space/Return/History matrix claimed.

## Cleanup/provenance

Ordinary quit of enabled process via super+q; anchored shell absence exit1. No subsequent CUA, no third launch/profileless replacement in this attempt. Existing historical apps untouched; no claims about prior c5f8 default-profile effects (that exception remains preserved). No default profile cleanup attempted. Package files, preserved strict executable, copied document/PNG/LUT all have unchanged hashes; result JSON records comparisons. Raw trace SHA e5e7b63f9ae2e8178333374524d9d79566e41cce0192173db6e59a259f457743. Runtime lane released; remaining work is source diagnosis/repair under root coordination. Original c5f8 crash and 114273 lifetime failure preserved separately. Full native CUA observations retained in conversation; this report records relevant IDs/actions, not a fabricated full AX export.
