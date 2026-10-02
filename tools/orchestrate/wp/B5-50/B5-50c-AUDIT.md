# B5-50c indirect identifier audit

Baseline: `270f0169a36ada716ab2e6930082f81e0c0edf20`.

Read each changed source against `git show origin/main:<file>`, including id/identifier arguments, helper parameters, assigned strings, concatenation, and interpolation prefixes.

Four renamed inputs found (six concrete IDs including two retry IDs):

- `detail-ai-denoise-model` and derived `detail-ai-denoise-model-retry`
- `lensblur-model` and derived `lensblur-model-retry`
- `transform-upright-reset`
- `transform-reset`

Additional renamed identifier inputs beyond these four: **0**.

The three intentional private payload changes preserve their main prefixes: `lrimport-mark-`, `keyword-suggestion-`, `keyword-suggestion-reject-`. Shared toolbar conditionals retain `document.toolbar.open` and `document.toolbar.inspector`. Newly added helper arguments and interpolation prefixes have no main identifier to restore.

Compared source files:

- `apps/mac/Sources/Tessera/Agent/AgentPanels.swift`
- `apps/mac/Sources/Tessera/App/AccessibilityKey.swift`
- `apps/mac/Sources/Tessera/Cull/AssistPanels.swift`
- `apps/mac/Sources/Tessera/Document/DocumentWorkspace.swift`
- `apps/mac/Sources/Tessera/Grid/ThumbnailBrowser.swift`
- `apps/mac/Sources/Tessera/Grid/ThumbnailCell.swift`
- `apps/mac/Sources/Tessera/Import/LightroomImportController.swift`
- `apps/mac/Sources/Tessera/Import/LightroomImportSheet.swift`
- `apps/mac/Sources/Tessera/Inspector/CurveEditorView.swift`
- `apps/mac/Sources/Tessera/Inspector/DevelopPanels.swift`
- `apps/mac/Sources/Tessera/Inspector/InspectorView.swift`
- `apps/mac/Sources/Tessera/Inspector/LensBlurPanel.swift`
- `apps/mac/Sources/Tessera/Inspector/LibraryPanels.swift`
- `apps/mac/Sources/Tessera/Inspector/MasksPanel.swift`
- `apps/mac/Sources/Tessera/Inspector/PhotoEditInspectorView.swift`
- `apps/mac/Sources/Tessera/Inspector/TransformPanel.swift`
- `apps/mac/Sources/Tessera/Inspector/UnderstandingPanels.swift`
- `apps/mac/Sources/Tessera/Library/FilterBar.swift`
- `apps/mac/Sources/Tessera/Library/RuleTextField.swift`
- `apps/mac/Sources/Tessera/Loupe/LoupeOverlay.swift`
- `apps/mac/Sources/Tessera/Loupe/MaskToolbar.swift`
- `apps/mac/Sources/Tessera/Loupe/SoftProof.swift`
- `apps/mac/Sources/Tessera/Shell/ContentView.swift`
- `apps/mac/Sources/Tessera/Sidebar/SidebarView.swift`
- `apps/mac/Sources/TesseraCore/Develop/Masking.swift`
- `apps/mac/Sources/TesseraCore/Document/EngineDocumentBackend.swift`
- `apps/mac/Sources/TesseraFFI/TesseraFFI.swift`
