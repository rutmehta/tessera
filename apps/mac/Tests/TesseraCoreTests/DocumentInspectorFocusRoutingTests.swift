import AppKit
import SwiftUI
import XCTest
@testable import Tessera
@testable import TesseraCore

/// A-run diagnostic/regression for the actual Properties name -> Load LUT path.
/// No global keyboard setting changes, private responder names or forced button focus.
@MainActor
final class DocumentInspectorFocusRoutingTests: XCTestCase {
    private func event(_ code: UInt16, _ text: String, window: NSWindow,
                       modifiers: NSEvent.ModifierFlags = []) throws -> NSEvent {
        try XCTUnwrap(NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: modifiers,
            timestamp: 0, windowNumber: window.windowNumber, context: nil,
            characters: text, charactersIgnoringModifiers: text, isARepeat: false, keyCode: code))
    }

    private func views(_ root: NSView) -> [NSView] {
        [root] + root.subviews.flatMap { views($0) }
    }

    private func record(_ stage: String, window: NSWindow, host: NSView) {
        let responder = window.firstResponder.map { "\(type(of: $0)) \(ObjectIdentifier($0))" } ?? "nil"
        var lines = ["\(stage): firstResponder=\(responder); fullKeyboardAccess=\(NSApp.isFullKeyboardAccessEnabled)"]
        for view in views(host) {
            lines.append("\(type(of: view)) \(ObjectIdentifier(view)) bounds=\(view.bounds) accepts=\(view.acceptsFirstResponder) AXfocused=\(view.isAccessibilityFocused()) AXid=\(view.accessibilityIdentifier()) AXlabel=\(view.accessibilityLabel() ?? "nil")")
        }
        let text = lines.joined(separator: "\n")
        print(text)
        let attachment = XCTAttachment(string: text)
        attachment.name = stage
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    func testHostedPropertiesNameToLoadLUTRetainsNavigationAndActivation() async throws {
        let priorPolicy = NSApplication.shared.activationPolicy()
        defer { _ = NSApplication.shared.setActivationPolicy(priorPolicy) }
        ShellHarness.prepare()
        let model = AppModel()
        model.documents.engine = StubDocumentEngine()
        model.documents.newDocument(NewDocumentSettings(width: 32, height: 32))
        let document = try XCTUnwrap(model.documents.current)
        document.addAdjustment(.colorLookup)
        // Actual Properties controls, without DocumentView.onAppear shared-owner attachment.
        let host = NSHostingView(rootView: PropertiesPanel(document: document)
            .frame(width: 288, height: 848, alignment: .topLeading))
        let bounds = NSRect(x: 0, y: 0, width: 288, height: 848)
        let window = NSWindow(contentRect: bounds, styleMask: .titled, backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.contentView = host
        window.setContentSize(bounds.size)
        host.frame = bounds
        defer { window.orderOut(nil); window.contentView = nil; window.close() }
        window.orderBack(nil)
        host.layoutSubtreeIfNeeded()
        await Task.yield() // let the initial SwiftUI transaction finish; no timed wait
        host.layoutSubtreeIfNeeded()
        window.recalculateKeyViewLoop()
        record("hosted", window: window, host: host)
        XCTAssertEqual(host.bounds, bounds)
        let name = try XCTUnwrap(views(host).compactMap { $0 as? NSTextField }.first { $0.isEditable },
                                 "actual Properties name field must be hosted")
        XCTAssertTrue(window.makeFirstResponder(name))
        let router = KeyRouter(model: model)
        let tab = try event(48, "\t", window: window)
        XCTAssertFalse(router.handle(tab), "name editing retains native Tab")
        window.selectNextKeyView(nil)
        record("after-name-Tab", window: window, host: host)
        let load = try XCTUnwrap(views(host).first {
            $0.accessibilityIdentifier() == "document.properties.colorLookup.load"
        }, "diagnostic requires actual Load LUT native host; see responder/AX attachment if SwiftUI exposes it differently")
        let responderView = window.firstResponder as? NSView
        let loadOwnsFocus = load.isAccessibilityFocused() || responderView === load
            || (responderView?.isDescendant(of: load) ?? false)
        guard loadOwnsFocus else {
            if !NSApp.isFullKeyboardAccessEnabled {
                throw XCTSkip("native traversal did not focus Load LUT with current keyboard policy; no settings changed, path NOT qualified; see diagnostic")
            }
            XCTFail("native name-to-Load focus not established; inspect diagnostic before interpreting router results")
            return
        }
        // Exercise the same handler used by the local monitor, without installing
        // another app-wide monitor in the test process or opening the LUT chooser.
        for (code, text, modifiers) in [(UInt16(48), "\t", NSEvent.ModifierFlags()),
                                        (48, "\t", .shift), (49, " ", []), (36, "\r", []), (76, "\r", [])] {
            let before = window.firstResponder
            XCTAssertFalse(router.handle(try event(code, text, window: window, modifiers: modifiers)),
                           "focused Load LUT owns navigation/activation code=\(code) modifiers=\(modifiers.rawValue)")
            XCTAssertFalse(model.documents.panelsHidden)
            XCTAssertFalse(model.documents.spaceHeld)
            XCTAssertTrue(window.firstResponder === before)
            // Keep each diagnostic independent even on the known failing router.
            model.documents.setPanelsHidden(false)
            model.documents.spaceHeld = false
        }
        window.selectNextKeyView(nil)
        record("after-Load-Tab", window: window, host: host)
        XCTAssertFalse(window.firstResponder === responderView, "native Tab must advance from Load LUT")
        window.selectPreviousKeyView(nil)
        record("after-next-ShiftTab", window: window, host: host)
        XCTAssertTrue(window.firstResponder === responderView, "reverse traversal must return to the captured Load responder")
    }

    func testViewportRetainsTabToggleAndSpacePan() throws {
        let model = AppModel()
        model.documents.engine = StubDocumentEngine()
        model.documents.newDocument(NewDocumentSettings(width: 32, height: 32))
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 288, height: 848),
                              styleMask: .titled, backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        defer { window.close() }
        // Non-control content is the routing negative control, not a hosted-button surrogate.
        XCTAssertTrue(window.makeFirstResponder(nil))
        let router = KeyRouter(model: model)
        XCTAssertTrue(router.handle(try event(48, "\t", window: window)))
        XCTAssertTrue(model.documents.panelsHidden)
        XCTAssertTrue(router.handle(try event(49, " ", window: window)))
        XCTAssertTrue(model.documents.spaceHeld)
        model.documents.spaceHeld = false
    }
}
