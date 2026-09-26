# Tessera — macOS app and engine bridge

AppKit where performance matters, SwiftUI elsewhere (docs/11 §1.5). Folder opens now use
`EngineLibrary` and the Rust index through UniFFI 0.32. Decisions, grades and named marks are
written through `sidecar` to `.edits/<stem>.json` and XMP, then refreshed in the SQLite index.
Thumbnails and camera previews come from the Rust embedded-JPEG fast path. RAWs in the loupe are
developed by the engine (`DevelopSession`), which writes into IOSurfaces the Metal loupe presents;
no pixel buffers cross UniFFI.

Requirements: macOS 15+, Xcode 26 / Swift 6.3. `xcodegen` is not installed on this machine, so the
project is a Swift package (Xcode opens `Package.swift` directly; there is no checked-in `.xcodeproj`).

## Build

Run from `apps/mac/`:

```sh
# Required once on a clean checkout and after Rust API/implementation changes.
# Preserves an existing CARGO_TARGET_DIR; otherwise uses ~/.cache/tessera-target/mac-ffi.
./build-ffi.sh

# Primary (CI) build: the brief's command, plus a macOS destination
xcodebuild -scheme Tessera -configuration Debug -destination 'platform=macOS' -derivedDataPath "$HOME/.cache/tessera-derived-data" build

# Equivalent SwiftPM build and unit tests
swift build
swift test

# Runnable app bundle: build/Tessera.app (bundle id dev.tessera.app)
Support/make-app.sh            # debug
Support/make-app.sh release    # use this for performance checks
open build/Tessera.app
```

Keep `-derivedDataPath` outside the repository. Xcode's dependency (`.d`) files can fail when
the checkout path contains a colon. The default DerivedData location
(`~/Library/Developer/Xcode/DerivedData`) and SwiftPM's `.build/` both work.

`build-ffi.sh` builds `libtessera_ffi.a`, generates Swift into `Sources/TesseraFFI/` and a C
header/module map into `Sources/CTesseraFFI/`. SwiftPM's system-library target imports the
header; the Swift target links the exact archive at `build/ffi/libtessera_ffi.a` with libc++
and zlib. Generated bindings are checked in; the archive and build products are not.
The script targets macOS 15 and produces arm64 on Apple Silicon. For an actual universal
archive, install both Rust targets (`rustup target add aarch64-apple-darwin x86_64-apple-darwin`)
and run `./build-ffi.sh --universal`. The ordinary arm64 build is the tested default.

The bridge provides `Engine.open`, `indexFolder`, `listImages`, `setSelection`,
`getRecipe`/`setRecipeJson`, `embeddedPreview`, `setScore`, `setEventListener`, and
`openCullSession(folder:)` / `openCullSessionForQuery(query:)`. Calls throw on errors.

`CullSession` (crates/tessera-ffi/src/session.rs) wraps `crates/cull` and owns a second SQLite
connection (WAL), so the engine's catalog lock is never held across a culling pass. It exposes the
queue (`images`, `groups` with the suggested `best`), the cursor (`current`/`setCurrent`,
`nextGroup`/`prevGroup`/`nextInGroup`/`prevInGroup`, which stop at boundaries), decisions on the
cursor (`decide`, `grade`, `mark`, `toggleBasket`), one-step batches (`decideImages`, `decideEach`,
`gradeImages`, `markImages`, `setBasket`, `keepBestRejectRest`, `removeFromAlbum`), the single global
`undo`/`redo`, `basketTarget`/`setBasketTarget`/`albums`, `derivedStatuses`, and the review-only
`defectSweep(thresholds:)`. Every mutation returns a `CullUpdate` listing each affected image's new
selection and basket membership plus the cursor, so the app never re-reads the whole library.
Auto-advance is off in bridge sessions: the app advances in its display order. Only ids and small
records cross the bridge.
`embeddedPreview` returns `PreviewResponse(bytes: Data?, pending: Bool)`: cache hits include
bytes immediately. RAW cache hits emit no event; cold RAW requests return pending while the engine worker runs.
`EngineLibrary` installs one shared listener per engine. `ThumbnailLoader` subscribes before
requesting, buffers readiness by image ID and size, and retries only on `PreviewReady` (no polling).
Waiting suspends a Swift task rather than occupying the decode queue or main thread. Completion
runs on MainActor; reuse, cancellation, and library resets suppress stale delivery and reset-time
cache writes. Subscriptions are removed on completion/cancellation. A failed background job also
signals completion via `PreviewReady`; the retry surfaces its error and ends the wait.
No full-RAW fallback is performed in Swift.
Catalog changes arrive in place (M2-28): every catalog write appends to the index's change feed
(SQLite triggers, any connection), and the engine announces it as `EngineEvent.libraryChanged`
(its own writes at once, tethered frames on each poll, other writers within 250 ms). `AppModel`
then pulls `CullSession.syncChanges()` off the main actor and `EngineLibrary.apply` renumbers the
dense item ids around the same session: new frames join their burst (only the affected groups are
recomputed), removed ones leave, the undo history, filters and selection stay, and the grid inserts
or removes just those cells. A `reset` delta (log trimmed) falls back to a full reload.
`openDevelopSession(imageId:)` (RAW only; decodes, so call it off-main) returns a `DevelopSession`
(crates/tessera-ffi/src/develop.rs): `planSurface`/`attachSurface` (an RGBA8 IOSurface ring at the
planned level's size), `setSettings(jsonPatch:interactive:)` (RFC 7386 merge patch of the engine's
`DevelopSettings`), `commit(label:)` (one history entry), `getSettingsJson`, `getHistogram`
(RGB + luminance, 256 bins, of the last frame), `undo`/`redo`/`reset`, `snapshot`/`restoreSnapshot`,
`historyState`, `ignoredSettings`, `flush` and `close`, and a `DevelopListener` with `frameReady`
(surface id, level, valid size, render time), `renderFailed` and `saved(recipeHash)`. Each change
cancels the previous job and submits a `ProgressiveRenderJob` at `Priority::Viewport` on the engine's
shared scheduler; tone-only changes rerun only Tone and Output on memoized WhiteBalance tiles.
Commits are saved on a 400 ms debounce through `sidecar` (recipe JSON + XMP with `crs:` values), the
index is refreshed, and the edited preview is stored under the new recipe hash, so
`embeddedPreview` serves edited thumbnails (edited RAWs without a stored preview are rendered from
the recipe on the preview worker). The renderer calibrates CPU and Metal for each opened RAW;
`TESSERA_RENDER_BACKEND=cpu` or `gpu` overrides that selection (Metal unavailability falls back to CPU).
Layered documents (crates/tessera-ffi/src/document.rs, WP B5-01): `newDocument(width:height:depth:profile:)`,
`openDocument(path:)` (`.tessera-doc`, `.psd`/`.psb` with unknown PSD records kept for save-back, flat
JPEG/PNG/TIFF as one pixel layer), `openDocumentFromImage(imageId:developed:)` (the library image rendered at
full resolution through the export path into one 16-bit sRGB pixel layer), `documentSession(id:)` and
`documentIds()` return a `DocumentSession`; opening the same file or image twice returns the same session.
Reads: `info()` (`DocumentInfo`: id `doc#N`, path, title, canvas, depth, profile, dirty, history head,
undo/redo availability, selected layers, selection bounds, source image, epoch, backend), `layers()`
(`LayerNode`s flat in pre-order, siblings top-first, `index` = compositor child index with 0 = bottom,
blend modes as the stable snake_case names of `blendModeNames()` plus `pass_through`, adjustment/fill JSON,
bounds, `revision` for thumbnail caches), `layer(id:)`, `setSelectedLayers(ids:)`. Edits, each one history
node returning `DocumentUpdate` (changed/created layers, head, level-0 dirty rect, epoch, dirty):
`addLayer(kind:name:parent:index:)` (`NewLayer`: pixel, group, adjustment JSON, fill JSON),
`duplicateLayer`, `removeLayer`, `moveLayer`, `setProps` (`LayerPropsRecord`), `renameLayer`, `setVisible`,
`setOpacity`/`setFillOpacity(id:value:interactive:)`, `setBlendMode`, `setGroupMode`, `setClipped`,
`setLocks`, `setAdjustmentJson`/`setFillJson(id:json:interactive:)`, `addMask(id:mask:)` (`MaskInit`:
reveal all, hide all, from selection), `removeMask`, `setMaskEnabled`, `setMaskDensity`, `setMaskLinked`
(session state only), `mergeDown`, `flatten`, `groupLayers(ids:name:)`, `ungroupLayer`,
`setSelectionRect(x:y:width:height:feather:)`, `clearSelection`. `interactive: true` edits are live only
(the viewport and `layers()` show them) until `commit(label:)` records the whole drag as one node; any other
edit, undo or save commits a pending drag first. History: `undo`, `redo`, `historyItems()`
(`DocHistoryItem`), `checkoutHistory(id:)`, `snapshot(name:)`, `snapshots()`, `restoreSnapshot(name:)`,
`setMaxStates`, `historyMemoryBytes()`. Presentation mirrors `DevelopSession`: `setListener` with a
`DocumentListener` (`onFrame(DocFrameInfo)`, `onLayersChanged`, `onHistoryChanged`, `onRenderFailed`; once per
coalesced frame, on the session's render thread), `planSurface` (fit-to-window level extent),
`attachSurface` (a ring of RGBA8 IOSurfaces), `setViewport(level:x:y:width:height:zoom:)` (a region of a
pyramid level, top-left in the surface; frames report it in level and level-0 coordinates),
`setDisplayHeadroom` (stored; frames are SDR until M5-08), `refresh`, `detachSurfaces`. Frames are
display-encoded sRGB with **straight alpha**: the host draws the transparency checkerboard. Rendering is
the compositor's GPU-resident renderer on the engine's single Metal device (shared with develop).
`layerThumbnail`, `maskThumbnail` and `compositeThumbnail(maxPx:)` return RGBA8 IOSurface ids cached per
revision. Output: `save()`, `saveAs(path:)` (`.tessera-doc`, `.psd`, `.psb` with the flattened composite),
`exportFlat(path:format:quality:color:)` (`ExportFormat` PNG/JPEG/TIFF, `ExportColor` document profile or a
built-in space, ICC embedded), `close()`.
Document mode uses them through `EngineDocumentBackend` (TesseraCore/Document, WP B5-03): `EngineDocumentEngine.for(engine)`
opens sessions (one backend object per session, so the same file or image is the same tab) and each call converts
records field by field to the UI's `DocumentBackend` types. The engine's base history node is the History panel's
`Opened` row (id 0). The listener adapter hops to the main queue and coalesces callbacks (newest frame, union of changed
layers, latest head). The workspace uses the open folder's engine, or a standalone engine when no folder is open, and
the stub backend only with `--stub-library` (and in unit tests). Opening documents and Edit in Layers run off the main
thread. `LayerNode.revision` changes only with what thumbnails show (content, mask, a group's children), so property
drags never re-render thumbnails; thumbnails render off the main thread anyway. Selection bounds are pixel-exact.

Layered-editor tools (crates/tessera-ffi/src/document/tools.rs, WP B5-04), on the same `DocumentSession`:
painting `beginStroke(layer:target:tool:brush:color:)` (`StrokeTarget` pixels/mask, `StrokeTool` brush/eraser/clone/
heal, `PaintBrush` size/hardness/opacity/flow/spacing/angle/roundness/blend/pressure toggles/smoothing/symmetry/tip),
`strokePoints(points:)` (the `StrokeSample`s of one display frame → `StrokeFrame` dirty rect, dabs, engine ms),
`endStroke()` (one history node), `cancelStroke()`, `setCloneSource(layer:dx:dy:)`; selections (one node each,
`SelectionOp` replace/add/subtract/intersect) `selectMarquee`, `selectLasso` (free/polygon/magnetic), `magneticPath`,
`selectWand`, `selectQuick`, `selectColorRange`, `selectSubject` / `selectSky` / `selectObject` (the engine's
segmenter), `selectAll`, `selectNone`, `selectInverse`, `modifySelection`, `refineEdge(params:interactive:)` +
`cancelRefineEdge`, `selectionOutline(level:)` (marching ants as polylines), `saveSelection` / `loadSelection` /
`selectionChannels`; transform `beginTransform(layers:)`, `setTransform(matrix:interpolation:)` (live),
`commitTransform`, `cancelTransform`; `fillSelection`, `deleteSelection`, `sampleColor`; free functions `brushTips()`,
`importAbr(path:)`, `brushTipPreview(id:maxPx:)`. The app reaches them through `DocumentToolsBackend`
(TesseraCore/Document/Tools) adopted by `EngineDocumentBackend` and (geometric subset) `StubDocumentBackend`.
`ImageQuery` accepts folder, FTS text, decision, limit (0 = all), and offset. Folder paths are
canonical paths returned by `indexFolder`; filtering includes descendants. RAW capture times
are Unix seconds as strings; JPEG EXIF capture times are local ISO date-times. Recipe JSON is
the engine-api Recipe, not the sidecar synchronization envelope. History must remain append-only,
ids cannot go backwards, unknown fields survive writes, and newer schemas are not writable.

Events report scan start/completion (the current index has no per-file progress hook) and
preview readiness. Callbacks run on the invoking worker thread, outside all engine locks, so
clients must dispatch UI changes to MainActor. Folder scans and preview requests already run
off-main in the app. Selection writes are synchronous before auto-advance, including undo/redo;
failures are shown in the status bar. The SQLite cache lives at
`~/Library/Application Support/Tessera/index.sqlite`, with a bounded JPEG preview cache beside it.
Sufficient embedded JPEGs stay on the camera-rendered fast path. Missing JPEGs or JPEGs smaller
than one eighth along either sensor axis use the CPU pipeline with bilinear demosaic and default
settings, downsampled in linear light. RAW work runs through `jobs` at `Priority::Preview`.
Both paths apply EXIF orientation and store a JPEG pyramid keyed by source bytes, requested size,
orientation and the recipe hash. Edited RAWs without a cached preview can be rendered from the recipe.

Rust tests exercise persistence, incremental scanning, filtering/pagination, recipe validation,
unknown-field preservation, JPEG dimensions and callbacks. Swift's bridge test copies the real
five-file `../../fixtures/raw` folder under `build/`, indexes it, persists a rejection, reopens
and checks both the decision and Rust thumbnail. It fails if fixtures are absent and never
modifies the shared fixture originals. Fetch them with the repository fixture tooling first.
The other bridge tests generate dHash-distinct JPEG bursts and drive `CullController` against a
real session: group navigation and boundaries, keep-best as one undo step that persists, "choose
this", basket targets and albums, safe album removal, seeded defect sweeps, and delete-from-disk
(with an injected trash so the user's Trash is untouched). `crates/tessera-ffi/tests/session.rs`
covers the same surface from Rust.

## Release packaging and updates

`bash Support/make-app.sh release` embeds Sparkle 2 and signs the app (ad-hoc by
default, or with `CODESIGN_IDENTITY`). Run `bash Support/release/make-dmg.sh` for a
versioned DMG, and `bash Support/release/test-release.sh` for packaging regression
tests. See [release operations](Support/release/README.md) for key provisioning,
notarization, appcasts, delta updates, and GitHub Actions. An empty `SUPublicEDKey`
produces a build warning and must be configured before publishing updates.

## Launch arguments

| Argument | Effect |
|---|---|
| `--folder <path>` | Open this folder, overriding the remembered last folder |
| `--app-dir <path>` | Store index and caches here; overrides `TESSERA_APP_DIR`, otherwise uses `~/Library/Application Support/Tessera` |
| `--stub <n>` | Load `n` generated items (for example `20000`) instead of a folder |
| `--stub-library` | Explicitly use the old ImageIO folder scanner and memory-only decisions, and the stub document backend |
| `--benchmark` | Run the grid scroll benchmark 1.5 s after launch. The result appears in the status bar and on stderr |
| `--keys "x p opt-right …"` | Self-test aid: after the library loads, feed one key every 0.3 s through the culling key map; `cmd-` tokens trigger the matching menu item (e.g. `cmd-z`, `cmd-shift-d`, `cmd-delete`) |
| `--seed-faces` | Hidden test aid: write deterministic synthetic faces (two people) for the face strip, People and per-person filters (0-based item n: person A in every frame, eyes closed when n % 6 == 5, out of focus when n % 5 == 2, the frames `make-sample-folder.swift --defects` blurs; person B in odd groups) |
| `--fake-planner` | Hidden test aid: Auto Edit offers and preselects the scripted planner (the engine's `FakePlanner` with a fixed three-step script), so agent runs need no API key or network |
| `--front` | Bring the window to the front without activating the app (for screenshots) |
| `--import-lrcat <catalog>` | Open File ▸ Import Lightroom Catalog… with this `.lrcat` already chosen (acceptance aid) |
| `--new-document` · `--open-document <file>` | Create a layered document (engine: one blank layer; stub: sample layers) / open one after launch |
| `--tools-selftest <dir>` | Self-test aid (WP B5-04): after the library loads, Edit in Layers on `sample.dng`, then through synthesized mouse events on the viewport: a brush stroke (checked, undo / redo), an eraser stroke to transparency, a magic wand click (outline), Select ▸ Subject, a Free Transform commit, PSD save and reopen in `<dir>`; prints `tools-selftest: step …`, `check …` and the stroke timing, then quits (`--tools-selftest-hold <s>`) |
| `--document-selftest <dir>` | Self-test aid: after the library loads, Edit in Layers on `sample.dng` (or the first RAW), add an Exposure layer, drag Opacity 100 → 40 % at display rate, undo, save / reopen `.tessera-doc`, export PNG, save and open a PSD in `<dir>`; prints `document-selftest: step …`, `check …` and the listener's frame timing, then quits (`--document-selftest-hold <s>` pauses per step). `TESSERA_DOC_FRAME_LOG=1` logs every document frame |
| `--filter-selftest <dir>` | Self-test aid (WP B5-05): Edit in Layers on `sample.dng`, Filter ▸ Gaussian Blur… with a 12-step Radius drag (prints the preview latency, value → frame), OK, undo, Image ▸ Adjustments ▸ Levels…, Convert for Smart Filters, Gaussian Blur as a smart filter toggled off and on, save in `<dir>`; prints `filter-selftest: step …` and `check …`, then quits (`--filter-selftest-hold <s>`) |
| `--develop-selftest` | Self-test aid: once a develop session opens, drag Exposure 0 → +1.5 through the slider path (61 steps at display rate, then mouse-up) and print `develop-selftest: … render median … p90 …` to stderr |

`--keys` also accepts `wait` (one idle 0.3 s step), e.g. `--keys "return wait wait cmd-z"`.

Without arguments the app reopens the last folder, if it still exists. If there is none, it shows an
empty state with "Open Folder…" and "Load 20,000 Stub Items".

To try the app before `fixtures/raw` has been fetched, or to exercise group review, generate sample
JPEGs with EXIF capture times: `swift Support/make-sample-folder.swift /tmp/tessera-samples 60`.
Each burst is a distinct seeded scene, so near-duplicate hashing keeps bursts apart (40 images → 16
groups, 12 with 2+ frames); grain varies so the default best-frame score differs within a burst.

## Culling (docs/06 §3–4)

| Key | Action |
|---|---|
| X · U · P | Reject · Undecided · Keep (auto-advance on) |
| 1 · 2 · 3 | Grade (implies Keep) · 6–9 toggle a mark · B toggle the basket target album |
| ← → / ↑ ↓ (loupe), ⌥ + arrows (grid) | Previous/next group (lands on its first frame) · previous/next frame in group |
| K | Keep the group's suggested best, reject the rest: one undo step, a toast with Undo, then the next group |
| C | Compare: the two selected frames, or the focused frame and its group neighbour |
| Compare: ← → · Return · Z · Esc | Pick side · "choose this" (keep it, reject the other; the next undecided frame of the group takes the rejected side) · fit ↔ 1:1 · back |
| ⇧⌘D | Defect sweep sheet: thresholds on real signals (sharpness, face sharpness, eyes-open proxy, highlight clipping), reviewable list with checkboxes, "Reject N Frames" as one undo step |
| Y · N | Assist on: confirm every suggested decision in view (one undo step) · dismiss the suggestion on the focused / selected frames |
| ⌫ | In an album: remove from that album only (undoable). Elsewhere: explains, deletes nothing |
| ⌘⌫ | Delete from Disk…: confirmed; file and sidecars go to the Trash, removed from all albums (not undoable) |
| ⌘Z / ⇧⌘Z | The session's single global undo / redo (decisions, batches, basket and album edits) |

The suggested best frame of each 2+ group carries an outlined SUGGESTED pill (a suggestion only; no
AI signal changes a decision without a key press). The basket target is always shown in the status
bar (`Basket → <album> n`), in the sidebar (`B` tag) and on member cells as a blue pill with the
album's name. Cull ▸ Basket Target switches or creates it; the choice is remembered. Derived status
(edited / exported / published, other albums) is an outlined pill bottom-right; unedited shows nothing
on the cell and `Status: Unedited` in the inspector. Albums live in `<folder>/library.json`.

## Assisted culling and Auto Edit (docs/06 §3, docs/10, WP M3-11)

**Signals.** Opening a folder measures every photo that has no scores yet, in the background
(progress strip "Analyzing", Stop): `Engine.analyzeImage` runs ml-quality on the displayed preview
(≤ 1024 px) and stores `sharpness`, `motion_blur`, `noise`, per-channel exposure / clipping,
`quality` and `highlight_clipping` (worst channel). Cull ▸ Analyze Faces adds YuNet/SFace faces
(weights download once into `<app-dir>/models/cache`, or `TESSERA_MODEL_CACHE`): per-face sharpness,
the eyes-open proxy and descriptors, and the `face_sharpness` / `eyes_open` aggregates. The defect
sweep reads these real signals; `--seed-scores` is gone.

**Assist** (toolbar, Cull menu, inspector ▸ Assist) switches the library's learner
(`<app-dir>/cull-learning/`, per folder) on the session: *automated* (default) pre-fills decisions
outside the thresholds as outlined `Keep?` / `Reject?` pills (Y confirms all in view as one undo
step, N dismisses), *assisted* only predicts and orders. The grid sorts by keep confidence (likely
rejects last) unless Cull ▸ Sort by Keep Confidence is off. Every manual keep / reject teaches the
learner; the inspector shows P(keep) with its additive explanation. Nothing is decided without a key.

**Faces.** In the loupe a face strip sits under the photo: close-ups with a focus dot (green sharp,
yellow soft, red missed) and an eyes glyph; click one to zoom and to filter the shoot by that person
(or only where their eyes read closed). People are session-local descriptor clusters (inspector ▸
People); the status bar shows an active person filter with a clear button.

**Auto Edit** (⇧⌘A, toolbar) runs the agent (`crates/agent`) through `Engine.runAgent`: planner =
style profile only, Anthropic, OpenAI or Ollama (keys from the Keychain via Settings ▸ AI; never
stored in files or logged), scope = selection / current view / whole shoot, batch consistency (same
burst → one tone and white balance, same person → one exposure), guardrails (masks, crop; retouch off).
The run is non-modal (progress strip with Cancel) and closes develop sessions on those photos first.
It ends in **Agent Review** (Develop menu, toolbar "Review n"): least confident first, with Accept
(marks it and teaches the style profile), Redo… (natural-language, scoped to the controls named) and
Revert (the agent group at 0 %, one history step). The inspector's **Agent Edit** panel shows the
provenance ("AI-assisted, non-generative edits"), confidence and each step's rationale for any photo.

**Develop ▸ History** lists each agent group ("Agent base edit", "Agent redo: …") with an **Amount**
slider (0–100 %; the drag previews through `AgentFade`'s merge patch, the release records one step via
`DevelopSession.commitGroupAmount`), per-step checkboxes with rationales, and "Redo with Instruction…".
Later manual edits survive any amount. **Settings ▸ AI** (⌘,) holds the providers, keys, guardrails,
assist thresholds and the style profile (questionnaire, Learn from My Edits). Preferences live in
`<app-dir>/ai-preferences.json`.

## Lightroom Classic import (docs/05 §3, WP M2-13b)

File ▸ Import Lightroom Catalog… (⇧⌘I) opens a sheet: choose a `.lrcat` → summary (counts, what is not
fully supported and why, disk space, Lightroom running / catalog locked) → mapping (library folder,
root folders with **Locate…** for moved drives, the Lightroom → Tessera selection table, colour label →
mark names, keyword hierarchy, "replace edits already made in Tessera") → fidelity preview (Lightroom
preview vs Tessera render with ΔE2000 badges, sortable, "looks different" filter) → **Import**. The
sheet closes while the import runs; a progress strip above the status bar shows the phase and has
**Cancel Import**. When it finishes the sheet returns with the report, `import-report.md` is written
next to `library.json`, and the library folder opens.

The bridge is `Engine.openLrcat(path:)` → `LrcatImport` (`summary`, `defaultOptions`, `plan(options:)`,
`fidelitySample(options:n:thumbPx:)`, `apply(options:listener:)`, `cancel`), plus `inspectLrcat(path:)`
(crates/tessera-ffi/src/lrcat.rs, lrcat_fidelity.rs). Safety: the catalog, its lock/WAL and
`Previews.lrdata` are read from temporary copies; Lightroom's own `<name>.xmp` files are never modified
(Tessera writes `<name>.<ext>.xmp`, seeded from Lightroom's); existing Tessera edits are kept unless the
user opts in. Each photo gets `.edits/<stem>.json` (translated recipe, selection) and XMP (selection,
keywords with hierarchy). Albums, groups, smart albums, keywords and people are merged into
`library.json` (ids offset, clashing album names get "(Lightroom)"), photos are indexed. Virtual
copies, stacks, faces, history and snapshots stay in `<library>/.tessera-import/<catalog>-<hash>/
import-plan.json`; `state.json` there makes a cancelled or interrupted import resumable. Photos that
share an edit-sidecar stem (RAW+JPEG pairs) are skipped and reported, as the sidecar format keys edits
by stem. The fidelity path can use `crates/pipeline-adobe`; accurate comparison still requires
user-supplied Lightroom reference exports and has documented unsupported fields.
`TESSERA_LRCAT_IMPORT_DELAY_MS` (test aid) slows the per-photo loop so Cancel can be exercised.

## Layout

```
Package.swift                 targets: TesseraCore (library), Tessera (app), TesseraCoreTests
Sources/TesseraCore/      UI-free and unit-tested
  EngineLibrary.swift         index + CullSession open, group-by-group display order, in-place updates, PhotoLibrary
  LightroomImport.swift       import mapping tables, fidelity grid model, import-report.md renderer
  CullController.swift        the app's culling model: Rust session (folders) or CullStore (stub)
  DevelopController.swift     one DevelopSession: IOSurface ring, per-frame patch coalescing, history
  PhotoItem.swift             item value type
  CullState.swift             Decision / grade / mark / basket; in-memory CullStore for the stub
  Grouping.swift              capture-time grouping for the synthetic stub only
  StubLibrary.swift           ImageIO folder scan (--stub-library), synthetic N-item generator
  ThumbnailLoader.swift       embedded-preview loader, NSCache, cancellable requests
Sources/Tessera/
  App/                        App entry + AppDelegate, AppModel (@Observable), KeyRouter, menus, Theme
  Grid/                       NSCollectionView grid + filmstrip, O(visible) layout, recycled cells
  Loupe/                      CAMetalLayer view (EDR, colour space per frame), renderer, IOSurface frames
  Compare/                    2-up compare, CGImage layers with one shared zoom/pan viewport
  Cull/                       defect sweep sheet
  Import/                     Lightroom import sheet, controller, non-modal progress strip
  Inspector/                  SwiftUI panels, HistogramView (AppKit) + ValueSlider (custom NSControl)
  Sidebar/                    SwiftUI sidebar (library, folders, albums, smart albums)
  Shell/                      ContentView, status bar, loupe overlay, toast, empty state
Support/                      Info.plist, make-app.sh, make-sample-folder.swift
```

## Design notes

- **Grid.** `NSCollectionView` hosted in SwiftUI through `NSViewRepresentable`. `UniformGridLayout`
  computes item frames arithmetically, so every query costs O(visible items). It subclasses
  `NSCollectionViewFlowLayout` only so the collection view learns the scroll direction, and it never
  runs the flow layout's O(n) `prepare`. Cells are recycled. The image is a bare `CALayer` and is not
  redrawn while scrolling. Badges are drawn in a small overlay that redraws only when cull state
  changes. Thumbnail requests are cancelled when a cell is reused.
- **Model and views.** AppKit views observe `AppModel` through `LibraryObserver` callbacks. SwiftUI
  observes only summary properties (counts, the focused item). A key press on a 20k-item library
  therefore never re-evaluates SwiftUI bodies per item.
- **Loupe.** `MetalLoupeView` is layer-backed by a `CAMetalLayer` using `RGBA16Float` and
  `wantsExtendedDynamicRangeContent`. The layer's colour space follows the frame on screen. The camera
  preview (first paint) is colour-converted by CoreGraphics into a half-float **IOSurface** in the
  extended, linearised screen space (`LoupeFrame.rasterize`). Engine frames are RGBA8 display-encoded
  sRGB surfaces, imported with `makeTexture(descriptor:iosurface:plane:)` as `.rgba8Unorm_srgb`, so
  the shader samples linear sRGB and the layer (`extendedLinearSRGB`) lets Core Animation convert to
  the display. Surfaces stay in sensor orientation; the shader applies the EXIF orientation and the
  frame's valid top-left region (coarse progressive levels fill part of the surface) and takes four
  bilinear taps when minifying. The surface size comes from `planSurface`: the coarsest pyramid level
  that covers the drawn image in device pixels. A ring of three surfaces means the engine never
  writes the surface being sampled. The loupe paints the cached grid thumbnail, then the embedded
  preview (2560 px), then engine frames, and prefetches the neighbouring images.
- **Sliders.** `ValueSlider` is an `NSControl`; drags touch no SwiftUI state. Each value goes to
  `DevelopController.set`, which records a JSON merge patch and unpauses the loupe's `CADisplayLink`;
  the next tick sends one `setSettings` per display frame. Mouse-up sends the final value and commits
  it as one undo step (`Exposure +0.50`). During a drag on a screen level above 4.2 MP the engine
  renders one level coarser and refines on mouse-up. Temperature/Tint switch white balance to
  Custom; double-clicking them returns to As Shot. Texture, Clarity, Dehaze, Vibrance and Saturation
  stay disabled: `pipeline-cpu` has no operators for them yet.
- **Histogram and readout.** `HistogramView` draws the session's histogram of each frame straight
  from the render callback. The status-bar readout (`render: L3 → L2, 7.8 ms`, Debug ▸ Show Render
  Timing, preference `ShowRenderReadout`) is updated at most ten times a second.
- **Undo.** ⌘Z/⇧⌘Z go to the history of the last kind of change: develop edits use the develop
  session (persisted in the recipe, so it survives relaunch), culling uses the cull session. When the
  cull session has nothing to undo, ⌘Z falls through to the develop history.
- **Keys** (docs/06 §2–3). A local event monitor handles them, so they work whichever pane has focus.
  They pass through while text is being edited, while a panel or sheet is open, and when ⌘ or ⌃ is
  held. In the loupe, ←/→ move between groups and ↑/↓ move within a group. In the grid, the arrow keys
  move spatially (⇧ extends the selection), and ⌥+arrows navigate groups the way the loupe does.
- **Style.** Dark-first neutral greys with one amber focus accent. Semantic colours are used only for
  decisions, marks and the basket. Badges are text pills and there are no symbol icons.

## Not in this WP

Survey mode and 3–6-up compare are not built (the face strip, learning / reordering and per-person
filters arrived with M3-11). Compare uses CGImage layers of the embedded preview, not the Metal loupe,
so it has no EDR path yet. Face zoom is a popover crop of the preview, not a loupe zoom. Filtered views (Keeps, albums, marks) navigate groups over the visible frames in the app with the
same semantics as the engine; the unfiltered view uses the Rust session. The 20k-item stub keeps
decisions in memory.

Develop: the loupe is fit-to-window only (no 1:1 zoom yet), so progressive refinement stops at the
screen level rather than level 0. JPEGs are not developable. Engine frames are 8-bit display sRGB, so
the loupe shows no EDR headroom for RAWs yet (an RGBA16F scene-linear surface is the planned path).
Temperature/Tint show an estimate of the as-shot white. `pipeline_cpu::camera_to_xyz` normalises the
camera matrix in raw units, which puts every fixture's as-shot white about Duv 0.03 off the Planckian
locus (renders are self-consistent, but no Custom temperature/tint within ±150 reproduces As Shot), so
moving Temperature away from As Shot shifts colour more than expected until the colour science is fixed.
