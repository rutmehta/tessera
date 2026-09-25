// swift-tools-version: 6.0
// Tessera macOS app shell (WP M0-04).
//
// Build:   xcodebuild -scheme Tessera -configuration Debug -destination 'platform=macOS' build
//    or:   swift build
// Bundle:  ./Support/make-app.sh   (wraps the executable into Tessera.app)
import PackageDescription
import Foundation

let ffiArchive = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
    .appendingPathComponent("build/ffi/libtessera_ffi.a").path

let package = Package(
    name: "Tessera",
    platforms: [.macOS(.v15)],
    products: [
        .executable(name: "Tessera", targets: ["Tessera"]),
    ],
    targets: [
        .systemLibrary(name: "CTesseraFFI", path: "Sources/CTesseraFFI"),
        .target(name: "TesseraFFI", dependencies: ["CTesseraFFI"],
                linkerSettings: [.unsafeFlags([ffiArchive]), .linkedLibrary("c++"), .linkedLibrary("z"),
                                 .linkedFramework("Security"), .linkedFramework("CoreFoundation")]),
        // UI-free model: items, cull decisions, grouping, stub library, thumbnail loading.
        // Kept separate so it can be unit tested and later swapped for the Rust engine (UniFFI).
        .target(
            name: "TesseraCore",
            dependencies: ["TesseraFFI"],
            path: "Sources/TesseraCore"
        ),
        // AppKit + SwiftUI shell.
        .executableTarget(
            name: "Tessera",
            dependencies: ["TesseraCore", "TesseraFFI"],
            path: "Sources/Tessera"
        ),
        .testTarget(
            name: "TesseraCoreTests",
            dependencies: ["TesseraCore", "TesseraFFI"],
            path: "Tests/TesseraCoreTests"
        ),
    ],
    swiftLanguageModes: [.v6]
)
