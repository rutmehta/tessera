# WP M0-04 — macOS app shell (Opus)

Create `apps/mac/` as an Xcode project (use `xcodegen` if installed, otherwise a Swift Package with an executable target plus a generated `.xcodeproj`; must build with `xcodebuild -scheme PhotoEditor -configuration Debug build` or `swift build` — document the exact command in apps/mac/README.md). macOS 15+ deployment target, Swift 6.
Read docs/06 §3–4, docs/08, docs/11 §1.5 first. Build the shell, wired to stub data (a `StubLibrary` that lists the JPEG/RAW files in a chosen folder and shows their embedded thumbnails via ImageIO, so the UI is real even without the engine):
- Window with a left sidebar (SwiftUI: folders, albums), centre content, right inspector (SwiftUI panels, stub Basic sliders as custom `NSControl`-backed views ready for a < 16 ms update path), bottom filmstrip.
- Grid: `NSCollectionView` with cell recycling, hosted in SwiftUI via NSViewRepresentable, virtualized, keyboard navigation, selection state. Must stay at 60 fps with 20k stub items (generate them).
- Loupe: an `NSView` backed by `CAMetalLayer` with `wantsExtendedDynamicRangeContent`, colour space set from the window's screen, redrawn on screen change; for now it presents a CGImage through a Metal texture. Structured so the Rust engine can later hand it an IOSurface.
- Culling keys exactly per docs/06: X reject, U undecided, P keep, 1/2/3 grade, 6–9 marks, B basket, ←/→ ↑/↓ group navigation (groups stubbed by capture-time proximity), auto-advance toggle. Decisions are shown as badges on cells and persist in memory for this WP.
- Folder picker ("Open Folder…", ⌘O) with `NSOpenPanel`; remembers the last folder.
- App name "PhotoEditor" (placeholder), bundle id `dev.local.photoeditor`.
Design: restrained, native, dark-first, inspired by Capture One / Pixelmator Pro. No SF-Symbol soup.
Also write `apps/mac/ACCEPTANCE.md`: numbered steps a computer-use verifier can follow (launch command, open the folder `fixtures/raw` at the repo root, expect a grid, press keys, expect badges) with expected on-screen results.
Must build cleanly. Commit on branch wp/M0-04 in your worktree.
