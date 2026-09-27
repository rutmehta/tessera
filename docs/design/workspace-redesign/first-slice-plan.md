# First redesign slice: Library and Photo Edit

Prepared 2026-09-27 for root execution in an isolated managed worktree; no product edits performed. Source recovery: `/tmp/tessera-redesign-ux-scope.md`. Provisional current visual direction: Precision Graphite, selected by coordinator; historical design approval is not asserted.

## Concrete interaction design

### Library

Top toolbar: existing folder/source title, **Library | Edit photo**, then Grid/Loupe/Compare as a subordinate view control. Keep source sidebar, filter row, current native grid/loupe/compare and filmstrip. Keep metadata, selection and relevant Assist/People panels in the Library inspector. Develop controls move to Photo Edit. “Edit photo” is disabled for empty/People views and unsupported targets, with a reason; the existing synthetic test path can use an explicit preview-only state without claiming real editing.

The context header immediately above the canvas uses two lines at constrained widths: **Ceremony › DSC_0142.ARW**; **3 selected · decisions apply to 3 photos**. A single item says one photo. Compare says **Active candidate: filename · decisions apply to 1 photo**. This derives from the actual command target function, not merely selectionCount. Explicit Auto Edit remains a separate batch action with its existing scope sheet.

Double-click and E/Return retain Library Loupe inspection. A visible Edit photo button and D enter editing. This makes the existing culling flow predictable and provides discoverable editing. Document access is a separately named “Layered documents” command, not an arrangement option next to Grid. File New/Open and existing document tabs remain available.

### Photo Edit

Top header: **Back to Library** (source name in help), **source › filename**, **Editing 1 photo · RAW/RGB**. The primary workspace affordance reads Edit. No fabricated version name, review acceptance badge or save state. If a real loading/failure state exists, render it using that state and disable controls until the controller target matches the header.

The inspector has fixed **Develop | Masks**. Develop contains existing histogram, Basic, Tone Curve, HSL, Color Grading, Detail, Transform, Effects, Lens Blur, Crop, HDR, Soft Proof, Presets, Snapshots, History in their existing ordering/expansion defaults. Masks contains the existing mask creation/list/arithmetic/local adjustments, with existing named selection shown as **Mask: name** or **No mask selected**. Global controls visibly target **Whole photo**. Reuse controls, binding paths and history grouping; don't alter algorithms or build a second controller.

Filmstrip uses the current source/filter order; selecting a neighbor changes only the active edit photo, never the stored Library return selection. No new cross-source browsing, source edits or batch operation is introduced. Library source/filter/sidebar controls are hidden while editing so navigation doesn't silently redefine the return destination. Opening a different folder intentionally exits Photo Edit and discards the old-folder bookmark.

Back to Library restores the previous Library view and source/filter/facet, original selected stable identities/focus, and saved grid/filmstrip anchor. Editing neighbor B then returning restores the original Library selection A; the last edited photo remains edited and is not silently inserted into the original selection. Explicit G returns and chooses Grid; Back restores prior Grid/Loupe/Compare. Escape first passes to existing transient tool cancellation, then leaves a tool, then returns to Library only when no gesture is active.

### Document boundary

Keep `ViewMode.document`, `DocumentWorkspace`, `DocumentView`, `DocumentInspector`, tabs, undo/save/dirty state and document keyboard code intact. A current document renders a distinct **Layered document: title** context through its existing shell branch. Library/Edit controls must not imply that the current raw became that document by navigation.

Rename only the Library-side handoff label/help to **Create layered copy from rendered photo…**. A lightweight pre-action explanation names the source, says current raw adjustments become pixels in a new layered document and the original/raw recipe stays separate, and offers Create layered copy / Cancel. Invoke existing `documents.editInLayers` after confirmation; do not change B's API or implementation. This is a transformation disclosure, not an extra confirmation for ordinary navigation. Coordinate shared menu/KeyRouter label hunks with B. Retain current Cmd-E routing until the broader shortcut migration is explicitly implemented; no silent destructive remap.

## State design and integration constraints

- Retain existing `ViewMode` as the renderer/arrangement adapter for this slice, avoiding a cross-repo migration while B uses `.document`. Add one non-document context enum (`Library`, `PhotoEdit`) and a derived shell context where `.document` always wins. Do not maintain a second mutable Document mode flag.
- Add an app-owned `LibraryReturnState` captured only on first entry into Photo Edit. It contains library identity/generation, source/filter snapshot (including current person scope and Assist sort option), prior view, compare pair expressed in stable image keys, selected/focused/anchor stable image keys and browser anchors. Re-entering/toggling inspector tabs never overwrites the bookmark.
- Use existing engine image identity, with the existing library stable-key fallback for non-engine items. Do not persist integer array positions across library deltas. Capture the display-order key list once on entry if needed for deterministic nearest-surviving fallback; never copy the 20k-item library on each render or gesture.
- Normal entry does not change the source/filter, so restore should avoid an unnecessary search/reload. Filmstrip editing may change model focus/selection; restore those atomically, suppressing auto-scroll. If source/filter actually changed via a global action, complete current search before applying the bookmark's surviving keys; use the existing generation guards.
- Keep both native grid and loupe alive as now. BrowserController supplies a typed bookmark of leading visible stable key plus logical offset for grid/filmstrip; capture once, restore after layout with normal bounds clamping. Same-layout return should preserve exact offset; resize/deletion preserves key and clamps. Do not call `libraryDidReload()` just to change contexts.
- A dedicated transition path suppresses `viewMode.didSet`'s unconditional selection scroll only during restoration, then issues one observer update and anchor restore. Normal Library keyboard navigation still scrolls to focus. Restore Compare from surviving pair keys; if either is absent return to Library Loupe on a surviving key with a named status.
- Entering Photo Edit sets `.loupe` through the existing path. It must reuse an already-open matching controller. Switching Develop/Masks changes inspector state only; existing controller/session generation rules remain authoritative.
- In Photo Edit, Undo/Redo routes exclusively to the matching Develop session and names that domain. No fallback to cull. In Library, preserve current non-edit behavior; Document and People routes remain first.
- Keep root shell/layout ownership on A; Document/**, TesseraCore/Document/**, document FFI and generated bindings remain B-owned. AppCommands/KeyRouter/ContentView are shared hotspots; root assigns exact hunks before parallel implementation. No theme-wide token/density change in this slice.

## File-level implementation sequence

1. **Navigation and identity model** — add `apps/mac/Sources/TesseraCore/Workspace/PhotoWorkspaceState.swift` for small state/identity/return-resolution values that can be unit tested without the renderer. Integrate app-specific source/filter and existing private selection/view fields in focused sections of `App/AppModel.swift`; prefer a new `App/AppModel+Workspace.swift` only where access can remain narrow (do not make private model state broadly public). Add `WorkspaceNavigationTests.swift` covering transitions and stable-key restoration.
2. **Browser bookmark adapter** — focused `Grid/ThumbnailBrowser.swift` and `LibraryObserver` additions for capture/restore, respecting the grid's existing update suppression. Add `WorkspaceBrowserRestoreTests.swift` using an unshown AppKit window; check offset within 1 pt and deletion/insertion fallback. No grid rewrite.
3. **Inspector composition** — refactor `Inspector/InspectorView.swift` into reusable existing panel groups with `LibraryInspectorView` and `PhotoEditInspectorView` (new files allowed in Inspector). Add a small `PhotoInspectorTab` enum; bind Masks entry to the existing MaskTools state rather than duplicating it. Preserve selection-change invalidation and detail scheduler notifications. No masks engine changes.
4. **Shell/header** — new `Shell/WorkspaceHeader.swift` and focused `Shell/ContentView.swift` edits. Use Theme/Components, compact layout/overflow and existing inspector sizing. Separate workspace from Library arrangement controls, add target labels, keep Document branch unchanged. Hide irrelevant filter/Assist/auto-advance controls in Edit instead of leaving transparent slots. Keep batch/review actions in Library.
5. **Named commands and scope** — focused non-document sections of `App/AppCommands.swift` and `App/KeyRouter.swift`. Add D and named Back/Edit commands; branch Photo Edit routing after text/Document guards and before cull actions. Preserve M/Mask tool behavior and normal control priority. Scope Library batch menu labels to actual target count; keep Y meaning explicit rather than remapping it. Wire truthful rendered-copy disclosure in new non-Document shell file and the existing Library command entry only.
6. **Documentation and validation** — update DESIGN.md with workspace/inspector contracts and provisional visual continuity; add a concise numbered ACCEPTANCE scenario and any new AX identifiers. Add tests to existing KeyFocusTests/DocumentKeyRoutingTests only as focused regression additions, and extend ShellLayoutHarness states with photoEditDevelop/photoEditMasks. Record actual results and ownership checks under the root-assigned UX work package.

## Tests to prepare before implementation

Tests should assert observable contracts, not enum plumbing alone:

- `testEditEntryKeepsLibraryMultiSelectionButTargetsOnePhoto`: three selected IDs, active middle; edit context target one; return IDs unchanged.
- `testBackRestoresLibraryViewSourceFilterAndSelection`: Grid, Loupe and Compare variants; compare active side preserved.
- `testEditNeighborDoesNotOverwriteReturnBookmark`: edit A→B, return original Library selection/focus and anchor.
- `testRestorationUsesStableKeysAfterInsertAndRemoval`: integer positions change; same photo returns; deterministic fallback when removed.
- `testDifferentLibraryDiscardsOldBookmark`: opening new folder cannot restore old source/IDs or a late old callback.
- `testInspectorTabDoesNotReopenDevelopOrChangeTarget`: matching controller identity and settings/history unchanged on repeated tabs.
- `testPhotoEditUndoCannotConsumeCullHistory`: no Develop undo but cull undo exists; Edit Undo stays disabled/no-op with domain label, then Library undo still works.
- `testEditScopeHeaderMatchesActualLibraryTargets`: multi-select, focus-outside-selection, compare-active, empty and loading cases.
- `testTextNumericAndDocumentKeysRetainPriority`: text field X/Y/D, numeric arrows/delete; document B/X/E remain tool actions; Edit bare cull letters don't mutate cull state.
- `testTransientEscapeDoesNotReturnToLibrary`: active crop/mask consumes first Escape; no unexpected document/file transition.
- `testReturnRestoresBrowserAnchorWithoutFocusSnap`: preserve first visible key+offset; hidden grid receives no scroll-to-focus during edit; visible filmstrip can move independently.
- `testRenderedCopyDisclosureCancelDoesNotOpenDocument`: Cancel leaves source/session unchanged; confirm calls existing handoff exactly once with captured target, not a later focused photo.

Some tests require a real RAW fixture; reuse existing DevelopBridgeTests harness instead of inventing another engine setup. Keep state/keyboard/geometry tests on stub/unshown hosts when sufficient. Do not use the legacy tiny RGB fixture to prove camera-dependent detail behavior.

## Verification commands and resource discipline

Root supplies worktree and external targets; do not invent paths or touch main. First focused run after the source compiles:

`swift test --package-path apps/mac -c release -Xswiftc -enable-testing --filter 'Workspace|KeyFocusTests|DocumentKeyRoutingTests|ThemeLintTests|ShellLayoutTests'`

Use the exact existing Swift test harness patterns. If current source requires regenerated bindings from integration, the root serializes the existing `./build-ffi.sh` stage; this slice itself introduces no FFI API and should not regenerate bindings unnecessarily. After focused checks and final source freeze, execute one full Swift release suite if required by the package gate, not repeated Rust suites for Swift-only changes. All heavy work needs root's slot. ShellHarness is prohibited-activation/offscreen; retain assertions that NSApp is inactive. Screenshot evidence should use that background-safe harness and existing size/appearance matrix. No actual presentation timing claim follows from those captures.

Completion means a coherent Library→Edit→Library workflow passes the scope/restoration/keyboard/layout contracts and B-owned document regressions remain green. It does not mean durable Review, batch sync or live raw Layers has shipped.
