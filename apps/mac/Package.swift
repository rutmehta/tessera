// swift-tools-version: 6.0
// PhotoEditor macOS app shell (WP M0-04).
//
// Build:   xcodebuild -scheme PhotoEditor -configuration Debug -destination 'platform=macOS' build
//    or:   swift build
// Bundle:  ./Support/make-app.sh   (wraps the executable into PhotoEditor.app)
import PackageDescription

let package = Package(
    name: "PhotoEditor",
    platforms: [.macOS(.v15)],
    products: [
        .executable(name: "PhotoEditor", targets: ["PhotoEditor"]),
    ],
    targets: [
        // UI-free model: items, cull decisions, grouping, stub library, thumbnail loading.
        // Kept separate so it can be unit tested and later swapped for the Rust engine (UniFFI).
        .target(
            name: "PhotoEditorCore",
            path: "Sources/PhotoEditorCore"
        ),
        // AppKit + SwiftUI shell.
        .executableTarget(
            name: "PhotoEditor",
            dependencies: ["PhotoEditorCore"],
            path: "Sources/PhotoEditor"
        ),
        .testTarget(
            name: "PhotoEditorCoreTests",
            dependencies: ["PhotoEditorCore"],
            path: "Tests/PhotoEditorCoreTests"
        ),
    ],
    swiftLanguageModes: [.v6]
)
