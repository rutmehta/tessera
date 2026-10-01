import AppKit
import SwiftUI
import Vision
import XCTest
@testable import Tessera
@testable import TesseraCore

/// A populated inspector, not the empty preview-only Masks tab. Native containment checks cannot
/// catch SwiftUI text wrapping/truncation, so inspect the text actually painted by the real panel.
@MainActor
final class MasksPanelLayoutTests: XCTestCase {
    func testPopulatedInspectorKeepsComponentActionsReadableAtMinimumWidth() async throws {
        ShellHarness.prepare()
        let scratch = FileManager.default.temporaryDirectory.appendingPathComponent("masks-layout-\(UUID().uuidString)")
        let folder = scratch.appendingPathComponent("photos")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: scratch) }
        try ShellHarness.writeJPEG(folder.appendingPathComponent("photo.jpg"), shade: 100)
        let model = AppModel()
        defer { model.closeDevelop() }
        model.install(try EngineLibrary.scan(folder: folder, appSupport: scratch.appendingPathComponent("support")))
        model.enterPhotoEdit()
        model.openDevelop(for: try XCTUnwrap(model.focusedItem))
        let deadline = Date().addingTimeInterval(30)
        while model.developStatus == .loading, Date() < deadline {
            try await Task.sleep(for: .milliseconds(20))
        }
        XCTAssertEqual(model.developStatus, .ready)
        let controller = try XCTUnwrap(model.develop)
        addTeardownBlock { await controller.close() }
        let masks = MaskTools(model: model)
        let group = try XCTUnwrap(controller.addMask(
            LinearGradientShape(start: (0.2, 0.2), end: (0.8, 0.8)).json))
        masks.refresh()
        masks.select(group)
        XCTAssertEqual(masks.selected?.components.count, 1)
        let history = controller.history

        // Literal inspector bounds from the design contract, including the standard panel gutter.
        for width in [CGFloat(288), CGFloat(380)] {
            for dark in [false, true] {
                let tag = "masks-components-\(Int(width))-\(dark ? "dark" : "light")"
                let host = NSHostingController(rootView:
                    ScrollView {
                        PanelSection("Masks") { MasksPanel(model: model, masks: masks) }
                    }
                    .background(Theme.panel)
                    .tint(Theme.accent))
                let window = NSWindow(contentRect: NSRect(x: 40, y: 40, width: width, height: 720),
                                      styleMask: [.titled, .closable], backing: .buffered, defer: false)
                window.isReleasedWhenClosed = false
                window.appearance = NSAppearance(named: dark ? .darkAqua : .aqua)
                window.contentViewController = host
                window.orderBack(nil)
                defer { window.orderOut(nil); window.contentViewController = nil }
                for _ in 0..<3 {
                    window.setContentSize(CGSize(width: width, height: 720))
                    host.view.layoutSubtreeIfNeeded()
                    try await Task.sleep(for: .milliseconds(100))
                }
                XCTAssertFalse(NSApp.isActive)
                XCTAssertEqual(host.view.bounds.width, width, accuracy: 1, tag)
                XCTAssertTrue(ShellLayoutAudit.containmentViolations(in: host.view, columnContent: true).isEmpty, tag)
                // Render the actual hosting view offscreen. WindowServer capture may be unavailable
                // on a background build machine, and is not needed to check the painted labels.
                let bitmap = try XCTUnwrap(host.view.bitmapImageRepForCachingDisplay(in: host.view.bounds))
                host.view.cacheDisplay(in: host.view.bounds, to: bitmap)
                let words = try renderedWords(try XCTUnwrap(bitmap.cgImage))
                for label in ["Components", "Add", "Subtract", "Intersect"] {
                    // Vision can join the adjacent menu chevron to the full word as "v".
                    // Accept that glyph only; never expand a truncated prefix such as "Sub…".
                    XCTAssertTrue(words.contains(label) || words.contains(label + "v"),
                                  "\(tag): full label '\(label)' is not painted; recognized \(words.sorted())")
                }
                if let captureDirectory = ProcessInfo.processInfo.environment["TESSERA_LAYOUT_CAPTURE"] {
                    let directory = URL(fileURLWithPath: captureDirectory)
                    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
                    let png = try XCTUnwrap(bitmap.representation(using: .png, properties: [:]))
                    try png.write(to: directory.appendingPathComponent(tag + ".png"))
                }
            }
        }
        XCTAssertEqual(masks.selected?.id, group)
        XCTAssertEqual(controller.history.headLabel, history.headLabel, "Layout does not mutate photo history")
        model.closeDevelop()
        await controller.close()
    }

    private func renderedWords(_ image: CGImage) throws -> Set<String> {
        let request = VNRecognizeTextRequest()
        request.recognitionLevel = .accurate
        request.recognitionLanguages = ["en-US"]
        // Do not let language correction expand the very truncations this regression detects.
        request.usesLanguageCorrection = false
        try VNImageRequestHandler(cgImage: image).perform([request])
        return Set((request.results ?? []).flatMap { observation in
            (observation.topCandidates(1).first?.string ?? "")
                .split(whereSeparator: { $0.isWhitespace }).map(String.init)
        })
    }
}
