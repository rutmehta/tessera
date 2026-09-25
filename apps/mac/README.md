# Tessera — macOS app and engine bridge (WP M1-12)

AppKit where performance matters, SwiftUI elsewhere (docs/11 §1.5). Folder opens now use
`EngineLibrary` and the Rust index through UniFFI 0.32. Decisions, grades and named marks are
written through `sidecar` to `.edits/<stem>.json` and XMP, then refreshed in the SQLite index.
Thumbnails and camera previews come from the Rust embedded-JPEG fast path. The developed
viewport still uses the IOSurface/Metal presentation path; no float pixel buffers cross UniFFI.

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
`getRecipe`/`setRecipeJson`, `embeddedPreview`, and `setEventListener`. Calls throw on errors.
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
Embedded previews are camera-rendered, not developed previews, and do not fall back to a full RAW
decode when a camera JPEG is missing. They are limited to the requested maximum dimension.

Rust tests exercise persistence, incremental scanning, filtering/pagination, recipe validation,
unknown-field preservation, JPEG dimensions and callbacks. Swift's bridge test copies the real
five-file `../../fixtures/raw` folder under `build/`, indexes it, persists a rejection, reopens
and checks both the decision and Rust thumbnail. It fails if fixtures are absent and never
modifies the shared fixture originals. Fetch them with the repository fixture tooling first.

## Launch arguments

| Argument | Effect |
|---|---|
| `--folder <path>` | Open this folder, overriding the remembered last folder |
| `--stub <n>` | Load `n` generated items (for example `20000`) instead of a folder |
| `--stub-library` | Explicitly use the old ImageIO folder scanner and memory-only decisions |
| `--benchmark` | Run the grid scroll benchmark 1.5 s after launch. The result appears in the status bar and on stderr |
| `--keys "x p opt-right …"` | Self-test aid: feed keys through the culling key map after launch |
| `--front` | Bring the window to the front without activating the app (for screenshots) |

Without arguments the app reopens the last folder, if it still exists. If there is none, it shows an
empty state with "Open Folder…" and "Load 20,000 Stub Items".

To try the app before `fixtures/raw` has been fetched, generate sample JPEGs with EXIF capture times:
`swift Support/make-sample-folder.swift /tmp/tessera-samples 60`.

## Layout

```
Package.swift                 targets: TesseraCore (library), Tessera (app), TesseraCoreTests
Sources/TesseraCore/      UI-free and unit-tested; the Rust engine replaces this later
  PhotoItem.swift             item value type
  CullState.swift             Decision / grade / mark / basket, CullStore with a global undo stack
  Grouping.swift              stub burst grouping by capture-time gap (2 s)
  StubLibrary.swift           folder scan (parallel header reads), synthetic N-item generator
  ThumbnailLoader.swift       ImageIO embedded-preview loader, NSCache, cancellable requests
Sources/Tessera/
  App/                        App entry + AppDelegate, AppModel (@Observable), KeyRouter, menus, Theme
  Grid/                       NSCollectionView grid + filmstrip, O(visible) layout, recycled cells
  Loupe/                      CAMetalLayer view (EDR, colour space from screen), renderer, IOSurface frames
  Inspector/                  SwiftUI panels + ValueSlider (custom NSControl)
  Sidebar/                    SwiftUI sidebar (library, folders, albums, smart albums)
  Shell/                      ContentView, status bar, loupe overlay, empty state
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
  `wantsExtendedDynamicRangeContent`. Its `colorspace` is the extended, linearised version of the
  window screen's colour space. It is updated and redrawn on screen, profile, backing and
  screen-parameter changes. A CGImage is colour-converted by CoreGraphics into a half-float
  **IOSurface** (`LoupeFrame.rasterize`) and imported with `makeTexture(descriptor:iosurface:plane:)`.
  The engine will use the same entry point, `present(frame:)`, with its own IOSurface, so no pixels
  cross UniFFI. The loupe paints the cached grid thumbnail first, then the embedded preview (2560 px),
  and prefetches the neighbouring images.
- **Sliders.** `ValueSlider` is an `NSControl`. Every drag step calls the model and the loupe
  synchronously, and SwiftUI state is not touched. Exposure is applied as a linear gain in the loupe
  shader, which shows the path end to end.
- **Keys** (docs/06 §2–3). A local event monitor handles them, so they work whichever pane has focus.
  They pass through while text is being edited, while a panel or sheet is open, and when ⌘ or ⌃ is
  held. In the loupe, ←/→ move between groups and ↑/↓ move within a group. In the grid, the arrow keys
  move spatially (⇧ extends the selection), and ⌥+arrows navigate groups the way the loupe does.
- **Style.** Dark-first neutral greys with one amber focus accent. Semantic colours are used only for
  decisions, marks and the basket. Badges are text pills and there are no symbol icons.

## Not in this WP

Basket membership remains session-local. There is no real "best of
group" pick, so a group jump lands on its first frame. Grouping is time-based only. Compare/survey,
the face strip, the defect sweep and zoom/pan in the loupe are not built yet. Only Exposure has a
visible effect. EDR headroom is shown in the loupe, but no HDR content exists yet to use it.
