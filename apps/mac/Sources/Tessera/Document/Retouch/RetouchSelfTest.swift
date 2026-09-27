import AppKit
import TesseraCore

/// `--retouch-selftest <dir>` (test aid, WP B5-09): after the library loads, Edit in Layers on `sample.dng`,
/// then through the same paths the UI takes: the Remove tool (⇧J) and a synthesized stroke on the viewport
/// (one "Remove" node, timed), undo / redo, save and reopen the `.tessera-doc`; Edit ▸ Content-Aware Fill on a
/// marquee; a slow Remove on a large selection cancelled from the options bar (history unchanged); Remove
/// Distractions review (one suggestion kept); Filter ▸ Neural Filters… for each filter (missing-weight
/// messages, Skin Smoothing to a new layer). Each step prints `retouch-selftest: step <n> <name> window
/// <x> <y> <w> <h>` (for `screencapture -R`) and holds; checks print `check <name> ok|FAIL`. Quits at the end.
@MainActor
final class RetouchSelfTest {
    private let model: AppModel
    private let dir: URL
    private let hold: Double
    private var failures = 0
    private var step = 0
    private static var started = false

    static func startIfRequested() {
        guard !started else { return }
        let args = CommandLine.arguments
        // `--retouch-selftest <dir>` or `--retouch-selftest=<dir>` (the one-argument form keeps a bare directory path off
        // the command line, which LaunchServices can take as a folder to open instead of the app's window).
        let path: String
        if let a = args.first(where: { $0.hasPrefix("--retouch-selftest=") }) {
            path = String(a.dropFirst("--retouch-selftest=".count))
        } else if let i = args.firstIndex(of: "--retouch-selftest"), i + 1 < args.count {
            path = args[i + 1]
        } else { return }
        started = true
        let dir = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)
        let hold = args.firstIndex(of: "--retouch-selftest-hold").flatMap { $0 + 1 < args.count ? Double(args[$0 + 1]) : nil } ?? 2.5
        // Later: the menu bar is built while the app itself is still being set up.
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) {
            MainActor.assumeIsolated {
                let test = RetouchSelfTest(model: AppModel.shared, dir: dir, hold: hold)
                Task { @MainActor in await test.run() }
            }
        }
    }

    init(model: AppModel, dir: URL, hold: Double) {
        self.model = model
        self.dir = dir
        self.hold = hold
    }

    private func log(_ s: String) {
        if s.hasPrefix("FAIL") { failures += 1 }
        FileHandle.standardError.write(Data("retouch-selftest: \(s)\n".utf8))
    }

    private func check(_ name: String, _ ok: Bool, _ detail: @autoclosure () -> String = "") {
        if !ok { failures += 1 }
        log("check \(name) " + (ok ? "ok" : "FAIL \(detail())"))
    }

    private func pause(_ s: Double) async { try? await Task.sleep(for: .milliseconds(Int(s * 1000))) }

    private func wait(_ timeout: Double, _ condition: () -> Bool) async -> Bool {
        let end = Date().addingTimeInterval(timeout)
        while !condition() {
            if Date() > end { return false }
            await pause(0.02)
        }
        return true
    }

    private func mark(_ name: String, hold h: Double? = nil) async {
        step += 1
        await pause(0.8)
        var frame = ""
        if let key = model.mainWindow, let screen = NSScreen.screens.first {
            let w = key.sheetParent ?? key
            w.level = .floating
            w.orderFrontRegardless()
            await pause(0.3)
            let f = w.frame
            frame = String(format: " window %.0f %.0f %.0f %.0f", f.minX, screen.frame.height - f.maxY, f.width, f.height)
        }
        log("step \(step) \(name)\(frame)")
        await pause(h ?? hold)
    }

    private func event(_ type: NSEvent.EventType, at canvas: CGPoint, in v: DocumentViewportView) -> NSEvent? {
        guard let w = v.window else { return nil }
        let p = v.convert(v.viewPoint(canvas: canvas), to: nil)
        return NSEvent.mouseEvent(with: type, location: p, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
                                  windowNumber: w.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: 1)
    }

    private func drag(_ points: [CGPoint], in v: DocumentViewportView, beforeRelease: (() async -> Void)? = nil) async {
        guard let first = points.first, let last = points.last else { return }
        if let e = event(.leftMouseDown, at: first, in: v) { v.mouseDown(with: e) }
        for p in points.dropFirst() {
            if let e = event(.leftMouseDragged, at: p, in: v) { v.mouseDragged(with: e) }
            await pause(1.0 / 120)
        }
        await beforeRelease?()
        if let e = event(.leftMouseUp, at: last, in: v) { v.mouseUp(with: e) }
    }

    private func finished(_ r: DocumentRetouch, timeout: Double = 600) async -> Result<RetouchOutcome, Error>? {
        var result: Result<RetouchOutcome, Error>?
        r.onFinished = { result = $0 }
        _ = await wait(timeout) { result != nil }
        r.onFinished = nil
        return result
    }

    func run() async {
        let ws = model.documents
        let r = DocumentRetouch.shared
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        guard await wait(60, { !model.isLoading && !model.library.items.isEmpty }) else {
            log("FAIL the library did not load"); return finish()
        }
        let items = model.library.items
        guard let item = items.first(where: { $0.name == "sample.dng" }) ?? items.first(where: { $0.kind == .raw }) else {
            log("FAIL no RAW in the library"); return finish()
        }
        model.select(id: item.id)
        // The viewport lives in the key window; bring it forward (a test aid may take focus).
        NSApp.activate(ignoringOtherApps: true)
        NSApp.windows.first { !($0 is NSPanel) && $0.isVisible }?.makeKeyAndOrderFront(nil)
        ws.editInLayers(model.focusedItem)
        guard await wait(180, { ws.current != nil && ws.opening == nil && ws.current?.viewport != nil }), let doc = ws.current,
              let v = doc.viewport else {
            log("FAIL Edit in Layers opened nothing: \(model.statusMessage ?? "") current \(ws.current != nil) "
                + "viewport \(ws.current?.viewport != nil) mode \(model.viewMode) window \(model.mainWindow != nil)")
            return finish()
        }
        _ = await wait(20) { doc.lastFrame != nil }
        log("document \(doc.info.width) × \(doc.info.height) px, \(doc.info.depth.title), \(doc.info.backend)")
        guard let photo = doc.layers.first(where: { $0.kind == .pixel })?.id else { return finish() }
        doc.select(photo)
        let (W, H) = (Double(doc.info.width), Double(doc.info.height))
        r.refreshModels()
        for m in r.models { log("model \(m.modelId) installed \(m.installed) (\(m.cachePath))") }

        // 1. Remove tool (⇧J) and a stroke.
        r.activate()
        r.options.setSize(Float(max(W, H) / 40))
        r.options.engine = .auto
        check("Remove tool on", r.removeActive && doc.tool == .heal)
        let y = H * 0.62
        let stroke = stride(from: 0.0, through: 1.0, by: 0.02).map { t in CGPoint(x: W * (0.40 + 0.12 * t), y: y + H * 0.02 * sin(t * 6)) }
        let before = doc.history.count
        await drag(stroke, in: v) { await self.mark("remove-stroke") }
        let res = await finished(r)
        switch res {
        case .success(let o)?:
            log(String(format: "remove stroke: backend %@, engine %.0f ms%@", o.backend, o.millis, o.note.map { ", note: \($0)" } ?? ""))
            check("Remove is one node", doc.history.count == before + 1 && doc.history.last?.label == "Remove", "\(doc.history.map(\.label))")
        case .failure(let e)?: check("Remove stroke", false, e.localizedDescription)
        case nil: check("Remove stroke finished", false, "timeout")
        }
        await mark("remove-applied")
        doc.run("Undo") { try doc.backend.undo() }
        check("undo removes it", doc.history.last(where: { $0.id == doc.info.historyHead })?.label != "Remove")
        await mark("remove-undone")
        doc.run("Redo") { try doc.backend.redo() }

        // Save and reopen.
        let file = dir.appendingPathComponent("RetouchSelfTest.tessera-doc")
        try? FileManager.default.removeItem(at: file)
        let saved = ws.write(doc, to: file)
        check("save .tessera-doc", saved, model.statusMessage ?? "")
        let historyLabels = doc.history.map(\.label)
        ws.discard(doc)
        r.deactivate()
        guard saved else { return finish() }
        ws.open(file)
        guard await wait(120, { ws.current != nil && ws.opening == nil }), let doc2 = ws.current else {
            check("reopen", false, model.statusMessage ?? ""); return finish()
        }
        _ = await wait(20) { doc2.lastFrame != nil }
        check("reopened", doc2.layers.contains { $0.kind == .pixel }, "\(historyLabels)")
        await mark("reopened")
        guard let layer = doc2.layers.first(where: { $0.kind == .pixel })?.id, let tools = doc2.backend as? any DocumentToolsBackend else {
            return finish()
        }
        doc2.select(layer)

        // 2. Edit ▸ Content-Aware Fill on a marquee.
        doc2.run("Marquee") {
            try tools.selectMarquee(.rect, rect: CGRect(x: W * 0.3, y: H * 0.3, width: W * 0.05, height: H * 0.05), feather: 0,
                                    antialias: true, op: .replace)
        }
        DocumentTools.shared.refreshOutline(doc2)
        let n2 = doc2.history.count
        r.contentAwareFill()
        switch await finished(r) {
        case .success(let o)?:
            log(String(format: "content-aware fill: %.0f ms", o.millis))
            check("Content-Aware Fill is one node", doc2.history.count == n2 + 1 && doc2.history.last?.label == "Content-Aware Fill",
                  "\(doc2.history.map(\.label))")
        case .failure(let e)?: check("Content-Aware Fill", false, e.localizedDescription)
        case nil: check("Content-Aware Fill finished", false, "timeout")
        }
        await mark("content-aware-fill")

        // 3. A slow job, cancelled.
        doc2.run("Marquee") {
            try tools.selectMarquee(.rect, rect: CGRect(x: W * 0.1, y: H * 0.1, width: W * 0.8, height: H * 0.8), feather: 0,
                                    antialias: true, op: .replace)
        }
        r.activate()
        r.options.engine = .patchMatch
        let n3 = doc2.history.count
        var cancelled: Result<RetouchOutcome, Error>?
        r.onFinished = { cancelled = $0 }
        r.removeSelection()
        await pause(1.5)
        check("slow job running", r.busy != nil)
        await mark("slow-job-running", hold: 0.5)
        r.cancel()
        _ = await wait(300) { cancelled != nil }
        r.onFinished = nil
        if case .failure(let e)? = cancelled {
            check("cancel leaves history unchanged", e.localizedDescription.contains("cancelled") && doc2.history.count == n3,
                  "\(e.localizedDescription) \(doc2.history.map(\.label))")
        } else {
            check("cancelled", false, "\(String(describing: cancelled))")
        }
        await mark("slow-job-cancelled")
        doc2.run("Deselect") { try doc2.backend.clearSelection() }
        DocumentTools.shared.refreshOutline(doc2)

        // 4. Remove Distractions: review, keep one, remove the rest.
        r.options.engine = .auto
        r.scanDistractions()
        _ = await wait(300) { r.busy == nil }
        if let review = r.review {
            log("distractions: \(review.candidates.count) suggestions, faces: \(review.scan.faces)")
            if let first = review.candidates.first { r.toggleSuggestion(first.id) }
            await mark("distractions-review")
            if r.review?.canApply == true {
                let n4 = doc2.history.count
                r.applyReview()
                if case .success? = await finished(r) {
                    check("Remove Distractions is one node", doc2.history.count == n4 + 1, "\(doc2.history.map(\.label))")
                }
                await mark("distractions-removed")
            } else {
                r.endReview()
            }
        } else {
            check("distraction scan", false, model.statusMessage ?? "")
        }
        r.deactivate()

        // 5. Neural Filters.
        r.openNeuralFilters()
        guard let sheet = r.neuralSheet else { check("Neural Filters sheet", false); return finish() }
        for k in [NeuralKind.colorize, .jpegArtifactRemoval] {
            sheet.choose(k)
            check("\(k.rawValue) names its missing model", sheet.missingModel != nil || r.models.contains { $0.installed })
            await mark("neural-\(k.rawValue)")
        }
        sheet.choose(.skinSmoothing)
        await mark("neural-skin-no-faces")
        sheet.cancel()
        await pause(0.5)
        // Skin Smoothing with a face-sized selection, to a new layer.
        doc2.run("Marquee") {
            try tools.selectMarquee(.ellipse, rect: CGRect(x: W * 0.45, y: H * 0.3, width: W * 0.1, height: H * 0.14), feather: 0,
                                    antialias: true, op: .replace)
        }
        DocumentTools.shared.refreshOutline(doc2)
        r.openNeuralFilters()
        if let s = r.neuralSheet {
            s.choose(.skinSmoothing)
            s.state.output = .newLayer
            let n5 = doc2.history.count
            let layers = doc2.layers.count
            s.apply()
            _ = await wait(600) { r.neuralSheet == nil || s.error != nil }
            check("Skin Smoothing to a new layer", doc2.history.count == n5 + 1 && doc2.layers.count == layers + 1,
                  s.error?.detail ?? "\(doc2.history.map(\.label))")
            await mark("neural-skin-new-layer")
        }
        finish()
    }

    private func finish() {
        log("done, \(failures) failure(s)")
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) { NSApp.terminate(nil) }
    }
}
