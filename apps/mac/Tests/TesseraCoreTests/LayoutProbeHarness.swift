import AppKit
import ObjectiveC
import CoreGraphics
import ImageIO
import UniformTypeIdentifiers
import TesseraFFI
@testable import TesseraCore
import SwiftUI
import XCTest
@testable import Tessera

/// Shared synchronization for final-geometry probes; no geometry acceptance predicate or tolerance.
@MainActor
enum LayoutProbeHarness {
    // Worktree gates share the xctest preferences domain. In particular, another process can
    // collapse History while this process measures its expanded geometry. Use the same explicit
    // app-directory isolation as the app, with a private store for this test process.
    private static let defaultsDirectory = FileManager.default.temporaryDirectory
        .appendingPathComponent("tessera-layout-defaults-\(UUID().uuidString)")

    /// Never become the active app: no Dock icon, no focus change for the person at the Mac.
    static func prepare() {
        KeyboardAccessHarness.install()
        _ = installNonblockingAnimations
        precondition(AppDefaultsIsolation.installForLaunch(
            arguments: ["TesseraTests", "--app-dir", defaultsDirectory.path], environment: [:]) != nil,
            "The layout harness requires an isolated preferences store")
        NSApplication.shared.setActivationPolicy(.prohibited)
    }

    // SwiftUI also creates NSAnimation instances that are not NSProgressIndicator descendants.
    // Each nonblockingThreaded instance reserves a dispatch worker on macOS 26. The layout suite
    // creates enough transient hosts to exhaust that pool. Keep animation on the main run loop
    // in this test process, installed once before any harness windows are created.
    private static let installNonblockingAnimations: Void = {
        let original = class_getInstanceMethod(NSAnimation.self, NSSelectorFromString("startAnimation"))!
        let replacement = class_getInstanceMethod(NSAnimation.self, #selector(NSAnimation.tesseraLayoutStart))!
        method_exchangeImplementations(original, replacement)
    }()

    /// Static layout checks create many background views. One threaded indeterminate animation per
    /// indicator can exhaust dispatch workers and starve unrelated async tests. Keep their normal
    /// geometry while stopping existing workers; subsequent animations use timers. Production views are unaffected.
    static func useTimerAnimations(in view: NSView) {
        if let progress = view as? NSProgressIndicator {
            progress.stopAnimation(nil)
            progress.usesThreadedAnimation = false
        }
        for child in view.subviews { useTimerAnimations(in: child) }
    }

    static func dispose(_ window: NSWindow) {
        if let content = window.contentView { useTimerAnimations(in: content) }
        window.orderOut(nil)
        window.contentViewController = nil
        window.contentView = nil
        window.close()
    }

    /// One offscreen rendering path for OCR and its optional evidence image.
    static func bitmap(_ view: NSView) throws -> NSBitmapImageRep {
        // OCR must see Retina-sized glyphs even when the build machine's display is 1×.
        // Set both pixel dimensions and logical size before caching so AppKit draws at 2×;
        // allocating via bitmapImageRepForCachingDisplay would inherit the screen scale.
        let bitmap = try XCTUnwrap(NSBitmapImageRep(bitmapDataPlanes: nil,
            pixelsWide: Int((view.bounds.width * 2).rounded(.up)),
            pixelsHigh: Int((view.bounds.height * 2).rounded(.up)),
            bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
            colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0))
        bitmap.size = view.bounds.size
        view.cacheDisplay(in: view.bounds, to: bitmap)
        return bitmap
    }

    static func root<V: View>(_ view: V) -> some View {
        prepare()
        return view.transaction { transaction in
            transaction.animation = nil
            transaction.disablesAnimations = true
        }
    }

    /// Window ordering/closing animation is separate from SwiftUI transactions. In a locked
    /// console it can strand NSAnimation workers until the process exhausts its dispatch pool.
    /// Test fixtures need final window geometry only, so disable lifecycle animation at creation.
    static func window(contentRect: NSRect, styleMask: NSWindow.StyleMask,
                       backing: NSWindow.BackingStoreType, defer flag: Bool) -> NSWindow {
        prepare()
        let window = NSWindow(contentRect: contentRect, styleMask: styleMask, backing: backing, defer: flag)
        window.isReleasedWhenClosed = false
        window.animationBehavior = .none
        return window
    }

    private struct ViewFrame: Equatable {
        let identity: ObjectIdentifier
        let frame: CGRect
        let bounds: CGRect
        let hidden: Bool
    }

    /// Both synchronous XCTest probes and async MainActor probes use this same settling engine.
    /// SwiftUI has no public pending-layout flag: observe its geometry callbacks alongside the
    /// native tree, and require 50 ms of quiet across run-loop turns (including a display tick).
    @MainActor
    private struct Probe {
        let root: NSView?
        let deadline: TimeInterval
        var previous: [ViewFrame]?
        var previousProbe: [String: CGRect]?
        var previousMeasurement: [CGRect]?
        var quietSince: TimeInterval?
        var diagnostic = ""

        init(_ view: NSView?, timeout: TimeInterval) {
            root = view?.window?.contentView?.superview ?? view
            deadline = ProcessInfo.processInfo.systemUptime + timeout
        }
        var remaining: TimeInterval { deadline - ProcessInfo.processInfo.systemUptime }
        func descendants(_ node: NSView) -> [NSView] {
            [node] + node.subviews.flatMap { descendants($0) }
        }
        func flush() {
            if let root { LayoutProbeHarness.useTimerAnimations(in: root) }
            NSAnimationContext.runAnimationGroup { context in
                context.duration = 0
                context.allowsImplicitAnimation = false
                root?.layoutSubtreeIfNeeded()
                root?.displayIfNeeded()
                root?.window?.displayIfNeeded()
            }
        }
        mutating func read(_ measured: [CGRect]) -> Bool {
            let nodes = root.map { descendants($0) } ?? []
            let frames = nodes.map {
                ViewFrame(identity: ObjectIdentifier($0), frame: $0.convert($0.bounds, to: root),
                          bounds: $0.bounds, hidden: $0.isHiddenOrHasHiddenAncestor)
            }
            let probe = DocumentInspectorProbe.isEnabled ? DocumentInspectorProbe.frames : [:]
            let pending = nodes.filter { $0.needsLayout || $0.needsUpdateConstraints }
            let now = ProcessInfo.processInfo.systemUptime
            diagnostic = "pending: \(pending.map { String(describing: type(of: $0)) }); measurements: \(String(describing: previousMeasurement)) -> \(measured)"
            if pending.isEmpty, frames == previous, probe == previousProbe, measured == previousMeasurement {
                // Intrinsic-size queries are synchronous and have no display tick to await.
                if root == nil { return true }
                if let quietSince, now - quietSince >= 0.05 { return true }
                if quietSince == nil { quietSince = now }
            } else {
                quietSince = nil
            }
            previous = frames
            previousProbe = probe
            previousMeasurement = measured
            return false
        }
    }

    @discardableResult
    static func settle(_ view: NSView?, timeout: TimeInterval = 2,
                       measurement: () -> [CGRect] = { [] },
                       file: StaticString = #filePath, line: UInt = #line) -> Bool {
        var probe = Probe(view, timeout: timeout)
        let initial = measurement()
        probe.flush()
        _ = probe.read(initial)
        repeat {
            RunLoop.main.run(until: Date().addingTimeInterval(max(0, min(0.005, probe.remaining))))
            let measured = measurement()
            probe.flush()
            if probe.read(measured) { return true }
        } while probe.remaining > 0
        XCTFail("Layout did not settle within \(timeout)s; \(probe.diagnostic)", file: file, line: line)
        return false
    }

    /// Async tests must also release MainActor: a nested RunLoop alone cannot run queued actor jobs.
    @discardableResult
    static func settleAsync(_ view: NSView, timeout: TimeInterval = 2,
                            file: StaticString = #filePath, line: UInt = #line) async -> Bool {
        var probe = Probe(view, timeout: timeout)
        probe.flush()
        _ = probe.read([])
        repeat {
            try? await Task.sleep(for: .seconds(max(0, min(0.005, probe.remaining))))
            probe.flush()
            if probe.read([]) { return true }
        } while probe.remaining > 0
        XCTFail("Layout did not settle within \(timeout)s; \(probe.diagnostic)", file: file, line: line)
        return false
    }

    static func fittingSize<V: View>(_ view: V, in proposal: CGSize) -> CGSize {
        let host = NSHostingController(rootView: root(view))
        var size = host.sizeThatFits(in: proposal)
        // Intrinsic-size queries have no window layout. Unattached hosting views retain native
        // layout flags until attachment; only the returned size is the measurement in this case.
        settle(nil, measurement: {
            size = host.sizeThatFits(in: proposal)
            return [CGRect(origin: .zero, size: size)]
        })
        return size
    }
}

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
        model.documents.columnVisibility = model.isPhotoEditing || model.isReviewing ? .detailOnly : .all
        let appearance = NSAppearance(named: dark ? .darkAqua : .aqua)
        let controller = NSHostingController(rootView: LayoutProbeHarness.root(ContentView.root(model: model)))
        let window = LayoutProbeHarness.window(contentRect: NSRect(origin: .zero, size: size),
                              styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
                              backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.toolbarStyle = .unified
        window.appearance = appearance
        window.contentViewController = controller
        // Establish final bounds before ordering: otherwise the first SwiftUI geometry callbacks
        // describe the hosting controller's preferred size, not the requested test window.
        let outer = CGSize(width: max(size.width, ShellBudget.minWindow.width),
                           height: max(size.height, ShellBudget.minWindow.height + toolbarHeight))
        window.setFrame(NSRect(x: 40, y: 40, width: outer.width, height: outer.height), display: false)
        controller.view.frame = window.contentView?.bounds ?? .zero
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
        window.setFrame(NSRect(x: 40, y: 40, width: outer.width, height: outer.height), display: true)
        if let content = window.contentView { LayoutProbeHarness.settle(content) }
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

    /// Optional whole-window evidence; OCR uses LayoutProbeHarness.bitmap directly.
    static func capture(_ window: NSWindow, to url: URL) throws {
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        let shot = Process()
        shot.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
        shot.arguments = ["-l", String(window.windowNumber), "-x", "-o", url.path]
        try shot.run(); shot.waitUntilExit()
        XCTAssertEqual(shot.terminationStatus, 0, "Window capture failed: \(url.lastPathComponent)")
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

private extension NSAnimation {
    @objc dynamic func tesseraLayoutStart() {
        animationBlockingMode = .nonblocking
        tesseraLayoutStart() // Original implementation after the test-only exchange.
    }
}
