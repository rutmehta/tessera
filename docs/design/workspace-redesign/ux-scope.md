# Tessera redesign: recovered UX architecture and first-slice scope

Read-only recovery, 2026-09-27. Source inspected on main `353aa9798f91de1402cdc1aee8511bdb7d58eba4`; no build, app launch, or product edit. References below are repository-relative under `/Users/rutmehta/Developer/tessera`.

## Authority and provenance

- Current user direction, relayed by the coordinator: resume UI/UX redesign autonomously with parallel agents. This authorizes progressing the redesign; it is not evidence that any historical mockup was selected.
- New coordinator decision: use **Precision Graphite provisionally**, preserving the amber identity. This is a current conservative selection, not recovered historical approval.
- Existing implemented contract: `apps/mac/DESIGN.md` makes Theme.swift/Components.swift authoritative, specifies graphite/amber, and records the M2-56 containment/yield rules. B5-02 deliberately introduced the fourth Layers segment and separate document workspace. M3-11 deliberately specified sheet-based Auto Edit and review, including one-key confirm-all. Those are existing design/work-package decisions, not accidental implementation leftovers.
- Audit `tools/orchestrate/audits/ux/REPORT.md` labels its ideal journeys and architecture **proposals**, not implemented behavior or usability research. Its Direction A recommendation is not an owner approval. M2-56 brief explicitly said a redesign was pending the owner's choice.
- Recovered `/private/tmp/claude-501/-Users-rutmehta-Developer-tessera/1ef5c604-ef13-4903-b9c9-757556764307/scratchpad/mock/tessera-redesign.html` is design exploration. Its live Develop layer, batch-match exceptions, shared document/history and model capabilities are target-state illustrations; do not promote them to shipped claims. It correctly calls out rendered-copy reality at line 343. Its claim that nothing checks fit predates M2-56 and is now stale.

## Implemented versus proposed

| Concern | Current source evidence | Missing / implication |
|---|---|---|
| Library/Edit navigation | `App/AppModel.swift:8` has grid/loupe/compare/document; `Shell/ContentView.swift:245` puts Grid/Loupe/Compare/Layers in one segment | No separate context, Photo Edit entry, or return bookmark. Audit §4 proposes these. Layers is a document domain, not a photo arrangement. |
| Place preservation | ContentView:44 keeps grid and loupe mounted. AppModel:172 mode changes notify selection with scroll=true; Compare clears when leaving. Grid/ThumbnailBrowser.swift:185 resets scroll on reload | Existing lifetime retention is useful, but it is not a tested restoration contract for selection + source + filter + exact viewport, especially after Review Show changes source. |
| Photo target | Basic panel captures focused item's ID (`Inspector/InspectorView.swift:170,219`), existing controller checks target identity; library `targetIDs` uses selected IDs, focused fallback or active compare side (`AppModel:1095`) | Selection count exists in bottom status (`ContentView:469`), but no persistent top-level distinction between “editing this one photo” and “library actions on N selected.” Multi-selection does not imply batch Develop. |
| Inspector | `InspectorView:14–44` combines histogram/image/selection/Assist/People/Agent/metadata with Basic, Masks and many Develop sections | Stable Develop/Masks entry points and a separate Library inspector are missing. No need to rewrite the underlying controls to reorganize them. |
| Keyboard | KeyRouter:60–63 protects text/controls/sheets; :97–101 routes Document separately and protects People; :147 Y confirms all visible suggestions | Safety foundations exist. No common command registry; remaining non-document routing can apply library actions in a photo editing workflow. Y-all and dual Cmd-E are explicit existing behavior: migration is a product change, not a bug fix. |
| Undo | AppModel:940 labels People undo; :947 routes Document, People, then Develop versus cull by last domain | Generic Undo otherwise hides domain. A distinct Photo Edit context must not fall through to cull undo when there is no photo edit to undo. |
| Review | AgentReviewSheet has thumbnails, rationale, Accept/Redo/Revert and Show. AgentReviewQueue (`TesseraCore/Assist/AgentReview.swift:77`) preserves row positions for setStatus and tracks pending/accepted/reverted/failures | No Library Review destination, large before/after viewer, pinned reference, durable UI run list or conflict state. Queue is held in AgentController memory and populated on completion; no app-side restore path was found. This is not proof that every engine review field lacks persistence. |
| Review navigation | AgentReviewSheet:36 dismisses then showInLoupe. AppModel:1554 may clear person filter/set source All and collapses selection | It loses review context for out-of-scope photos. Move to a non-modal destination only with explicit return state, and distinguish queue lifespan from recipe persistence. |
| Batch scope | AutoEditSheet:73 captures IDs at Start. AgentController:161 builds fixed engine image IDs; releases pending Develop before run and relinks by ID afterwards | Frozen target membership is substantially implemented; do not reimplement or claim it absent. Defaults can become Whole shoot when only one photo is selected (`AgentController:112`), making visible scope important. |
| Batch operations | Provider choice, consistency, guardrails, cancellation, per-image review, group fade/toggles are implemented | No general selective Copy absolute / Relative delta / category preset / reference-match UI found. Full durable run IDs, revision-conflict handling, accepted-only export and run-specific recovery remain separate engine/UI contracts. |
| Assist ordering | AssistController:19 defaults sortByConfidence=true; enabling refreshes and reorders. confirmAll:205 uses every visible suggestion as one undo group | Audit proposes suggestions in place, explicitly chosen sort, focused Y and separately named batch confirmations. Not shipped; don't silently remap Y in a navigation patch. |
| Raw to Layers | DocumentWorkspace:171–180 calls openDocumentFromImage(developed:true); FFI document.rs:631 states rendered 16-bit sRGB, separate session | No live raw graph continuity or common save/undo timeline. Label current transition “Create layered copy from rendered photo”; show baked development/source preserved before action. Never render this as a harmless Layers tab over the same RAW. |
| Document tools and save | Existing B-owned Document workspace has real current document, tabs, save/dirty state, its own viewport and inspector | Machine B owns this implementation. Audit snapshot tool-placeholder statements must be rechecked against B's newer work, not reused as current gaps. |

## Recommended first coherent slice

**Explicit Library ↔ Photo Edit with visible target and reliable return.** This is a recommendation for the coordinator's implementation scope, grounded in audit P0.2/P1.1 and the provisional Precision Graphite selection.

1. Library keeps Grid/Loupe/Compare, sources/filters and metadata/selection inspector. Loupe remains inspection; it does not silently become the edit context.
2. “Edit photo” / D enters the focused photo with a header showing source, filename, RAW/RGB and “Editing 1 photo.” Multi-selection remains a Library selection, never a hidden batch target.
3. Photo Edit uses the existing loupe/render session and filmstrip. Its inspector has fixed Develop and Masks tabs and the existing controls. A selected mask has a named target; global controls say “Whole photo.” No fake Layers tab or unsupported version picker.
4. “Back to Library” restores the saved source/filter/person facet, view, selection/focus and grid/filmstrip anchor. Existing stable IDs drive restoration. Returning does not manufacture a file, flatten pixels or reset settings.
5. Existing standalone Document workspace stays functional and explicitly named. A Library-side action truthfully describes the rendered copy and invokes the existing B-owned API. Its save/dirty/undo state remains distinct. No visual claim of shared raw/document persistence.
6. Top-level scope appears where users act. Library selection commands display affected count and focused filename separately. Edit excludes bulk cull actions and routes Undo to photo history only. Preserve text/sheet priority, Document shortcuts and the existing Y-all shortcut for now, with its full count/scope explicitly named.

Why first: a palette-only pass would leave wrong-target ambiguity; a combined navigation + durable review + live-raw graph change would entangle three persistence/engine contracts and B's active files. This slice is a user-visible workflow improvement that can reuse current engines and existing Theme/components.

## Objective acceptance

- Select three photos in a filtered album, scroll midway, focus the middle one; enter Edit, change exposure, switch Develop↔Masks, return. Exactly the focused photo changes; other selected photos' settings and decisions are unchanged. Header always names the true target. Same source, filter/facet, prior Library view, selection and focus return.
- At unchanged window dimensions, top visible grid asset and pixel offset return within 1 logical point; no selection-scroll snap. After insert/remove/resize, preserve surviving stable anchor and clamp offset. Missing target uses a declared nearest-surviving fallback and visible status, never another photo's editable controls under the old filename.
- A late session/open/render callback after switching photo cannot replace the new target or enable its controls. Existing generation/controller checks stay intact.
- Text and numeric editing consume letters/arrows/delete; no rating/rejection while typing. Photo Edit does not dispatch cull undo or bare culling decisions; Library and Document keyboard behavior remains as explicitly documented. Escape cancels active gesture/tool before returning; menus show the undo domain.
- Photo Edit entry/return does not recreate an unchanged Develop session merely for inspector tabs. No FFI call/render from header body evaluation. Retain native virtualized grid and loupe; no new per-item SwiftUI rebuilding path.
- Document New/Open/Save/Close, current tab, dirty indication, document shortcuts and panels work unchanged; the rendered-copy handoff is labeled and disclosed before execution. No Document/** or document FFI edits by this lane.
- Shell keeps M2-56 960×600, 1280×800, 1440×900 and 1728×1117 containment in both appearances, visible target/back action, no toolbar/control overlap, no new arbitrary colors/fonts. Long filenames truncate with full AX/help labels.
- Tests are focused state-transition, scope and keyboard tests plus affected shell harness/Theme lint, then one final Swift suite on frozen source if the integration gate requires it. No performance PASS from unit tests; no foreground launch without coordinator authorization.

## Follow-on contracts, not hidden first-slice scope

1. Review as a Library destination: reuse current ordering/status model; add visual context, stable reference and return behavior, then separately add durable run storage/conflict recovery before promising relaunch persistence.
2. Deterministic batch operations: explicit source, included fields and frozen targets; relative +0.3 EV must preserve crop/masks; cancellation and history semantics specified before UI.
3. Command registry/shortcut migration: focused Y versus visible/selected/all; remove Cmd-E cross-domain ambiguity only with discoverability/legacy migration policy.
4. Live raw document/version ownership with B: common graph/transaction/save contract before adjacent Layers claims, including source-bound masks and document-space retouch consequences.

Root remains the coordinator for user/design decisions and B interface ownership. This review found no historical approval of the audit's complete target state.
