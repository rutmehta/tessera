// swift-tools-version: 6.0
import PackageDescription
let package = Package(name: "SmartPreviewModelGate", platforms: [.macOS(.v14)], products: [.library(name: "TesseraCore", targets: ["TesseraCore"])], targets: [.target(name: "TesseraCore"), .testTarget(name: "TesseraCoreTests", dependencies: ["TesseraCore"])])
