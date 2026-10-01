import AppKit
import TesseraCore

/// Opt-in P19 diagnostic; no production layout changes or screen capture.
@MainActor
enum FilterLayoutReproduction {
    // Geometry arguments allow the diagnostic's target to be tested on other displays.
    static func viewportTarget(visibleFrame: NSRect, scale: CGFloat,
                               hostSize: NSSize, viewportSize: NSSize) -> NSSize {
        NSSize(width: 3840, height: 2160)
    }

    static func log(_ text: String) {
        FileHandle.standardError.write(Data("filter-layout: \(text)\n".utf8))
    }

    static func run(model: AppModel, window: NSWindow) async -> [String] {
        let previousHandler = NSGetUncaughtExceptionHandler()
        NSSetUncaughtExceptionHandler { exception in
            let text = "filter-layout: EXCEPTION \(exception.name.rawValue): \(exception.reason ?? "no reason")\n\(exception.callStackSymbols.joined(separator: "\n"))\n"
            FileHandle.standardError.write(Data(text.utf8))
        }
        let keys = ["NSViewLayoutFeedbackLoopDebugging", "NSConstraintBasedLayoutLogUnsatisfiable", "InspectorPanel.History"]
        let previous = keys.map { UserDefaults.standard.object(forKey: $0) }
        for key in keys { UserDefaults.standard.set(true, forKey: key) }
        defer {
            NSSetUncaughtExceptionHandler(previousHandler)
            for (key, value) in zip(keys, previous) {
                if let value { UserDefaults.standard.set(value, forKey: key) }
                else { UserDefaults.standard.removeObject(forKey: key) }
            }
        }
        let previousProbe = DocumentInspectorProbe.isEnabled
        DocumentInspectorProbe.isEnabled = true
        defer { DocumentInspectorProbe.isEnabled = previousProbe }
        let ws = model.documents
        guard let doc = ws.current else { return ["no live document"] }
        model.showInspector = true
        ws.columnVisibility = .all
        ws.inspectorTab = .stack
        guard let layer = doc.layers.first(where: { $0.kind == .pixel }),
              let gaussian = ws.filters.catalogue(doc).first(where: { $0.id == "gaussian_blur" }) else {
            return ["no pixel layer or Gaussian Blur"]
        }
        doc.select(layer.id)
        ws.filters.open(gaussian, doc)
        defer { ws.filters.filterSheet?.cancel() }
        for _ in 0..<100 where window.attachedSheet == nil || doc.viewport == nil {
            try? await Task.sleep(for: .milliseconds(50))
        }
        guard let sheet = ws.filters.filterSheet, window.attachedSheet != nil,
              let viewport = doc.viewport else { return ["filter sheet or live viewport not attached"] }
        let initial = window.frame
        log("document=\(doc.info.width)x\(doc.info.height) backend=\(doc.info.backend) history=\(doc.history.count) inspector=\(model.showInspector) attachedSheet=true initial=\(initial) scale=\(window.backingScaleFactor)")
        var failures: [String] = []
        // Both interpretations: literal 3840×2160 host points, then the historical 4K device viewport.
        for mode in ["host-points", "viewport-pixels"] {
            let scale = window.backingScaleFactor
            let pixels = viewportTarget(visibleFrame: window.screen?.visibleFrame ?? window.frame,
                                        scale: scale, hostSize: window.frame.size,
                                        viewportSize: viewport.bounds.size)
            let target = mode == "host-points" ? NSSize(width: 3840, height: 2160) :
                NSSize(width: pixels.width / scale + window.frame.width - viewport.bounds.width,
                       height: pixels.height / scale + window.frame.height - viewport.bounds.height)
            for size in [target, initial.size] {
                log("BEFORE \(mode) requested=\(size) sheet=\(window.attachedSheet != nil)")
                sheet.set(gaussian.params[0], .number(9), final: false)
                window.setFrame(NSRect(origin: initial.origin, size: size), display: true)
                window.contentView?.layoutSubtreeIfNeeded()
                try? await Task.sleep(for: .seconds(1))
                log("AFTER \(mode) frame=\(window.frame.size) viewport=\(viewport.bounds.size) pixels=\(viewport.bounds.width * scale)x\(viewport.bounds.height * scale) sheet=\(window.attachedSheet != nil) key=\(window.isKeyWindow) main=\(window.isMainWindow)")
                if abs(window.frame.width - size.width) > 2 || abs(window.frame.height - size.height) > 2 {
                    // AppKit may constrain an oversized point-height to the display. Record it;
                    // P19 requires exception-free layout and the exact device-pixel viewport below.
                    log("CLAMP \(mode): requested \(size), actual \(window.frame.size)")
                    if mode == "viewport-pixels" || size == initial.size {
                        failures.append("host failed to reach the 4K viewport size or restore initial size")
                    }
                }
                if mode == "viewport-pixels", size == target,
                   abs(viewport.bounds.width * scale - 3840) > 2 || abs(viewport.bounds.height * scale - 2160) > 2 {
                    failures.append("4K viewport not reached")
                }
                log("inspector regions=\(DocumentInspectorProbe.frames)")
                if window.attachedSheet == nil { failures.append("sheet disappeared during resize") }
                if window.isKeyWindow || window.isMainWindow { failures.append("host became key/main") }
            }
        }
        log("done failures=\(failures)")
        return failures
    }
}
