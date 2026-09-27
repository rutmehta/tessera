import AppKit
import CoreGraphics
import ImageIO
import SwiftUI
import UniformTypeIdentifiers
import XCTest
import TesseraFFI
@testable import Tessera
@testable import TesseraCore

/// Background-safe window harness for shell layout tests (WP M2-56), after the layout audit's
/// window probe: the real `ContentView` in a titled, unified-toolbar window like the app's, the
/// process never activates (activation policy prohibited), the window is ordered to the back and
/// never made key, and captures use `screencapture -l <window> -x` (never the screen).
@MainActor
enum ShellHarness {
    enum State: String, CaseIterable { case library, raw, document, photoEditDevelop, photoEditMasks }

    static let sizes: [CGSize] = [CGSize(width: 960, height: 600), CGSize(width: 1280, height: 800),
                                  CGSize(width: 1440, height: 900), CGSize(width: 1728, height: 1117)]

    static var repoRoot: URL {
        URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    }

    /// Never become the active app: no Dock icon, no focus change for the person at the Mac.
    static func prepare() {
        NSApplication.shared.setActivationPolicy(.prohibited)
    }

    /// A model in `state`: 24 generated JPEGs on the engine, the RAW fixtures on the engine, or the
    /// stub layered document (sample layers) over a stub library.
    static func model(_ state: State, scratch: URL) throws -> AppModel {
        let model = AppModel()
        switch state {
        case .library:
            let folder = scratch.appendingPathComponent("shoot")
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            for n in 0..<24 { try writeJPEG(folder.appendingPathComponent(String(format: "IMG_%04d.jpg", n)), shade: 20 + n * 9) }
            model.install(try EngineLibrary.scan(folder: folder, appSupport: scratch.appendingPathComponent("support")))
        case .raw:
            let raw = scratch.appendingPathComponent("raw")
            try FileManager.default.copyItem(at: repoRoot.appendingPathComponent("fixtures/raw"), to: raw)
            model.install(try EngineLibrary.scan(folder: raw, appSupport: scratch.appendingPathComponent("support")))
        case .photoEditDevelop, .photoEditMasks:
            model.install(StubLibrary.synthetic(count: 40))
            model.enterPhotoEdit()
            model.photoInspectorTab = state == .photoEditMasks ? .masks : .develop
        case .document:
            model.install(StubLibrary.synthetic(count: 40))
            model.documents.policy = .stub
            model.documents.newDocument(model.documents.newSettings)
        }
        return model
    }

    /// The app's window (unified toolbar, full-size content) at `size` outer points.
    static func window(_ model: AppModel, size: CGSize, dark: Bool) -> (NSWindow, NSView) {
        model.documents.columnVisibility = model.isPhotoEditing ? .detailOnly : .all
        let appearance = NSAppearance(named: dark ? .darkAqua : .aqua)
        let controller = NSHostingController(rootView: ContentView.root(model: model))
        let window = NSWindow(contentRect: NSRect(origin: .zero, size: size),
                              styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
                              backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.toolbarStyle = .unified
        window.appearance = appearance
        window.contentViewController = controller
        window.orderBack(nil)
        settle(window, size: size)
        return (window, controller.view)
    }

    /// Unified toolbar height above the content's safe area (measured on macOS 26).
    static let toolbarHeight: CGFloat = 52

    /// Sets the outer frame (like a person resizing) and lets SwiftUI and AppKit settle. "960 × 600"
    /// is the declared minimum *content* size, so the outer window there is 600 + toolbar tall (a
    /// person cannot make it smaller; asking AppKit for less makes it grow the frame off-screen).
    static func settle(_ window: NSWindow, size: CGSize) {
        let outer = CGSize(width: max(size.width, ShellBudget.minWindow.width),
                           height: max(size.height, ShellBudget.minWindow.height + toolbarHeight))
        for _ in 0..<3 {
            window.setFrame(NSRect(x: 40, y: 40, width: outer.width, height: outer.height), display: true)
            window.contentView?.layoutSubtreeIfNeeded()
            RunLoop.main.run(until: Date().addingTimeInterval(0.25))
        }
    }

    /// The toolbar's bottom edge in the content view (flipped top inset).
    static func toolbarInset(_ window: NSWindow) -> CGFloat {
        guard let content = window.contentView else { return 0 }
        return max(content.safeAreaInsets.top, content.bounds.height - window.contentLayoutRect.height)
    }

    struct AXElement {
        let role: String
        let label: String
        /// Visible part, in the content view's coordinates, y down from the top.
        let frame: CGRect
        /// The view itself (for ancestor checks).
        let view: NSView
    }

    /// Visible actionable AppKit controls below `root`: native controls and the platform views
    /// SwiftUI uses for pop-up menus, text fields, sliders, checkboxes and colour wells, plus
    /// Tessera's own controls (ValueSlider, outlines). SwiftUI does not build its accessibility tree
    /// for a process no assistive client has queried (and a prohibited-activation test process
    /// cannot be queried), so drawn-only SwiftUI buttons are covered by the containment and
    /// toolbar checks rather than here. Scroll content outside its viewport has an empty
    /// visible rectangle and is skipped.
    static func actionables(_ root: NSView, in host: NSView) -> [AXElement] {
        var out: [AXElement] = []
        func visit(_ view: NSView) {
            guard !view.isHiddenOrHasHiddenAncestor, view.alphaValue > 0.01 else { return }
            if (view is NSControl && !(view is NSScroller)) || view is NSTableView {   // overlay scrollers overlap content by design
                // The frame, clipped by every clip view / clipping ancestor (visibleRect is not
                // reliable for SwiftUI's platform-view hosts).
                var r = view.convert(view.bounds, to: host)
                var ancestor = view.superview
                while let a = ancestor, a !== host {
                    if a is NSClipView || a.clipsToBounds { r = r.intersection(a.convert(a.bounds, to: host)) }
                    ancestor = a.superview
                }
                if !r.isNull, r.width > 1, r.height > 1 {
                    let flipped = host.isFlipped ? r : CGRect(x: r.minX, y: host.bounds.height - r.maxY, width: r.width, height: r.height)
                    let role = view.accessibilityRole()?.rawValue ?? String(describing: type(of: view))
                    let label = view.accessibilityLabel() ?? view.accessibilityTitle() ?? view.accessibilityIdentifier()
                    out.append(AXElement(role: role, label: label.isEmpty ? String(describing: type(of: view)) : label,
                                         frame: flipped, view: view))
                }
                if view is NSTableView || view is NSSegmentedControl { return }   // rows / segments are one control
            }
            for child in view.subviews { visit(child) }
        }
        visit(root)
        return out
    }

    /// Pairs of actionable siblings (neither contains the other in the AX tree) whose frames overlap
    /// by more than `tolerance` points in both axes.
    static func overlaps(_ elements: [AXElement], within visible: CGRect, tolerance: CGFloat = 2) -> [String] {
        let shown = elements.filter { visible.intersects($0.frame) }
        var out: [String] = []
        for i in shown.indices {
            for j in shown.indices where j > i {
                let a = shown[i], b = shown[j]
                if a.view.isDescendant(of: b.view) || b.view.isDescendant(of: a.view) { continue }
                let r = a.frame.intersection(b.frame)
                if !r.isNull, r.width > tolerance, r.height > tolerance {
                    out.append("\(a.role) '\(a.label)' \(NSStringFromRect(a.frame)) × \(b.role) '\(b.label)' \(NSStringFromRect(b.frame))")
                }
            }
        }
        return out
    }

    /// `screencapture -l <window> -x`, then downscaled to at most 1400 px wide.
    static func capture(_ window: NSWindow, to url: URL) throws {
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        let shot = Process()
        shot.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
        shot.arguments = ["-l", String(window.windowNumber), "-x", "-o", url.path]
        try shot.run(); shot.waitUntilExit()
        let sips = Process()
        sips.executableURL = URL(fileURLWithPath: "/usr/bin/sips")
        sips.arguments = ["-Z", "1400", url.path]
        sips.standardOutput = FileHandle.nullDevice
        try sips.run(); sips.waitUntilExit()
    }

    static func writeJPEG(_ url: URL, shade: Int) throws {
        let w = 96, h = 64
        var pixels = [UInt8](repeating: 0, count: w * h * 4)
        for y in 0..<h {
            for x in 0..<w {
                let i = (y * w + x) * 4
                pixels[i] = UInt8(clamping: shade + x)
                pixels[i + 1] = UInt8(clamping: shade / 2 + y * 2)
                pixels[i + 2] = UInt8(clamping: 255 - shade)
                pixels[i + 3] = 255
            }
        }
        let provider = try XCTUnwrap(CGDataProvider(data: Data(pixels) as CFData))
        let image = try XCTUnwrap(CGImage(width: w, height: h, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: w * 4,
                                          space: CGColorSpace(name: CGColorSpace.sRGB)!,
                                          bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
                                          provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent))
        let dest = try XCTUnwrap(CGImageDestinationCreateWithURL(url as CFURL, UTType.jpeg.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(dest, image, [kCGImageDestinationLossyCompressionQuality: 0.9] as CFDictionary)
        XCTAssertTrue(CGImageDestinationFinalize(dest))
    }
}
