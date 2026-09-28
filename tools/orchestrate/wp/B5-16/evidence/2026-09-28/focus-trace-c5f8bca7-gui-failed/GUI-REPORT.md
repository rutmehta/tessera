# Actual focus diagnostic: failed / inconclusive

Candidate c5f8bca7, strict02 preserved executable SHA256 7a26729cb661fbece69850a5687fcf55789c4e7c9d973a43169cd23b2e49920f. Packaging/signing/rpath changes are recorded in manifest.json and commands.json. Same unique signed package was used for both runs; all package files and three copied fixtures/document remain hash-identical after the diagnostic. No builds or product edits occurred during GUI work.

## Disabled sequence

Explicit launch: open -n [owned package] --args --app-dir /tmp/tessera-inspector-focus-trace-c5f8bca7/profile; PID 7873. Opened owned outputs/warm-gradient.tessera-doc through native File > Open Document. Properties selected, Color Lookup 1 selected. Clicked Name, pressed Tab. Native CUA AX reported focused button document.properties.colorLookup.load (Load 3D LUT…). Second Tab hid inspector and sidebar: Inspector toggle off, Show Sidebar visible, canvas/document remained. This reproduces the original failure with tracing disabled. Ordinary CUA super+q, anchored process check exit 1; no CUA after this quit before explicit next launch.

## Enabled sequence and failure

Exact launch command in disabled-to-enabled.json: same app/profile, plus --inspector-focus-trace /tmp/tessera-inspector-focus-trace-c5f8bca7/evidence/focus-01.jsonl (absent before launch). PID 12227. Opened same copied document, Properties already selected. Clicked Name, pressed Tab. Native CUA AX again reported focused Load button. On second Tab, subsequent CUA observation displayed fresh Library. Anchored shell check found replacement PID 12310 with the same executable and NO arguments, while PID 12227 was gone. The CUA observation apparently relaunched the app after its crash; this was not an intentional third launch.

Crash report matches PID 12227, package executable, 2026-09-28 19:46:26 -0400. Main-thread EXC_BREAKPOINT/SIGTRAP: _ArrayBuffer._typeCheckSlowPath → Collection.prefix → AppKitFocusNode.children(limit:) InspectorFocusTrace.swift:285 → snapshot:229 → routeEvent capture:174. Line 285 iterates accessibilityChildrenInNavigationOrder() with ordered.prefix(limit). This is an observer bridge failure, not evidence of the router's second-event decision. Concrete second-event semantic/window/native snapshot and handler result were never persisted and remain UNKNOWN.

Raw trace contains exactly one complete event: first Tab, keyCode48, document=true, ownedKeyWindow=true, blockedWindow=false, fullKeyboardAccess=true. Pre-event native type SwiftUI._SystemTextFieldFieldEditor; primary AXTextField document.properties.name focused=true, window membership matched, incomplete=false. Handler returned false. This corroborates only the first Tab.

## Cleanup and scope

Ordinarily quit owned replacement PID12310 via CUA super+q. Final anchored process check exit1 confirms no owned executable remains. No subsequent CUA calls or additional launches. Runtime lane released to root. No global preferences, focus forcing, Load Space/Return, routing fixes, or expanded GUI matrix. Since the unintended replacement lacked --app-dir, default-profile startup effects are UNKNOWN: no before-baseline exists, no claim of unchanged user/default state, and no cleanup of unknown startup files was attempted.

Disabled reproduction passed; enabled diagnostic FAILED and is INCONCLUSIVE for keyboard routing/noninterference. Automated 42-test/strict results remain separate evidence; they did not predict the actual native bridge crash. Original full CUA observations are retained in the conversation; this report records their exact relevant IDs/sequence, not a fabricated full AX export. Raw JSONL and crash are copied verbatim beside this report.
