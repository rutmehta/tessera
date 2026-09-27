# Tessera UX research and product direction

Research date: 2026-09-27  
Repository baseline: `46e9928e69f6efd7fce8eb471a03d83c199c086f`  
Scope: macOS library, culling, raw development, layered editing, and AI-assisted workflow. Documentation and public-source research only. No source changes, app launch, user interviews, or hands-on competitor benchmark were performed.

## Executive recommendation

Build **one asset-centered workspace with explicit Library and Edit contexts**, not seven Lightroom modules and not a magically rearranging inspector. Inside Edit, Develop, Masks, and Layers are stable, adjacent working surfaces over the same document. Switching surfaces must not create a file, bake a render, reset a zoom, or lose a selection.

Tessera's opportunity is not simply “Lightroom plus Photoshop.” Adobe already shares a raw engine and supports raw Smart Objects. Affinity Photo 2 already demonstrates revisitable raw development inside a layered document. The opportunity is to combine that continuity with a first-class shoot library, fast group-based culling, safe batch operations, and an inspectable AI review workflow.

The largest current product gap is at that seam: the supplied acceptance flow develops a raw into a **single 16-bit sRGB pixel layer** for Edit in Layers. That is not the desired live raw document, even though the engine has smart objects and an RGB Camera Raw filter. Prioritize document continuity and trustworthy scope before adding visual polish or more filter categories.

Keep the existing strengths: folder-first entry, keyboard culling, explicit keep/reject decisions, separate suggestions, safe deletion, real history, subdued chrome, and one action per gesture. Change the places where users can accidentally act on the wrong scope, confuse a suggestion with an accepted result, or believe an operation is nondestructive when it is not.

## Evidence and interpretation

Evidence labels used throughout:

- **Documented:** stated in the repository or an official product/help page. This establishes a described behavior, not a successful execution in this audit.
- **Inference:** my UX assessment of the likely cost or failure mode. It is not a measured prevalence claim.
- **Proposal:** a recommended Tessera behavior, not a claim that it exists.
- **Knowledge-based:** established product knowledge where version-specific live verification was incomplete.

All competitor weaknesses below are heuristic inferences unless an explicit documented constraint is named. Vendor claims such as “100% precision,” “zero delay,” or percentage time savings are not accepted as research results. No invented user quotes or performance comparisons are used.

### Repository reading and baseline reconciliation

Read in full:

- [R1] `docs/01-lightroom-classic-spec.md`, especially Library §1, masks §2.15, history/sync §2.17, export and proofing. This is a parity specification, not an implementation inventory.
- [R2] `docs/02-photoshop-spec.md`, especially document model §1, retouch §5, type §9, Camera Raw §13, workspace §16. Also a parity specification.
- [R3] `docs/10-agentic-editing.md`, complete. Base edits use ordinary engine tools, with consistency constraints, group amount, review, rationale, and optional local/cloud planning.
- [R4] `docs/STATUS.md`, complete. Its heading says September 25, but later entries include September 26. Some “Known gaps” repeat work described as merged elsewhere in the same file.
- [R5] `apps/mac/DESIGN.md`, complete. Existing tokens, controls, motion, and later document-mode additions.
- [R6] `apps/mac/ACCEPTANCE.md`, complete, including §§V–AD. Acceptance instructions describe expected behavior; they explicitly are not visual-QA evidence by themselves.
- [R7] `docs/06-culling-and-selection.md`, complete. Tessera deliberately uses decisions, grades 1–3, marks, derived status, and a basket instead of blindly cloning Lightroom ratings.

Use the later, specific acceptance sections to qualify the older summary. Do not silently turn specifications into shipped features. Examples:

| Area | Documented current surface | Qualification that matters for UX |
|---|---|---|
| Cull and compare | Group navigation, K keep-best/reject-rest, undo, two-up challenger comparison, defect sweep, basket, safe delete | R7 specifies 2–6-up; R6 §E demonstrates two-up. Wider survey is a target, not verified current UI. |
| Assist and people | Suggested decisions, face strip, people naming/merge/split, person facet | R6 §Q Assist reorders by confidence and Y confirms all suggestions. People undo is separate and session-limited (§U). |
| Develop | RAW and rendered formats, sliders, crop, curves, masks, snapshots/history | R6 §M calls Sky a blue-sky heuristic and People a boxed whole-object proxy, not full semantic sky or body-part parsing. |
| AI editing | Auto Edit scope, providers, consistency toggles, low-confidence review, group fade/toggles | R6 §R uses a scripted planner in its example. This does not establish the quality of a real planner. Cloud providers are available: “all AI stays on device” would be false. |
| Layers | Document tabs, real engine, layers/adjustments, masks, tools, filters, channels, styles, save/export | R6 §V step 144 renders the raw to a pixel layer. R6 §W still labels Type, Crop, and Gradient palette tools placeholders. Engine text support in R4 is not evidence of a complete type UI. |
| Models | Develop first-use acquisition, inline progress, retry | R6 §Z Remove/neural models require manual installation, whereas §AC Develop downloads are integrated. Availability is inconsistent by feature. |
| Output | Export presets, formats, watermarks, proofing, print, EDR preview | R6 §§AA/AC describe unavailable HDR export and possible Lens Blur omission with a warning file. An EDR viewport is not proof of HDR delivery. |
| Effects | Editable layer-style panel and saved effects | R6 §AD describes CPU fallback, slow redraw, and a pixel limit. “One GPU engine” does not mean every operation has a responsive GPU path today. |

## 1. User archetypes and top jobs

These are job-based hypotheses, not demographic personas validated by interviews. A person can move between them within one shoot.

| Archetype | Top jobs, in order | What success feels like | Critical risk |
|---|---|---|---|
| Consumer enthusiast | Bring in a phone/camera folder without learning catalogs; find a small set worth keeping; improve exposure/color and remove distractions; share the right size; find the originals later | “I made these look like I remember, and I can undo anything.” | Layer terminology, unexplained file copies, downloads appearing as failures, too many tiny controls |
| Wedding/event pro | Ingest and back up multiple cards; align camera times; preserve meaningful moments while culling bursts; normalize mixed lighting across keepers; review exceptions; deliver reproducible galleries | “I know which images need me, and the rest are under control.” | Losing the only moment, treating a blink as a verdict, wrong sync scope, waiting for AI before starting work |
| Landscape/fine-art | Organize an archive; compare near-identical compositions at detail scale; merge brackets/panoramas; refine local light and color; maintain versions; proof and print | “The final file matches my intent, and I can revisit the raw years later.” | Preview/export mismatch, color-biased surround, baked raw state, masks drifting after geometry changes |
| Retoucher/compositor | Start from a developed raw or supplied PSD; isolate subjects; retouch on separate targets; combine assets, type, masks, and filters; revise source color; hand off an editable document | “Every layer, mask, and dependency is predictable.” | Painting on the wrong target, hidden rasterization, unsupported PSD semantics, source edits invalidating retouch without warning |

### Prioritize by job, not by feature count

The first product promise should be **shoot to reviewed selects to consistent base edits**, with hero-image finishing in the same app. This is a more coherent adoption wedge than trying to make every Photoshop filter equally prominent. Landscape users exercise color and nondestructive integrity; retouchers exercise document semantics. Both should be release-validation cohorts, not reasons to overload the opening screen.

## 2. Competitive patterns: what to borrow and what to avoid

Sources are indexed in §7. Live documentation supports the behaviors; the failure assessments are design inferences.

| Tool | Strong pattern / documented behavior | Where the journey becomes expensive | Tessera implication |
|---|---|---|---|
| Lightroom Classic | Catalog/library depth, candidate/select comparison, masks with arithmetic and AI batch recomputation, snapshots, selective sync, external Photoshop integration [R1, S1, S3] | Different Library/Develop mental models; active photo versus selection scope; separate documents and save semantics on Photoshop return. These are learning and coordination costs, not claims that the integration is broken. | Preserve catalog-scale organization and precise mask scope, remove app/file handoffs. |
| Lightroom (cloud) | Simpler photo-first editing; Cloud and Local tabs. Local can edit connected-drive files without cloud import [S2] | Storage location becomes a second mental model. Import to Cloud is different from browsing Local. Do not caricature it as cloud-only. | One visible source/storage indicator, with local work usable before optional services. |
| Capture One | Sessions or Catalogs, fast grouped culling, face-oriented inspection, styles and Smart Adjustments [S4, S5] | Workspace flexibility raises setup cost. A reference's intended look and its numerical settings are not equivalent; the original Smart Adjustments release explicitly relied on faces. Do not generalize that old limitation to every current version. | Offer reference-based matching with explicit applicability and scene groups, without asking novices to configure a studio first. |
| Photo Mechanic | Contact sheet, concurrent multi-card ingest and selection, rapid raw previews, IPTC templates, capture-time alignment, side-by-side comparison [S6] | Exceptional intake still needs a development/compositing destination. Fast camera previews are useful but not the same thing as final raw rendering. | Start culling immediately; show preview provenance instead of blocking until full processing. |
| Apple Photos | Library continuity, original preservation, revert, selective copy/paste to multiple items [S7] | Simple global photo state is reassuring but offers less visible production structure for a professional shoot than a dedicated culling/review system. | Copy its confidence and simple recovery, not its absence of explicit production stages. |
| Photomator | Approachable raw controls, AI subject/sky/background selections, brush/gradients, reusable batch workflows, browser copy/paste [S8] | A comfortable photo editor is not a complete layered compositor. A separate finishing app can reintroduce artifact and ownership questions. | Progressive disclosure can coexist with real batch tools; do not reserve batch editing for an “expert” onboarding choice. |
| Darkroom | Tight library workflow, flag/reject, selective copy/paste, batch organization and export [S9] | Same-family shortcuts differ from Lightroom conventions. Minimal controls can leave target scope implicit unless selection feedback is strong. | Borrow fast set-based actions, keep scope and shortcut labels explicit. |
| Pixelmator Pro | Compact native layered-editor pattern, contextual editing rather than a catalog-first surface; layer-based photo/design workflows visible on Apple's product page [S10] | Document editing alone does not answer shoot-wide culling, consistency, and exception review. Detailed raw-state behavior was not verified from the image-heavy extracted page. | Use native restraint and contextual properties, but give the library equal product status. |
| Affinity Photo 2 | Develop Persona plus embedded/linked raw-layer workflows and live-filter/layer editing (Photo 2 behavior knowledge-based; see S11) | Personas make tool domains legible but create context switches. It is not a Lightroom-class shoot library. | A serious precedent for live raw inside layers, not a capability Tessera can claim is unprecedented. |
| Photoshop | Smart Objects, adjustment layers, smart filters, selections, retouch, type, contextual properties, deep file handoff [R2, S3, S12] | Nondestructive behavior depends on target/type and the chosen command. Pixel adjustment versus adjustment layer, mask versus pixels, smart filter versus baked filter are easy to confuse. | Make the target and output form visible before applying; default toward editable results. |
| Luminar Neo | Catalog-to-edit workflow, approachable named tools, layers, sync adjustments [S13] | Official sync is broadly “all adjustments”; the layer guide documents exceptions and watermark-plus-other-edit coupling. A pleasant creative shortcut can become ambiguous in a batch. | Never conceal which settings will propagate, and separate reusable looks from literal values. |
| Aftershoot | Assisted or automated culling, grouping, Survey review, local processing; current vendor pages describe integrated editing/retouching too [S14] | The hard work shifts from making every choice to trusting or correcting machine choices. A top-picks-only review can conceal narrative omissions. | Always keep rejects and unreviewed groups accessible. Do not assume Aftershoot still requires an external editor for every edit. |
| Imagen | Personal style profiles, cloud-assisted processing, editable results returned to Lightroom, feedback from final edits [S15] | Upload/download and catalog reconciliation add coordination; a matching style is not proof every important moment is correct. | Preserve editable recipes and learning, make the run/review boundary first class. |
| Evoto | High-volume retouch/color sync; explicitly distinguishes source photo from targets; selective effects and presets [S16] | Its documented sync settings popup defaults to once per project, so remembered choices can become invisible. Same person does not imply identical lighting or retouch needs. | Keep source, target count, settings summary, and identity-specific retouch exclusions visible every time. |
| Figma-style modern pro UI | Stable canvas, object-context properties, searchable Actions, optional property labels, compact tool surfaces [S17] | Floating chrome can cover valuable image area; icon simplification reduces discoverability. Figma's own UI3 refinement post discusses icon clarity and overflow duplication. | Borrow command discoverability and context, not a bottom floating toolbar or border-radius fashion. |

### Cross-tool synthesis

1. Fast selection is mainly about seeing the relevant evidence with little latency, not inventing a new rating scheme.
2. Batch editing has three different intents: copy values, apply a relative change, or match an appearance. A single “Sync” command cannot make those distinctions invisible safely.
3. AI changes the primary unit of work from “edit every image” to “review uncertain decisions.” Review deserves a real workspace, not a list of confidence percentages in a transient sheet.
4. Nondestructive is a relationship among source, parameters, masks, pixels, and save format. It is not a marketing badge.
5. The most modern interface is not necessarily the emptiest. Stable labels, predictable targets, and reliable undo matter more than glass and floating controls.

## 3. The five core journeys

All “ideal” flows here are proposals. Each is written as an executable product scenario rather than a feature inventory.

### A. Import → cull → choose photos fast

**Typical incumbent flow:** ingest/import → wait for enough previews → set flags/stars → compare bursts → inspect faces at 100% → collect selects → hand off to editing. Photo Mechanic minimizes the intake wait; Capture One brings grouped review into import/cull; Lightroom brings catalog structure; AI cullers offer a first pass [S4–S6, S14].

**Pain points:** catalog/destination decisions before useful work; switching between fit and face detail; losing one's place when sorting or rejecting; confusing duplicate groups with editorial stacks; over-trusting “best” when emotion or coverage matters. These are workflow risks, not measured complaints from this audit.

**Ideal Tessera flow:**

1. **Enter through Open Folder or Import from Card.** Open Folder references files in place. Card import defaults to Copy, with destination, optional second-copy backup, naming preview, and duplicate policy on one clear sheet. No hidden Move default. Existing folder-first work never requires creating a catalog first.
2. **Begin browsing during ingest.** The grid populates from embedded previews. Show separate copy, verification, preview, and analysis progress. “Safe to eject” depends on file-copy verification, not on whether AI finished. An unreadable card or failed backup stays visible with retry and affected filenames.
3. **Establish the review sequence.** Capture order is the default; offer camera-time alignment and burst/similarity grouping. Label an AI group “Similar frames” and let users split/merge it. Manual stacks remain organizational objects, not a destructive consequence of grouping. A burst can legitimately have several keepers.
4. **Choose a frame without touching the mouse.** P = Keep, X = Reject, U = Undecided; 1–3 optionally grade keepers. B adds to the visible basket. Preserve the existing model [R7]. Auto-advance is visible and reversible; undo restores the previous focus as well as the decision.
5. **Inspect the evidence.** Return/E opens loupe. Z toggles Fit/100% in the culling context. Show camera preview versus developed preview and actual detail availability; do not call an upscaled embedded preview “full raw detail.” Face-strip clicks jump to corresponding face detail without losing the sequence. “Not analyzed” must differ from “no faces.”
6. **Compare the burst.** C opens two-up candidate/challenger; an explicit Survey option expands to 3–6 frames when useful. Link zoom/pan by default with a visible unlink toggle. Pin the reference; selecting a candidate must not silently replace it. Allow aligned per-person face comparisons, but fall back to ordinary image coordinates if identities are uncertain.
7. **Apply group decisions deliberately.** Preserve K as “Keep suggested best and reject other undecided frames,” with a persistent command hint and one undo group. Show the proposed winner before pressing K, and protect existing manual keeps by default. Provide “Keep this, leave others undecided” beside “Choose winner.” Never turn a similarity group into a one-winner rule by assumption.
8. **Turn Assist on without losing position.** Suggestions appear in place. “Sort by suggestion” is a separate explicit action, not an automatic resort. Use reasons such as “eyes possibly closed” and “soft face”; never imply that a technical score determines emotional value. Permit “intentional blur” and “important moment” overrides.
9. **Review suggestions at the right scope.** Y confirms the focused suggestion in the proposed keymap; “Confirm selected suggestions (N)” and “Confirm all pending (N)” are separate named actions. This intentionally changes current Y-all behavior [R6 §Q] and requires migration guidance, not a silent remap. Pending/confirmed counts never share the same filled decision treatment.
10. **Check coverage, then hand off.** Show unreviewed groups, kept frames per scene, and optional named-person coverage. People filters support “contains any” and “contains all,” visibly distinguished; current multi-person facet is any-match [R6 §U]. Preserve the ability to review rejects. “Edit keepers” opens the exact frozen selection, with count and exclusions.

**Recovery:** a disconnected volume leaves previews and decisions usable; copying can resume without duplicate decisions; reject never deletes. Remove from Album and Move to Trash remain distinct, preserving current safe-delete behavior.

**Validation scenario:** cull a mixed-camera event with bursts, intentional motion blur, no-face scenes, and a missed-focus but unique moment. Measure time to first decision, wrong-target decisions, lost-place incidents, and unique moments rejected. Run keyboard-only, including undo immediately after auto-advance and after a filter removes the current frame.

### B. Mass/batch editing

**Typical incumbent flow:** edit a representative image → choose targets → copy/paste or sync settings → fix outliers → export. Capture One Smart Adjustments and Imagen-style profiles aim at appearance consistency rather than identical settings; Evoto emphasizes synchronized retouching; Photomator/Darkroom make repeatable batches approachable [S5, S8, S9, S15, S16].

**Pain points:** selected versus active-photo ambiguity; copying crops or local masks unintentionally; a preset overcorrecting a differently lit frame; remembered sync options; insufficient visibility into partial failures. With AI, reviewing many independent “good-looking” frames can miss a visibly inconsistent gallery.

**Ideal Tessera flow:**

1. **Select a bounded set.** From Keepers or an album, show “240 selected · Ceremony · excludes 18 rejects.” Target membership is frozen when the job starts; incoming tether frames and subsequent filter changes do not quietly join it.
2. **Choose the operation, not just “Sync.”** Offer Copy settings (absolute), Adjust selection (relative, e.g. +0.3 EV), Apply preset (selected recipe fields), and Match reference / Auto Edit (per-image values to achieve a look). Each has a one-sentence explanation.
3. **Pick and pin the source.** Show its thumbnail, filename, and “Reference” text. A border color alone is not enough. For AI, choose a style profile or several reference edits with a visible cold-start status.
4. **Set propagation boundaries.** Tone/color default on; crop, geometry, retouch, person-specific masks, and generative changes default off. AI subject/sky masks recompute on each target; they do not copy pixel coordinates. Missing subjects create review exceptions, not invisible empty masks. Preview the exact included settings even if the dialog was previously used.
5. **Constrain consistency.** Group by scene/light, not simply by timestamp or person. Match tone/color within a scene; preserve intentional exposure variation and different scene ambience. Keep person identity available for consistent retouch preferences, not a rigid universal skin-color target. Let the user split a mixed-light group and pin its reference.
6. **Preview a representative sample.** Show reference plus bright/dark, different-camera, and low-confidence examples. Display the changes as editable values with before/after. Allow “Do not change crop” or “Preserve warm reception light” as visible constraints. A sample is illustrative, not approval of every frame.
7. **Run in the background.** Each job has provider, scope, style version, allowed operations, phase, cancel/pause where supported, and an honest ETA only when estimable. Foreground navigation/rendering outranks background AI/export. If manual edits happen during a run, mark that asset conflicted and do not overwrite newer edits.
8. **Review visually, not in a modal checklist.** Open a durable Review view: gallery consistency/contact sheet, reference comparison, and a needs-attention queue. Sort by review priority with reason labels: failed mask, large crop change, mixed lighting, missing model, or low-confidence planning. A numeric score is not a calibrated probability unless validated.
9. **Accept, tune, or redo safely.** A proposed base edit is visible in preview but labeled Pending. Accept changes its review state; Reject/Revert removes only that run's contribution and preserves later manual work. Named history groups retain amount and per-step toggles. Redo starts from a specified baseline and cannot silently stack repeated exposure corrections.
10. **Finish with an accountable summary.** Accepted, pending, reverted, failed, skipped, and conflicted counts are distinct. Export offers “Accepted only” by default for an AI review queue, with explicit override for pending edits. One batch undo restores affected settings; a durable run record allows recovery after relaunch.

**Current delta:** R6 §R already has meaningful scope, guardrails, explanations, group amount, and review actions. Keep those. Replace sheet-first review and “Edited” as the only visible result marker with durable pending/reviewed state. Do not confuse the scripted acceptance planner with trained style prediction.

**Validation scenario:** a multi-camera wedding across daylight, tungsten, and dance-floor LEDs. Measure reviewer correction time and within-scene consistency, not just images processed per minute. Include one manually edited image while the job is running, cancel mid-batch, relaunch, then revert a run after adding a manual local mask.

### C. Single-photo Lightroom-style editing with masks

**Typical incumbent flow:** choose image → profile/WB/tone → crop and geometry → color/detail → local masks → compare → snapshot/proof/export. Lightroom's mask definitions, arithmetic, and recomputation are a strong reference; Photomator is useful for approachable entry [S1, S8].

**Pain points:** long accordion stacks; global and local sliders looking alike; overlay hiding the real color result; uncertainty about which component a brush modifies; geometry changes making masks feel detached; “before” meaning original in one place and previous step elsewhere.

**Ideal Tessera flow:**

1. **Open Edit in place.** Preserve library selection, collection, scroll, filter, filmstrip, zoom, and neighboring images. A compact breadcrumb identifies source and version. Show RAW versus RGB, profile, and preview readiness without a metadata wall.
2. **Start with the essentials.** A stable Develop tab contains Light, Color, Detail, and Geometry groups, with expanded Light/Color defaults in the approachable layout. Advanced curves, calibration, proofing, and lens tools remain searchable and pinnable, not randomly moved by AI.
3. **Make direct edits.** Dragging is immediate, values are editable, Option is fine adjustment, double-click resets, and each completed gesture is one history step. If refinement is pending, retain a useful preview with a small “Refining” indicator rather than flashing blank.
4. **Enter Masks explicitly.** “Create mask” offers Subject, Sky, People, Object, Brush, Linear, Radial, Color range, Luminance range, and Depth where supported. Unavailable capabilities explain why. Current heuristic Sky and boxed People must be labeled honestly; do not present face-skin/eyes/teeth choices until actual part parsing is available.
5. **Create an intelligible mask.** Name it by intent or content (“Subject · coat” / “Sky darken”), show its thumbnail and components. Add/Subtract/Intersect sits next to the selected component. A brush action explicitly says “New mask” or “Add to Sky”; it must not unexpectedly create Mask 2 because the previous mask lacked a brush [current behavior in R6 §M].
6. **Refine on the canvas.** Brush/erase, gradient handles, range sampling, feather, and overlay controls stay close to the image without covering the region being edited. Allow user-chosen overlay colors, mask-only grayscale, and temporary overlay suppression. AI-generated masks are editable starting points.
7. **Adjust the intended target.** Inspector header says “Mask: Sky · local adjustments” versus “Photo · global adjustments.” Switching masks preserves panel position; the active mask thumbnail receives a strong target ring. Before painting, the cursor and inspector agree on target.
8. **Compare with an explicit baseline.** Offer Hold to compare original, split/side-by-side Before/After, and a pinned history state or snapshot as Before. Label both panes and synchronize zoom/pan. A separate Reference image is not the same command as Before. Original means the initial raw rendering under a defined profile/process, not sensor data without a render.
9. **Save a version and continue.** Name a snapshot or create an independent version without copying the raw. History distinguishes global edits, masks, AI groups, and geometry. Returning to Library updates the preview for that exact version. Export and soft proof clearly show SDR/HDR and destination profile.

**Recovery and edge cases:** mask jobs cancelled before completion must leave no phantom applied mask; offline models offer manual tools immediately. Coordinate-bound masks remain attached through crop/rotation according to documented source-space semantics. If a model version changes, keep the saved mask result or offer explicit regeneration rather than silently changing an old image.

**Validation scenario:** recover a backlit portrait, refine hair against a bright sky, brush an exclusion, then rotate/crop and compare to a named pre-mask state. Switch between RAW and JPEG, close/reopen, and compare exported pixels visually against the final preview under the same output transform.

### D. Photoshop-style layered editing

**Typical incumbent flow:** open document → identify target → select/mask → create layers → retouch/transform → add type/filter → save editable source → export flat output. Photoshop offers comprehensive control; Pixelmator Pro shows how much can fit into a quieter native shell; Affinity uses explicit task domains [R2, S10–S12].

**Pain points:** wrong-target painting; tiny pixel/mask thumbnails; layer opacity versus fill confusion; destructive and nondestructive commands side by side without consequence labels; tool parameters hidden in unrelated panels; unsupported handoff features discovered only on save.

**Ideal Tessera flow:**

1. **Open or continue the same asset.** A library photo reveals its Layers surface without conversion when the live-raw model is implemented. Standalone PSD/native documents open as documents with a clear save location and import-compatibility report.
2. **Choose a target that stays visible.** Layer rows distinguish Raw, Pixel, Adjustment, Text, Shape, Group, and linked/embedded objects. Pixel, mask, vector mask, and channel targets have separate selectable thumbnails and text labels. Inspector header: “Retouch layer · Pixels,” not just “Properties.”
3. **Select with predictable arithmetic.** Marquee/lasso/quick/object/subject/sky/color-range tools share Replace/Add/Subtract/Intersect. Select and Mask previews support edge inspection at 100%, output-to-mask/new-layer choices, and cancel restoring the exact previous state.
4. **Retouch safely by default.** “New retouch layer” offers sample-current-and-below. Clone/heal/remove show source and target. If an operation cannot work nondestructively on the selected type, offer Create pixel copy or another supported output. Never silently rasterize a raw/text/vector node. Removal output records whether it was patch-based or model-generated; the base-edit “non-generative” promise must not be incorrectly applied to every Remove backend.
5. **Transform without destroying source information.** On-canvas handles and typed values agree. Enter commits one transaction, Escape cancels. A linked-object change shows dependencies; native source retains original resolution. Unavailable warp capabilities stay discoverable with reasons, not active-looking placeholders.
6. **Add editable type and vectors.** Click creates point text; drag creates a paragraph box. Font, size, alignment, fill, and essential spacing are upfront; OpenType/path options are advanced. While typing, single keys never invoke photo decisions or tools. Missing fonts preserve editable text and expose substitution before export. This is a required future flow, not demonstrated by R6's placeholder Type tool.
7. **Apply editable adjustments and filters.** Primary adjustment commands create adjustment layers. Supported filters default to smart-filter form; intentional pixel application is a labeled secondary choice. The filter header names its target and output. Show the full canvas plus a pinned detail crop; preview/commit/cancel semantics are consistent.
8. **Review structure and history.** Layer search/filter must work before being presented as a field. Groups collapse predictably; keyboard reorder and context menus are alternatives to dragging. History describes target and operation, e.g. “Sky mask: feather 2 px.” Styles' live-edit panels explicitly say “Changes apply live · Undo to revert,” unlike preview-and-OK filter sheets.
9. **Save native, export with consequences.** Native retains every supported editable node. PSD/PSB preflight lists unsupported features and offers native save, explicit compatible copy, or cancel; no silent dropping. Export Flat names the conversion, profile, bit depth, size, alpha handling, and generated-content metadata. Preview-only spot channels are never implied to be separation-ready print output.

**Validation scenario:** retouch a portrait above a raw layer, select hair, add an adjustment and smart filter, type a title, move a mask, undo, save/reopen, then request PSD. The tester must identify the active paint target and preserved/lost editability before each potentially destructive operation.

### E. Raw development and layers simultaneously, then back to Library

**Incumbent reality:** Lightroom → Photoshop already supports Smart Objects and Smart Object Layers, then returns a saved layered file to the catalog [S3]. Affinity Photo 2's revisitable raw layers are another precedent. The friction is not “competitors cannot do this”; it is the separate file/version/save ownership and the work required to keep the library, raw parameters, and composite mentally aligned.

**Current Tessera gap:** R6 §V step 144 creates a rendered pixel layer. R4's Camera Raw smart filter is described as a develop chain on RGB layers. Neither alone proves raw-source continuity. Calling the current action “Open as live raw” would be misleading.

**Ideal step-by-step flow:**

1. **Start with a library asset and version.** Develop WB/tone/masks normally. The visible version owns an editable graph, initially just a raw source node and its recipe. Library asset identity and source-file identity are distinct from a document version.
2. **Reveal Layers, not “send” the photo.** The same canvas, zoom, color transform, version, and undo timeline remain. The Layers panel reveals “Original raw · developed” at the bottom. No TIFF/PSD is generated merely to change surfaces.
3. **Add a retouch layer and composite elements.** Add mask, adjustment, text, or another source. Selecting a raw layer exposes Develop controls for that node; selecting an adjustment or pixel layer exposes its properties. Two raw layers can have independent recipes in the same document.
4. **Change raw development while viewing the finished composite.** Keep the composite visible by default, with an optional isolated-source preview. A breadcrumb says “Document › Raw: DSC_0142 › Develop.” The user can lift raw shadows and see how it affects the retouch and overlying text without a modal external raw editor.
5. **Make dependency consequences intelligible.** Separate source-development crop from document-canvas crop. Source-bound masks transform with their source; document-space paint stays in document coordinates. For a raw crop/geometry change that may invalidate sampled retouch, show affected nodes and offer review/recompute where supported. Do not promise old clone/heal pixels magically become correct after arbitrary source changes.
6. **Keep copies and links explicit.** Default a new document version to independent recipe state referencing the immutable source, not a live link to another version's mutable settings. “Link develop settings” is opt-in with affected-instance counts. Duplicate version changes parameters cheaply; embed/package originals is an explicit portability choice.
7. **Switch images safely while jobs run.** Cache previews by asset/version/recipe state. A completed mask or AI job belongs to the captured target, not whichever tab is active on completion. A failed model does not destroy the prior composite. Missing linked sources show their proxy, status, and Relink action; full-quality export waits for the source or asks for an explicit proxy export.
8. **Return to Library without a save round trip.** Back to Library restores the exact album/filter/scroll and selected version. The thumbnail is the current composite, with “Layers” and review state, not an unexplained `-Edit-2` duplicate. Durable autosave commits the native version; failed saving remains a visible state. Standalone documents still expose their file save state clearly.
9. **Batch and deliver without breaking the graph.** Batch Develop applies only to compatible raw-source recipe nodes unless the user chooses a document-wide adjustment. Show which layer/version will be changed. Export renders the same graph seen in Edit. Preserve native source and metadata; JPEG/TIFF/PSD are explicit deliverables, not the transport between views.

**Proposed ownership model:**

- Asset: library identity, file reference, metadata, albums, culling decision.
- Version: independent editing intent and review state; points to a graph and source dependencies.
- Graph: raw/RGB sources, recipes, mask definitions, pixel layers, transforms, adjustments, filters, type, groups.
- Transaction: scoped edits with named target, before/after revision, provenance, and undo information.
- Output: exported artifact tied to a version/revision, not a competing master.

This is a UX contract, not a claim that a new storage schema has already been designed or implemented. Preserve existing sidecars/native documents through a deliberate migration; do not treat the schema sketch as license to replace current persistence wholesale.

**Release test for the promise:** RAW exposure → subject mask → retouch layer → text → raw WB revision → undo WB → redo → switch photo → return → relaunch → export → return to original album. No automatic flattened intermediary, no duplicate library item, no source mutation, no lost editable type/masks, and consistent rendering. Add a second raw layer and a missing-source case before calling it complete.

## 4. Information architecture

### Choose explicit contexts with stable adaptive properties

Reject both extremes:

- **Many disconnected modules:** easy to name but encourages duplicate controls, separate histories, and handoff artifacts.
- **One fully adaptive workspace:** visually spare but forces users to infer why controls and keyboard meanings changed.

Use **Library | Edit** as the top-level context switch. Review is a durable library destination with an edit-capable viewer, not a third document format. Within Library use Grid, Loupe, Compare, and Survey as views. Within Edit keep Develop, Masks, and Layers at fixed inspector locations. These can reveal different panels without changing the document underneath. Advanced users can pin Layers and Develop together.

The current Grid/Loupe/Compare/Layers segmented control mixes viewing arrangements with an editing domain [R5 §10]. Replace that conceptual mismatch rather than adding another equal segment for every feature.

### Where things live

| Surface | Contents | Must not become |
|---|---|---|
| macOS menu bar | Complete named command hierarchy, visible contextual shortcuts, File/Library/Photo/Edit/Layer/Select/Filter/View/Window | A legacy dumping ground for commands absent from search |
| Top toolbar | Library/Edit, source/version breadcrumb, view arrangement, contextual primary action, jobs/review count, panel toggles | A row of every AI feature or unrelated equal-weight buttons |
| Left sidebar | Library sources/volumes, folders, albums/groups/smart albums, People, Review, recent documents | A second editing inspector; avoid making folders and albums look like identical ownership containers |
| Canvas-side tool rail | Stable tool groups in Edit; active tool labeled in options bar | Floating controls covering the face or mask edge under inspection |
| Tool options bar | Parameters for the current tool, active target, commit/cancel when transactional | A horizontally overflowing mystery strip without group labels |
| Right inspector | Develop/Masks/Layers tabs; selection-specific properties; histogram/proof state; advanced sections/pinned controls | A single endless accordion combining library metadata, faces, AI rationale, and all document properties |
| Filmstrip | Neighbors in current scope, decision/review/layers indicators, pinned reference, visible filter and count | An unrelated global library or a second tab bar with ambiguous ordering |
| History drawer | Current document transactions, named snapshots, AI groups, restore preview | The only place users can discover pending review or learn that an edit is AI-authored |
| Activity drawer | Copy/analysis/model/edit/export jobs, failures, cancel/retry, output links | Permanent stacked progress bars that consume the photo viewport |
| Command palette | Search commands, tools, settings, masks/layers by name, recent actions, shortcuts, applicability reasons | A required conversational prompt for ordinary editing |

At narrow widths, collapse the library sidebar first and use a toggleable filmstrip. Do not squeeze controls below readable labels. At wide widths, allow docked Layers plus Develop and a pinned reference. Window/document tabs are for open documents; filmstrip items are for library navigation. Keep that distinction consistent.

### AI: suggestions versus actions

- **Suggestions:** quality signals, similar groups, possible people, potential defects, proposed keywords. Outlined treatment, explanation on demand, no committed status until accepted. No automatic layout or sort change.
- **Actions:** Auto Edit, Subject mask, Denoise, Remove. User initiates a bounded operation with a named target and output. Long work has cancellation and recoverable failure.
- **Review:** a durable state after proposed changes, not a temporary toast. Separate “rendered,” “saved,” “reviewed,” and “exported.”
- **Settings:** model availability, disk usage, downloads, privacy, style-profile training, provider selection. Routine tools should show human capabilities; model names/backends live in Details unless relevant to a failure or a deliberate expert choice.
- **Privacy:** display “On this Mac” only for genuinely local execution. Local planning through Ollama is distinct from cloud planning through Anthropic/OpenAI. Before cloud use, show what leaves the Mac, provider, and explicit consent. First-use model download is network activity even when subsequent inference is local.
- **Guardrails:** crops/identity-changing retouch/generative output are separate opt-ins. Base Edit remains non-generative; removal/colorization must use their own accurate provenance.
- **Learning:** accept, rejection, and manual correction can improve style only under a visible learning preference. “Accepted” should not secretly mean “permission to upload training images.” Provide reset/exclude controls.

### Keyboard model and conflict policy

Preserve familiar local shortcuts while making context explicit. Do not pretend B can mean Basket and Brush simultaneously.

| Scope | Proposed behavior |
|---|---|
| Global | Cmd-Z / Shift-Cmd-Z undo/redo for the named active scope; Cmd-S save/checkpoint; Cmd-Shift-E export; proposed Cmd-K command palette, subject to command-registry collision check |
| Library culling | G grid, E/Return loupe, C compare, P/X/U decisions, 1–3 grades, B basket, K group keep-best, Z fit/detail, arrows per displayed view; Option-arrows retain explicit group navigation |
| Edit photo | D Develop, M Masks, R crop where not conflicting with focused tool; photo decisions available through named menu/modified shortcuts, not by stealing tools' keys |
| Layer canvas | Familiar V/M/L/W/B/E/S/J/G/C/T/I/H/Z tools, Shift to cycle grouped tools, X swap colors, brackets brush size, Space temporary pan; numbers follow tool opacity semantics |
| Text/numeric controls | Native field editing wins. Arrows nudge the focused value or caret, not the photo. Return/Escape follow the visible commit/cancel contract. |
| Compare/review | Visible Reference/Candidate labels; named accept/reject scope. Bare Y accepts focused suggestion, never an invisible whole-library batch. |
| Escape | First cancel transient gesture or close popover; then leave the tool; only then return to parent context. Never unexpectedly exit a document while cancelling a selection. |

Current Cmd-E means Edit in Layers outside document mode and Merge Down inside [R6 §V]. Proposal: remove the cross-domain ambiguity by making the Library action a separately labeled command/shortcut and keeping Cmd-E scoped to Merge Down in Layers. Existing users need an explicit shortcut migration guide and a legacy preset; do not silently switch destructive meanings.

A searchable shortcut reference must expose conflicts and active scope. One command registry should drive menu labels, tooltips, command palette, and remapping. Show “Undo Merge People,” “Undo Batch edit: 240 photos,” or “Undo Brush: Retouch,” not a generic Undo detached from its target.

### Progressive disclosure without separate products

Ship one feature model with **Comfortable** and **Compact** layouts, not Consumer and Pro editions. Comfortable defaults to labeled tools, larger text, essentials, and hints. Compact increases information density and exposes pinned advanced controls. Both can use layers, batch jobs, and keyboard shortcuts.

Use task-based starts: “Choose photos,” “Edit a photo,” “Open layered document.” Remember local workspace choices. Hide advanced controls until relevant, but keep them searchable and reveal their location when invoked. Do not continually rearrange panels based on inferred expertise. Saved workspace presets and Reset Workspace provide a safe escape.

## 5. Visual direction for a modern 2026 pro creative app

### Principles

1. **The image is the color event.** Use neutral photo surrounds. R5's warm graphite is coherent branding, but tint immediately adjacent to photographs is an unnecessary variable for critical color judgment. Keep warmth, if any, in peripheral chrome; offer a neutral evaluation surround independently of app appearance.
2. **Density is hierarchy, not tiny text.** Existing 11 pt labels and 20 pt controls can be useful in compact mode but should not be the only option. Default essential labels to 12–13 pt. Use aligned numeric columns and tabular digits; do not put every setting inside a separate card.
3. **Legibility beats hairline purity.** Text, focus, selection, and disabled states must survive different monitors and increased-contrast settings. Aim for WCAG AA text contrast and 3:1 essential control boundaries/focus indicators where applicable; verify token pairs and composited overlays. An existing contrast table is not proof every state is accessible.
4. **Stable chrome near color-critical content.** Prefer opaque panels next to the photo. Native vibrancy can work in the outer library source list. Avoid photo-dependent tinting, glass toolbars across the image, and saturated gradients around the canvas.
5. **Icons have a grammar.** Use consistent SF Symbol weight/optical size, plus custom symbols only where necessary to distinguish layer/source/mask semantics. Label infrequent or high-impact actions. Do not use a sparkle as the only description of a model-dependent operation.
6. **Motion explains a state change, not speed.** Preserve R5's no-animation rule for culling, tool toggles, and repeated keyboard navigation. Sliders/brushes track input directly. Modest 120–180 ms panel/feedback transitions may be used off the hot path; Reduce Motion substitutes static changes or short fades. Never tween between photos during quality judgment.
7. **Status should not contaminate the photograph.** Keep badges in a compact reserved rail or on a scrim, with text/shape redundancy. Pending AI is outlined; committed decisions are filled. Allow an image-clean review mode.
8. **Distinctiveness comes from photographic work.** A signature compare layout, precise face inspection, quiet scope captions, named edit groups, and a beautiful contact sheet are stronger identity than purple glows, rounded dashboard cards, “magic” buttons, or conversational empty states.
9. **Light mode is real.** Support daylight culling and high-contrast accessibility, but keep evaluation surround independently adjustable. Expose HDR/EDR and soft-proof state without boosting chrome into HDR highlights.

### Three distinct mockup-ready directions

All values below are **proposed tokens**, in macOS points and sRGB hex. They are not modifications to `Theme.swift` and not measured accessibility certification. Derive hover/pressed/error/focus states and test contrast before implementation. Shared semantic labels always include an icon or text, not color alone.

| Token | A. Precision graphite — recommended | B. Archival paper | C. Instrument panel |
|---|---|---|---|
| Character | Quiet native photography tool; evolution of current design | Editorial contact sheet and fine-art archive; less software-like | Dense technical workstation for retouch/color experts |
| Typography | SF Pro Text; 11 caption, 12 label, 13 control/body, 15 panel title, 18 heading, 22 onboarding | SF Pro Text 12 caption, 13 control, 14 body, 17 heading; New York 24 only for album/collection display titles, never controls | SF Pro Text 11 caption, 12 label/control, 13 body, 16 heading; SF Mono 11 for diagnostic values only |
| Line height | 14 / 16 / 18 / 20 / 24 / 28 respectively | 16 / 18 / 20 / 23 / 30 respectively | 14 / 16 / 18 / 22 for 11/12/13/16 |
| Weight | Regular, medium, semibold | Regular and medium; semibold for active section | Regular and medium; semibold for active target |
| Spacing | 4 base; 8 gaps; 12 panel inset; 16 section; 24 sheet | 4 base; 8 small; 16 inset; 24 section; 32 large margin | 2 micro; 4 base; 8 inset/gap; 12 section; 16 sheet |
| Radii | 4 input/chip, 6 button, 8 popover; photo corners 0 | 3 input, 5 button, 8 popover; photo corners 0 | 2 input/chip, 3 button, 4 popover; photo corners 0 |
| Dark canvas / panel / raised | `#181818` / `#222222` / `#2D2D2D` | `#202020` / `#282725` / `#33312E` | `#121212` / `#1C1C1C` / `#292929` |
| Light canvas / panel / raised | `#DADADA` / `#F2F2F2` / `#FFFFFF` | `#D8D8D8` / `#F4F1EB` / `#FFFDFA` | `#D6D6D6` / `#EBEBEB` / `#F8F8F8` |
| Dark primary / secondary ink | `#EEEEEE` / `#B7B7B7` | `#EEECE7` / `#BDB7AC` | `#F0F0F0` / `#B9B9B9` |
| Light primary / secondary ink | `#202020` / `#575757` | `#25231F` / `#625C52` | `#171717` / `#505050` |
| Accent dark / light | Amber `#E2A04A` / `#915200` | Deep-teal family `#8DC8BF` / `#246B63` | Signal blue `#90B8F0` / `#245E9B` |
| Separator dark / light | `#3A3A3A` / `#CECECE` | `#46413A` / `#D2CCC1` | `#454545` / `#BBBBBB` |
| Controls and rows | 28 comfortable / 24 compact; sliders 36 / 32; layer rows 32 | 30 controls; 32 library rows; 40 sliders | 24 controls; 24 library rows; 28 sliders; 28 layer rows |
| Sidebar / inspector / filmstrip | 220 / 320 / 80; resizable | 240 / 340 / 96; more caption space | 200 / 304 / 64; optional second inspector column |
| Selection treatment | Thin amber ring plus neutral/tinted row fill and named active target | Teal edge and subtle warm-paper row fill; no decorative tile shadows | Squared blue focus bracket plus target label; minimal fill |

Shared evaluation surround presets: neutral dark `#202020`, neutral mid `#777777`, neutral light `#D8D8D8`, white `#FFFFFF`, black `#000000`. These are display surround colors, not claims about physical 18% gray or calibrated illumination. Keep profiles, display calibration, and soft proofing as separate concerns.

Shared semantics for initial mockups: Keep `#80B38C`, Reject `#D8786C`, Warning `#D8B55C` on dark surfaces; use darker tested equivalents on light surfaces. Pending uses an outlined chip and “Pending” text. Primary buttons use dark ink on light accents in dark mode, white ink on dark accents in light mode. Do not assume the same on-accent ink works in both appearances.

**Recommendation: A.** It preserves Tessera's amber identity and token discipline while removing warm tint from the image surround. B is a genuinely different consumer/fine-art direction, but its paper styling must stop at the canvas boundary. C is useful as an expert density preset; making it the only face of the app would weaken the enthusiast entry point.

**Mockup set to compare all three:** the same event contact sheet, six-face comparison, mixed-light batch review, masked landscape, and portrait composite with raw layer + retouch + text. Use identical image content and window dimensions so a preference for photographs is not misread as a preference for chrome. Include a narrow laptop window, light/dark appearance, long filenames, missing models, and increased text size.

## 6. Prioritized UX changes for the current app

Priorities are judgments based on task risk and differentiation, not a completed usability study. P0 protects correctness/trust; P1 improves the core workflow; P2 expands fluency. Effort is relative: S = mainly surface/behavior, M = cross-surface integration, L = engine/persistence dependency. These are not time estimates.

| Priority / change | Evidence and current risk | Specific recommendation | Dependency / size | Acceptance criterion |
|---|---|---|---|---|
| P0.1 Truthful raw-to-layer transition now; live-raw continuity next | R6 §V.144 produces a 16-bit sRGB pixel layer; R4 RGB Camera Raw filter is not a raw node | Immediately label “Create layered copy from rendered photo,” explain baked raw adjustments, and preserve source. Then implement journey E as the core product milestone | Interim S; full graph/recipe ownership L | User knows whether WB/demosaic remain editable before opening Layers; full E release scenario passes before “live raw” naming |
| P0.2 Explicit command scope and keyboard safety | R6 §Q Y confirms all; §V Cmd-E changes meaning to Merge Down; §U undo routes by view | Visible selection/source/target counts, scoped actions, command registry, labeled Undo, tested text-field priority, deliberate shortcut migration | M | No single-key action affects an unseen batch; typing never rates/rejects; every undo names the correct target |
| P0.3 Make degraded output a preflight decision | R6 §AC.529 exports without Lens Blur when its model is missing, with a warning after writing; §§AA/AD have format limits | Preflight missing sources/models and unsupported effects; default block affected final exports, offer explicit “Export without X” with per-file report. Distinguish skipped conflicts from failed renders | Engine readiness/capability reporting M | No final output silently differs because a model/effect was omitted; incompatible PSD path offers native save before work is lost |
| P0.4 One coherent model/privacy surface | R6 §AC auto-fetch versus §Z manual cache installation; §R cloud/local provider choice | Settings model library, capability readiness, disk/download information, retry/cancel, import local model where permitted; inline tool explanations reuse it | Model registry/acquisition integration M | Fresh offline install can use manual tools; a missing model never creates a fake successful edit; cloud use requires explicit scope/data disclosure |
| P0.5 Preserve manual work across AI runs and review | R3 ordinary reversible recipes; R6 review operates on edits already present | Run IDs/revisions, pending state, target capture, conflict handling, run-scoped revert, persistent queue | Recipe/history transaction support L | Revert AI after a manual brush edit preserves the brush; relaunch retains pending/conflict state; cancelled jobs report exact committed scope |
| P1.1 Separate Library views from Edit domains | R5 mixes Layers into Grid/Loupe/Compare; R6 has separate document histories and tabs | Library/Edit shell, fixed Develop/Masks/Layers entry points, Back to Library restoring source/filter/scroll | Navigation/state M | User completes A → C → D → A without losing place; context and save target remain visible |
| P1.2 Non-modal visual batch review | R6 §R Agent Review is sheet/list-first despite available per-image rationale | Review library destination with contact sheet, pinned reference, synchronized before/after, reason-based exceptions and status filters | Existing queue + navigation M | Entire pending set can be reviewed keyboard-only; accepting an image does not unexpectedly jump to another sort position |
| P1.3 Make deterministic batch operations first-class | R1 §2.17 specifies sync; R6 extensively documents Auto Edit but does not establish full selective-sync UI | Copy absolute / relative delta / preset / appearance-match with source, fields, preview sample, and frozen target set | Batch recipe operations/capabilities M–L | A +0.3 EV batch preserves each photo's crop/masks; AI mask transfer recomputes or flags failure; partial cancel is recoverable |
| P1.4 Stop Assist changing the review order unexpectedly | R6 §Q enabling Assist confidence-sorts immediately | In-place suggestions; explicit sort; stable selection anchor; optional review queue | S–M | Turning Assist on/off preserves current frame and neighbors unless the user chooses sorting |
| P1.5 Make mask targeting and limitations unmistakable | R6 §M brush can create a new mask implicitly; Sky/People are proxies | New/Add/Subtract/Intersect control; named target header; truthful mask capability labels; overlay visibility shortcuts | S–M; real part segmentation separate L | Tester predicts which mask changes before every stroke; unsupported People parts never appear available |
| P1.6 Nondestructive defaults and active paint target | R6 §V Image Adjustments changes pixels; smart filters need conversion; §Y channel painting is unavailable | Primary editable outputs, target labels, explicit rasterize/copy choices, real layer filtering, disabled channel painting with reason | Document tool capability map M | Painting/filtering a raw/text/mask target never silently modifies a different node or rasterizes it |
| P1.7 Complete the layered essentials before adding breadth | R6 §W lists Type/Crop/Gradient placeholders although engine features exist in R4 | Wire and validate editable type, document crop, gradient creation; remove active-looking placeholders until functional | Text/vector/document contracts L | Compose and reopen a portrait with editable text; missing fonts and PSD limits are reported; typing stays responsive |
| P1.8 Unify compare/before/reference language | R6 §E two-up culling exists; R1 §2.17 describes editing Before/After/Reference | Distinct Compare photos, Before/After state, Reference look commands; persistent baseline labels and linked zoom | M | User can identify both image and state in each pane; changing baseline never creates an edit |
| P1.9 Foreground-first feedback and capability honesty | R4 has performance claims at specific levels; R6 §AD styles have slow CPU fallback | Honest refining/running state, no blocked culling, foreground priority, no blank frame while waiting; show unsupported/slow path before large style operations | Renderer/jobs capability work L | Record end-to-end input-to-visible-frame latency, not only kernel time; cancellation remains accessible during heavy work |
| P2.1 Expand fast culling inspection | R7 wants 2–6-up; R6 demonstrates two-up, face strip and person facet | Survey with 3–6 frames, pinned reference, face alignment, coverage view, manual group split/merge | M | Review a burst with two valid keepers and a unique soft frame without forcing a single winner |
| P2.2 Neutral evaluation surround and comfortable density | R5 warm canvas, 11 pt working labels, fixed compact control families | Direction A tokens as a proposal for the shared theme; independent surround; comfortable/compact text and row sizes | Theme/layout/accessibility M | Real photo assessment remains unobstructed at larger text size and high contrast, in light/dark and EDR/SDR |
| P2.3 Command palette and pinned controls | Specs span a large professional toolset; current docs do not establish a universal palette | Search every actionable command with target eligibility/reason and shortcut; pin commonly used sections | Shared command registry M | A novice finds “mask sky,” “relative exposure,” and “relink source”; an expert runs them without traversing menus |
| P2.4 Simplify entry and compatibility explanations | Folder-first, catalog import mapping/fidelity report, model errors, and export constraints already exist [R6 §§N/AA/AC] | Task-based opening state; plain-language storage ownership; readable import differences with inspectable original mapping; “Needs attention” rather than technical-code-first errors | S–M | User can state where originals and edits live, what imported differently, and how to get back to the source |

### Suggested sequence

1. Ship truth and scope: P0.1 interim label, P0.2, P0.3, P0.4. These protect existing work without waiting for a new document model.
2. Establish transaction and version ownership for P0.1 full continuity and P0.5. Do not disguise independent engines/documents as one workspace until the save/undo behavior agrees.
3. Build Library/Edit navigation, visual review, selective batch editing, and target clarity. Preserve the current fast culling/develop hot paths.
4. Finish type/crop/gradient and validate the complete hero-photo journey before expanding peripheral Photoshop parity.
5. Apply density/surround improvements and broader survey/command fluency after the interaction contract stabilizes.

### Research and validation plan

Run moderated sessions with each archetype, including existing Lightroom/Capture One and Photoshop/Affinity users plus enthusiasts with no layer vocabulary. This report does not establish a statistically representative sample or performance baseline.

Use five matching tasks from §3 and capture:

- Time to first useful action and time actively spent reviewing, excluding background compute.
- Wrong-target, wrong-scope, and irreversible-action misunderstandings.
- Decision quality: missed unique moments, mistaken people, and AI corrections, with photographer judgment as the reference.
- Ability to explain original/version/document/output ownership after raw-to-layer editing.
- End-to-end navigation/brush/slider latency distributions on declared hardware, screen size, image dimensions, and warm/cold model state.
- Preview/export agreement, resume/relaunch recovery, and keyboard-only/accessibility completion.

R3's “2,000-image wedding under one hour of human time” and “over 70% base edits accepted after three shoots” are product targets, not demonstrated results. Track correction effort and gallery consistency beside acceptance rate so the metric cannot reward users accepting bad results to get through the queue.

## 7. Public source ledger and limitations

Accessed/searched 2026-09-27. “Search excerpt” means official-page text returned by search, not a full page inspection. Search intermittently returned 403; alternative queries and direct official-page extraction recovered several sources. The Affinity Photo 2 page remained unverified: extraction failed and browser fallback failed at the local bridge. No installed competitor apps were exercised. Figma detailed help extraction failed, but official help/blog search excerpts were available.

- **S1 — Adobe, Lightroom Classic Masking tool.** https://helpx.adobe.com/lightroom-classic/desktop/process-and-develop-photos/masking.html — official search excerpt verified mask types and AI mask application via copy/paste, sync, auto-sync, previous. Direct extraction was navigation-heavy/truncated. Used for behavior, not model quality.
- **S2 — Adobe, Edit locally stored photos in Lightroom.** https://helpx.adobe.com/lightroom/desktop/add-import-and-capture-photos/access-photos.html — official search excerpt, page dated March 27, 2026. Explicit Local versus Cloud distinction and local editing without import/sync.
- **S3 — Adobe, Open and edit Lightroom Classic photos in Photoshop.** https://helpx.adobe.com/lightroom-classic/desktop/work-with-external-editors/open-lightroom-photos-external-applications.html — full extraction. Copy/original choices, compatibility/version considerations, saved catalog return, Smart Objects and Smart Object Layers.
- **S4 — Capture One, portrait workflow.** https://www.captureone.com/en/photo-genres/portrait-photography — official search excerpt. Snap to Eye, grouping/culling, Sessions/Catalogs, retouch and reference-style features. Marketing performance promises not independently validated.
- **S5 — Capture One 23 (16.0) release notes.** https://support.captureone.com/hc/en-us/articles/7619202722589-Capture-One-23-16-0-release-notes — official search excerpt. Grouped culling in import, Smart Adjustments intent, original face dependency, layers in styles. Historical evidence, not a complete 2026 limitation list.
- **S6 — Camera Bits, Tour Photo Mechanic.** https://home.camerabits.com/tour-photo-mechanic/ — full extraction. Multi-card ingest, work while copying, raw preview, contact sheet, metadata, capture-time correction, comparison. Page contains legacy references, so no current-version/platform claim is derived from it.
- **S7 — Apple, Editing basics in Photos on Mac.** https://support.apple.com/en-al/guide/photos/pht304c2ace6/mac — official current-guide search excerpt. Copy selected edits to multiple items, original preservation, revert. An older macOS 10.15 result allowed one target at a time; that older limitation was deliberately not used.
- **S8 — Photomator product page.** https://www.pixelmator.com/photomator/ — official search excerpt. Nondestructive raw adjustments, subject/sky/background and geometric selections, batch workflows and copy/paste.
- **S9 — Darkroom Help.** https://darkroom.co/help/manage/batch-actions ; https://darkroom.co/help/manage/flag-reject ; https://darkroom.co/help/app/keyboard-shortcuts — official search excerpts. Selective adjustment transfer, batch actions, culling and keyboard distinctions.
- **S10 — Apple, Pixelmator Pro.** https://www.apple.com/pixelmator-pro/ — redirected from Pixelmator's official site; extracted content was chiefly image descriptions showing layered editing, color controls, retouch, and type. Compact native/contextual-interface assessment is knowledge-based; no detailed 2026 feature parity, pricing, or raw-layer persistence claim relies on this page.
- **S11 — Affinity raw editing references.** Attempted Photo 2 documentation: https://affinity.help/photo2/en-US.lproj/pages/Raw/raw.html (not successfully retrieved). Official current product overview: https://www.affinity.studio/features/raw-photo-editing (search excerpt supports revisitable raw-to-layer workflow, but is not version-specific evidence for Photo 2). Photo 2 embedded/linked raw and Persona discussion is explicitly knowledge-based. Do not substitute the current Affinity product's full feature set for Photo 2.
- **S12 — Adobe, Create embedded Smart Objects.** https://helpx.adobe.com/photoshop/desktop/create-manage-layers/smart-objects/create-embedded-smart-objects.html — official search excerpt. Place/Open as Smart Object, conversion, edit contents, instance updates. Detailed Photoshop tool taxonomy is additionally grounded in repository R2, not independently re-audited tool by tool.
- **S13 — Skylum, Syncing Adjustments and layers guide.** https://support.skylum.com/how-to-use-luminar-neo/catalog/syncing-adjustments ; https://blog.skylum.com/guide-to-layering-watermarking-and-beyond — official search excerpts. Source/target selection, global sync, and documented exception examples. Confirm current tool-specific exceptions in a hands-on test before designing importer parity around them.
- **S14 — Aftershoot current product and culling FAQ.** https://aftershoot.com/ ; https://aftershoot.com/culling-faq/ — vendor search excerpts. Assisted/automated review, Survey, local workflow, no automatic deletion, integrated editing/retouching claims. No accuracy or speed percentage is treated as proven.
- **S15 — Imagen workflow.** https://imagen-ai.com/solution/the-ai-plugin-for-automating-your-lightroom-workflow ; https://imagen-ai.com/post/ai-editing-vs-lightroom-presets — vendor search excerpts. Profiles, editable Lightroom results, cloud processing, feedback. Marketing pages differ on upload description (RAW files versus catalog smart previews); this report does not assume a single payload for all Imagen workflows.
- **S16 — Evoto, Batch Style Matching: Sync & Presets.** https://support.evoto.ai/save-time-with-batch-style-matching-sync-presets-in-evoto/ — direct extraction plus official search excerpt. Source/target highlighting, selected effects, and once-per-project default popup behavior. Evoto's offline behavior was not sufficiently verified and is not asserted here.
- **S17 — Figma UI3 official help and rollout discussion.** https://help.figma.com/hc/en-us/articles/23954856027159-Navigating-UI3 ; https://www.figma.com/blog/making-the-move-to-ui3-a-guide-to-figmas-next-chapter/ — official search excerpts. Actions, properties grouping/labels, toolbar placement, and the vendor's own discussion of clarity refinements. Detailed help extraction failed; no hands-on UI inspection is claimed.

### Final product call

Do not compete on the length of the tool list or the number of AI badges. Compete on the photographer never having to ask: “Which photo am I changing, is this a suggestion, can I still edit the raw, what did the agent do, and will this export match what I see?”

When those answers stay visible from the first card ingest to the final layered hero image, the unified engine becomes a meaningful user benefit rather than an architectural fact.

RESULT: DONE
