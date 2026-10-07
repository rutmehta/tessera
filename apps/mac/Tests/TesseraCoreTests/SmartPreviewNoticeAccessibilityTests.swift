import AppKit
import SwiftUI
import XCTest
@testable import Tessera

/// LR-13: a proxy rendered without an optional setting must say so in the Develop/loupe UI.
@MainActor
final class SmartPreviewNoticeAccessibilityTests: XCTestCase {
    private func elements(_ root: AnyObject) -> [AnyObject] {
        var seen = Set<ObjectIdentifier>()
        func walk(_ object: AnyObject) -> [AnyObject] {
            guard seen.insert(ObjectIdentifier(object)).inserted else { return [] }
            return [object] + (object.accessibilityChildren?() ?? []).flatMap { walk($0 as AnyObject) }
        }
        return walk(root)
    }

    private func host<V: View>(_ view: V, check: ([AnyObject]) throws -> Void) async rethrows {
        let policy = NSApplication.shared.activationPolicy()
        defer { _ = NSApplication.shared.setActivationPolicy(policy) }
        let bounds = NSRect(x: 0, y: 0, width: 640, height: 420)
        let host = NSHostingView(rootView: LayoutProbeHarness.root(view))
        let window = LayoutProbeHarness.window(contentRect: bounds, styleMask: .titled, backing: .buffered, defer: false)
        window.contentView = host
        host.frame = bounds
        defer { LayoutProbeHarness.dispose(window) }
        window.orderBack(nil)
        await LayoutProbeHarness.settleAsync(host)
        let attribute = NSAccessibility.Attribute(rawValue: "AXEnhancedUserInterface")
        let previous = NSApp.accessibilityAttributeValue(attribute)
        NSApp.accessibilitySetValue(true, forAttribute: attribute)
        defer { NSApp.accessibilitySetValue(previous, forAttribute: attribute) }
        await LayoutProbeHarness.settleAsync(host)
        let nodes = elements(host)
        XCTAssertGreaterThan(nodes.count, 1, "Hosted AX hierarchy must be populated before checking content")
        try check(nodes)
    }

    private func node(_ identifier: String, in elements: [AnyObject]) -> AnyObject? {
        elements.first { $0.accessibilityIdentifier?() == identifier }
    }

    func testDegradedProxyListsEverySettingNotAppliedWithStableIdentifiers() async throws {
        let notices = ["Creative look unavailable; shown without it.",
                       "Lens profile unavailable; shown without it.",
                       "Rendered using the available Smart Preview dynamic range."]
        try await host(SmartPreviewLoupeBadge(notices: notices)) { nodes in
            let notice = try XCTUnwrap(node("loupe.smart-preview-notice", in: nodes), "Missing visible notice")
            let value: Any? = notice.accessibilityValue?()
            let text = try XCTUnwrap(value as? String)
            for sentence in notices { XCTAssertTrue(text.contains(sentence), text) }
            let badge = try XCTUnwrap(node("loupe.smart-preview-badge", in: nodes))
            let label = try XCTUnwrap(badge.accessibilityLabel?())
            XCTAssertTrue(label.hasPrefix("Smart Preview, original offline"), label)
            for sentence in notices { XCTAssertTrue(label.contains(sentence), label) }
        }
        XCTAssertEqual(SmartPreviewLoupeBadge.noticeText(notices), notices.joined(separator: "\n"),
                       "The visible text is exactly the listed settings")
    }

    func testUndegradedProxyShowsOnlyTheBadge() async throws {
        try await host(SmartPreviewLoupeBadge(notices: [])) { nodes in
            XCTAssertNil(node("loupe.smart-preview-notice", in: nodes))
            let badge = try XCTUnwrap(node("loupe.smart-preview-badge", in: nodes))
            XCTAssertEqual(badge.accessibilityLabel?(), "Smart Preview, original offline")
        }
    }
}
