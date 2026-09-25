// swift-tools-version: 6.0
// Tessera macOS app shell (WP M0-04).
//
// Build:   xcodebuild -scheme Tessera -configuration Debug -destination 'platform=macOS' build
//    or:   swift build
// Bundle:  ./Support/make-app.sh   (wraps the executable into Tessera.app)
import PackageDescription

let package = Package(
    name: "Tessera",
    platforms: [.macOS(.v15)],
    products: [
        .executable(name: "Tessera", targets: ["Tessera"]),
    ],
    targets: [
        // UI-free model: items, cull decisions, grouping, stub library, thumbnail loading.
        // Kept separate so it can be unit tested and later swapped for the Rust engine (UniFFI).
        .target(
            name: "TesseraCore",
            path: "Sources/TesseraCore"
        ),
        // AppKit + SwiftUI shell.
        .executableTarget(
            name: "Tessera",
            dependencies: ["TesseraCore"],
            path: "Sources/Tessera"
        ),
        .testTarget(
            name: "TesseraCoreTests",
            dependencies: ["TesseraCore"],
            path: "Tests/TesseraCoreTests"
        ),
    ],
    swiftLanguageModes: [.v6]
)
