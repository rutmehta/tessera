# Tessera — macOS app and engine bridge (WP M1-12, culling UX M1-09, develop UI M1-10)

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
the recipe on the preview worker). The CPU operators are the default renderer; set
`TESSERA_RENDER_BACKEND=gpu` for the Metal operators (slower on the M4, see `Engine::develop_renderer`).
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
orientation and the default recipe hash. Applying edited recipes is a later feature.

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
| `--stub <n>` | Load `n` generated items (for example `20000`) instead of a folder |
| `--stub-library` | Explicitly use the old ImageIO folder scanner and memory-only decisions |
| `--benchmark` | Run the grid scroll benchmark 1.5 s after launch. The result appears in the status bar and on stderr |
| `--keys "x p opt-right …"` | Self-test aid: after the library loads, feed one key every 0.3 s through the culling key map; `cmd-` tokens trigger the matching menu item (e.g. `cmd-z`, `cmd-shift-d`, `cmd-delete`) |
| `--seed-scores` | Hidden test aid: write deterministic synthetic `focus` / `closed_eyes` scores for the defect sweep (item n: focus 0.25 when n % 4 == 1, closed eyes 0.92 when n % 5 == 2) |
| `--front` | Bring the window to the front without activating the app (for screenshots) |
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
| ⇧⌘D | Defect sweep sheet: thresholds, reviewable list with checkboxes, "Reject N Frames" as one undo step |
| ⌫ | In an album: remove from that album only (undoable). Elsewhere: explains, deletes nothing |
| ⌘⌫ | Delete from Disk…: confirmed; file and sidecars go to the Trash, removed from all albums (not undoable) |
| ⌘Z / ⇧⌘Z | The session's single global undo / redo (decisions, batches, basket and album edits) |

The suggested best frame of each 2+ group carries an outlined SUGGESTED pill (a suggestion only; no
AI signal changes a decision without a key press). The basket target is always shown in the status
bar (`Basket → <album> n`), in the sidebar (`B` tag) and on member cells as a blue pill with the
album's name. Cull ▸ Basket Target switches or creates it; the choice is remembered. Derived status
(edited / exported / published, other albums) is an outlined pill bottom-right; unedited shows nothing
on the cell and `Status: Unedited` in the inspector. Albums live in `<folder>/library.json`.

## Layout

```
Package.swift                 targets: TesseraCore (library), Tessera (app), TesseraCoreTests
Sources/TesseraCore/      UI-free and unit-tested
  EngineLibrary.swift         index + CullSession open, group-by-group display order, PhotoLibrary
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

The face strip, survey mode, 3–6-up compare, learning/reordering, and per-person filters are not built.
Compare uses CGImage layers of the embedded preview, not the Metal loupe, so it has no EDR path yet.
Scores come only from `--seed-scores` until ML producers land; with real folders the defect sweep is
empty. Filtered views (Keeps, albums, marks) navigate groups over the visible frames in the app with the
same semantics as the engine; the unfiltered view uses the Rust session. The 20k-item stub keeps
decisions in memory.

Develop: the loupe is fit-to-window only (no 1:1 zoom yet), so progressive refinement stops at the
screen level rather than level 0. JPEGs are not developable. Engine frames are 8-bit display sRGB, so
the loupe shows no EDR headroom for RAWs yet (an RGBA16F scene-linear surface is the planned path).
Temperature/Tint show an estimate of the as-shot white. `pipeline_cpu::camera_to_xyz` normalises the
camera matrix in raw units, which puts every fixture's as-shot white about Duv 0.03 off the Planckian
locus (renders are self-consistent, but no Custom temperature/tint within ±150 reproduces As Shot), so
moving Temperature away from As Shot shifts colour more than expected until the colour science is fixed.
