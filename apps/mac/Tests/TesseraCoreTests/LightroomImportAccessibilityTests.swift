import AppKit
import SwiftUI
import XCTest
import TesseraCore
import TesseraFFI
@testable import Tessera

@objc private protocol ImportHostedAccessibilityApplication {
    @objc optional func isAccessibilityEnhancedUserInterface() -> Bool
    @objc optional func setAccessibilityEnhancedUserInterface(_ enabled: Bool)
}

@MainActor
final class LightroomImportAccessibilityTests: XCTestCase {
    private let warning = "operator X not implemented by the CPU reference renderer"

    private func elements(_ root: AnyObject) -> [AnyObject] {
        var seen = Set<ObjectIdentifier>()
        func walk(_ object: AnyObject) -> [AnyObject] {
            guard seen.insert(ObjectIdentifier(object)).inserted else { return [] }
            // SwiftUI.AccessibilityNode exposes AX selectors without protocol conformance.
            return [object] + (object.accessibilityChildren?() ?? []).flatMap { walk($0 as AnyObject) }
        }
        return walk(root)
    }

    private func host<V: View>(_ view: V, check: ([AnyObject]) throws -> Void) async rethrows {
        let policy = NSApplication.shared.activationPolicy()
        defer { _ = NSApplication.shared.setActivationPolicy(policy) }
        let bounds = NSRect(x: 0, y: 0, width: 820, height: 620)
        let host = NSHostingView(rootView: LayoutProbeHarness.root(view))
        let window = LayoutProbeHarness.window(contentRect: bounds, styleMask: .titled, backing: .buffered, defer: false)
        window.contentView = host
        host.frame = bounds
        defer { LayoutProbeHarness.dispose(window) }
        window.orderBack(nil)
        await LayoutProbeHarness.settleAsync(host)
        // Materialize SwiftUI's virtual AX tree locally, without requiring test-runner
        // TCC permission or changing the user's system accessibility preferences.
        let application = NSApp as AnyObject
        guard let previous = application.isAccessibilityEnhancedUserInterface?() else {
            return XCTFail("AppKit must support hosted accessibility activation")
        }
        application.setAccessibilityEnhancedUserInterface?(true)
        defer { application.setAccessibilityEnhancedUserInterface?(previous) }
        await LayoutProbeHarness.settleAsync(host)
        let nodes = elements(host)
        XCTAssertGreaterThan(nodes.count, 1, "Hosted AX hierarchy must be populated before checking content")
        try check(nodes)
    }

    private func value(_ identifier: String, in elements: [AnyObject],
                       file: StaticString = #filePath, line: UInt = #line) throws -> String {
        let node = try XCTUnwrap(elements.first { $0.accessibilityIdentifier?() == identifier },
                                 "Missing AX element: \(identifier)", file: file, line: line)
        let value: Any? = node.accessibilityValue?()
        return try XCTUnwrap(value as? String, "Missing AX value: \(identifier)", file: file, line: line)
    }

    private var failedSample: LrcatFidelitySample {
        LrcatFidelitySample(catalogId: 1, name: "portrait.jpg", path: "/portrait.jpg", status: .failed,
            message: warning, deltaEMean: 0, deltaEP95: 0, lightroomJpeg: Data(), tesseraJpeg: Data())
    }

    func testReportExposesCountsWarningsAndReadOnlyMarkdown() async throws {
        let report = LrcatReport(catalogPath: "/Fixture.lrcat", cancelled: false, imported: 4, resumed: 1,
            virtualCopies: 2, skipped: [LrcatSkip(name: "lost.jpg", path: "/lost.jpg", reason: "original not found")],
            unsupported: [LrcatIssue(category: "Develop", reason: warning, count: 9, examples: ["/photos/portrait.jpg", "/photos/two.jpg", "/photos/three.jpg", "/photos/four.jpg", "/photos/five.jpg"])],
            approximate: [LrcatIssue(category: "PointColors", reason: "hue range semantics unverified", count: 3,
                                     examples: ["/photos/portrait.jpg", "/photos/two.jpg"])],
            cloud: [LrcatIssue(category: "GenerativeRemove", reason: "requires Adobe cloud; not translatable",
                count: 2, examples: ["/photos/one.jpg", "/photos/two.jpg"])],
            albums: 5, albumGroups: 6, smartAlbums: 7, keywords: 8,
            selection: LrcatSelectionCounts(rejects: 0, keeps: 0, undecided: 0, grade1: 0, grade2: 0, grade3: 0, marked: 0),
            libraryPath: "/Photos/library.json", bundlePath: "/Photos/bundle", indexed: 5, seconds: 0.1)
        let markdown = "# Import report\nPhotos written: 4\n\(warning)"
        try await host(ReportStep(report: report, reportURL: nil, reportMarkdown: markdown,
            fidelity: LrcatFidelity(renderer: "native", previewsAvailable: true, samples: [failedSample]))) { nodes in
            let summary = try value("document.import.report.summary", in: nodes)
            for count in ["Photos written: 4", "Resumed: 1", "Albums: 5", "Album groups: 6", "Smart albums: 7",
                          "Keywords: 8", "Skipped: 1", "Virtual copies (bundle): 2"] {
                XCTAssertTrue(summary.contains(count), summary)
            }
            let warnings = try value("document.import.report.warnings", in: nodes)
            for text in [warning, "Develop", "9", "/photos/portrait.jpg", "/photos/two.jpg", "/photos/three.jpg", "/photos/four.jpg", "/photos/five.jpg", "lost.jpg", "original not found"] {
                XCTAssertTrue(warnings.contains(text), warnings)
            }
            XCTAssertFalse(warnings.contains("PointColors"), "approximate translations are not warnings: \(warnings)")
            XCTAssertEqual(try value("document.import.report.approximate", in: nodes),
                           "PointColors: 3 photos; e.g. hue range semantics unverified; /photos/portrait.jpg, /photos/two.jpg")
            let cloud = try value("document.import.report.cloud", in: nodes)
            for text in ["GenerativeRemove", "2 photos", "requires Adobe cloud; not translatable", "/photos/one.jpg", "/photos/two.jpg"] {
                XCTAssertTrue(cloud.contains(text), cloud)
            }
            XCTAssertFalse(warnings.contains("GenerativeRemove"))
            let fidelity = try value("document.import.report.fidelity", in: nodes)
            XCTAssertTrue(fidelity.contains(warning), fidelity)
            XCTAssertTrue(fidelity.contains("portrait.jpg"), fidelity)
            XCTAssertEqual(try value("document.import.report.markdown", in: nodes), markdown)
            let text = try XCTUnwrap(nodes.first { $0.accessibilityIdentifier?() == "document.import.report.markdown" })
            XCTAssertEqual(text.accessibilityRole?(), .textArea)
            XCTAssertFalse(text.isAccessibilitySelectorAllowed?(NSSelectorFromString("setAccessibilityValue:")) ?? true)
        }
    }

    func testCloudOnlyReportDoesNotClaimNoWarnings() async throws {
        let report = LrcatReport(catalogPath: "/Fixture.lrcat", cancelled: false, imported: 1, resumed: 0,
            virtualCopies: 0, skipped: [], unsupported: [], approximate: [],
            cloud: [LrcatIssue(category: "GenerativeFill", reason: "requires Adobe cloud; not translatable",
                count: 1, examples: ["/photos/one.jpg"])],
            albums: 0, albumGroups: 0, smartAlbums: 0, keywords: 0,
            selection: LrcatSelectionCounts(rejects: 0, keeps: 0, undecided: 0, grade1: 0, grade2: 0, grade3: 0, marked: 0),
            libraryPath: "/Photos/library.json", bundlePath: "/Photos/bundle", indexed: 1, seconds: 0.1)
        try await host(ReportStep(report: report, reportURL: nil, reportMarkdown: nil, fidelity: nil)) { nodes in
            let warnings = try value("document.import.report.warnings", in: nodes)
            XCTAssertNotEqual(warnings, "No warnings.")
            XCTAssertTrue(warnings.contains("Requires Adobe cloud"), warnings)
            XCTAssertEqual(try value("document.import.report.cloud", in: nodes),
                           "GenerativeFill: 1 photo; requires Adobe cloud; not translatable; /photos/one.jpg")
        }
    }

    func testFidelityExposesFailureBeforeSamplesExist() async throws {
        let importer = LightroomImportController()
        importer.step = .fidelity
        importer.error = "Fidelity preview failed: " + warning
        try await host(LightroomImportSheet(importer: importer)) { nodes in
            XCTAssertTrue(try value("document.import.report.fidelity", in: nodes).contains(warning))
        }
    }

    func testFidelityExposesFailedRendererMessageInSheet() async throws {
        let importer = LightroomImportController()
        importer.step = .fidelity
        importer.fidelity = FidelityGrid(samples: [failedSample])
        try await host(LightroomImportSheet(importer: importer)) { nodes in
            let fidelity = try value("document.import.report.fidelity", in: nodes)
            XCTAssertTrue(fidelity.contains("portrait.jpg"), fidelity)
            XCTAssertTrue(fidelity.contains("failed"), fidelity)
            XCTAssertTrue(fidelity.contains(warning), fidelity)
        }
    }
}
