import AppKit
import TesseraCore
import TesseraFFI

/// `--retouch-selftest <dir>` (test aid, WP B5-09; B5-09b: background-only, add `--new-document` with `open -g`
/// so it starts without the menu bar, and the download / cancel / menu steps): after the library loads, Edit in Layers on `sample.dng`,
/// then through the same paths the UI takes: the Remove tool (⇧J) and a synthesized stroke on the viewport
/// (one "Remove" node, timed), undo / redo, save and reopen the `.tessera-doc`; Edit ▸ Content-Aware Fill on a
/// marquee; a slow Remove on a large selection cancelled from the options bar (history unchanged); Remove
/// Distractions review (one suggestion kept); Filter ▸ Neural Filters… for each filter (missing-weight
/// messages, Skin Smoothing to a new layer); B5-17a: Photo Restoration listed with its limitation, and without
/// DRUNet every output fails and changes nothing, on pixels and on a smart object. Each step prints `retouch-selftest: step <n> <name> window
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
        // B5-09b: never raise, float or activate the window (the Mac is in use): the step names the window
        // number for `screencapture -x -o -l <id>` of this window only (sheets are separate windows).
        var frame = ""
        if let key = window {
            let w = key.sheetParent ?? key
            frame = " window \(w.windowNumber)" + (w.attachedSheet.map { " sheet \($0.windowNumber)" } ?? "")
        }
        log("step \(step) \(name)\(frame)")
        await pause(h ?? hold)
    }

    /// The app's document window (key or not: the app may be in the background).
    private var window: NSWindow? {
        model.mainWindow ?? NSApp.windows.first { !($0 is NSPanel) && $0.isVisible && $0.contentView != nil }
    }

    /// A menu bar item as AppKit has it after the menu is brought up to date, the way opening it would
    /// (`menuNeedsUpdate`, then validation).
    private func menuItem(_ menu: String, _ title: String) -> (NSMenu, Int)? {
        guard let m = NSApp.mainMenu?.items.first(where: { $0.title == menu })?.submenu else { return nil }
        m.delegate?.menuNeedsUpdate?(m)
        m.update()
        guard let i = m.items.firstIndex(where: { $0.title == title }) else { return nil }
        return (m, i)
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
        // A background launch starts the test from the document view (`--new-document`): close that blank one.
        while let blank = ws.current { ws.discard(blank) }
        let items = model.library.items
        guard let item = items.first(where: { $0.name == "sample.dng" }) ?? items.first(where: { $0.kind == .raw }) else {
            log("FAIL no RAW in the library"); return finish()
        }
        model.select(id: item.id)
        // B5-09b: runs in the background (`open -g`): no activation, no key window, nothing raised.
        ws.editInLayers(model.focusedItem)
        guard await wait(180, { ws.current != nil && ws.opening == nil && ws.current?.viewport != nil }), let doc = ws.current,
              let v = doc.viewport else {
            log("FAIL Edit in Layers opened nothing: \(model.statusMessage ?? "") current \(ws.current != nil) "
                + "viewport \(ws.current?.viewport != nil) mode \(model.viewMode) window \(model.mainWindow != nil)")
            return finish()
        }
        _ = await wait(20) { doc.lastFrame != nil }
        log("document \(doc.info.width) × \(doc.info.height) px, \(doc.info.depth.title), \(doc.info.backend)")
        guard let photo = doc.layers.first(where: { $0.kind == .pixel })?.id else { log("FAIL no pixel layer"); return finish() }
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
        guard let layer = doc2.layers.first(where: { $0.kind == .pixel })?.id, let tools = doc2.backend as? any DocumentToolsBackend,
              let v2 = doc2.viewport else {
            log("FAIL reopened document has no pixel layer / viewport"); return finish()
        }
        doc2.select(layer)

        // 2. Edit ▸ Content-Aware Fill on a marquee drawn with the Rectangular Marquee tool, invoked from the
        // menu bar item itself (B5-09b: it was disabled with an active marquee).
        doc2.run("Deselect") { try doc2.backend.clearSelection() }
        DocumentTools.shared.select(.marquee)
        await pause(0.3)
        let noSel = menuItem("Edit", "Content-Aware Fill")
        log("menu Edit ▸ Content-Aware Fill without a selection: \(noSel.map { $0.0.items[$0.1].isEnabled ? "enabled" : "disabled" } ?? "missing")")
        check("Content-Aware Fill disabled without a selection", noSel.map { !$0.0.items[$0.1].isEnabled } ?? false)
        let x0 = W * 0.3, y0 = H * 0.3
        await drag(stride(from: 0.0, through: 1.0, by: 0.1).map { t in CGPoint(x: x0 + W * 0.05 * t, y: y0 + H * 0.05 * t) }, in: v2)
        _ = await wait(5) { doc2.marquee != nil }
        await pause(0.3)
        if let m = NSApp.mainMenu?.items.first(where: { $0.title == "Edit" })?.submenu,
           let item = m.items.first(where: { $0.title == "Content-Aware Fill" }) {
            log("menu Edit ▸ Content-Aware Fill as SwiftUI left it (before the menu is opened): \(item.isEnabled ? "enabled" : "disabled")")
        }
        let withSel = menuItem("Edit", "Content-Aware Fill")
        log("menu Edit ▸ Content-Aware Fill with a marquee \(doc2.marquee.map { "\($0)" } ?? "none"): "
            + "\(withSel.map { $0.0.items[$0.1].isEnabled ? "enabled" : "disabled" } ?? "missing")")
        check("Content-Aware Fill enabled with a marquee", withSel.map { $0.0.items[$0.1].isEnabled } ?? false)
        let n2 = doc2.history.count
        if let (menu, i) = withSel, menu.items[i].isEnabled {
            menu.performActionForItem(at: i)
        } else {
            r.contentAwareFill()
        }
        switch await finished(r, timeout: 120) {
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
        var late: Result<RetouchOutcome, Error>?
        r.onDiscarded = { late = $0 }
        r.removeSelection()
        await pause(1.5)
        check("slow job running", r.busy != nil)
        if let (menu, i) = menuItem("Edit", "Content-Aware Fill") {
            // While an apply runs the item is disabled; the old Cancel kept this state until the engine returned.
            check("Content-Aware Fill disabled while a Remove runs", !menu.items[i].isEnabled)
        }
        await mark("slow-job-running", hold: 0.5)
        // B5-09b: Cancel returns the bar to idle at once, whatever the engine does.
        let t0 = Date()
        r.cancel()
        let back = Date().timeIntervalSince(t0)
        check("Cancel returns the bar to idle at once", r.busy == nil && back < 0.25, String(format: "%.3f s", back))
        log(String(format: "cancel returned in %.1f ms; engine still running: %@", back * 1000, r.jobs.abandoned != nil ? "yes" : "no"))
        await mark("slow-job-cancelled", hold: 0.5)
        if r.jobs.abandoned != nil, let (menu, i) = menuItem("Edit", "Content-Aware Fill") {
            // The old Cancel kept the job busy until the engine returned, and busy disabled this item.
            check("Content-Aware Fill enabled again right after Cancel", menu.items[i].isEnabled)
        }
        if r.jobs.abandoned != nil {
            // A new job while the cancelled one is still stopping: refused with a clear message.
            r.removeSelection()
            check("a new job waits for the cancelled one", r.busy == nil && (r.notice?.contains("still stopping") ?? false), r.notice ?? "no notice")
            await mark("slow-job-refused", hold: 0.5)
        }
        _ = await wait(600) { late != nil }
        r.onDiscarded = nil
        log(String(format: "the cancelled job returned after %.1f s: %@", Date().timeIntervalSince(t0),
                   late.map { if case .success = $0 { "a result (reverted)" } else { "cancelled" } } ?? "never"))
        check("the late result is discarded, history unchanged", late != nil && doc2.history.count == n3 && r.jobs.abandoned == nil,
              "\(doc2.history.map(\.label))")
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
        for k in [NeuralKind.colorize, .jpegArtifactRemoval, .photoRestoration] {
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
        await downloads(doc2, v2)
        await restoration(doc2, layer: layer)
        finish()
    }

    /// 7. Photo Restoration (B5-17a, M5-32): listed with its one control and the engine's limitation caption;
    /// without DRUNet every output fails with the missing-weights message and changes nothing, on the pixel
    /// layer and after it becomes a smart object. With DRUNet installed the atomicity checks are skipped.
    private func restoration(_ doc: DocumentController, layer: DocLayerID) async {
        let r = DocumentRetouch.shared
        doc.run("Deselect") { try doc.backend.clearSelection() }
        doc.select(layer)
        r.deactivate()
        r.refreshModels()
        r.openNeuralFilters()
        guard let sheet = r.neuralSheet else { return check("Neural Filters sheet for restoration", false) }
        sheet.choose(.photoRestoration)
        let spec = sheet.state.spec
        check("Photo Restoration is listed", spec?.name == "Photo Restoration", "\(sheet.state.specs.map(\.name))")
        check("Photo Restoration has one Photo enhancement control",
              spec?.controls.map(\.key) == ["photo_enhancement"], "\(spec?.controls.map(\.key) ?? [])")
        check("Photo Restoration shows its limitation", spec?.limitation?.hasPrefix("Denoise only") == true,
              spec?.limitation ?? "nil")
        check("Photo Restoration needs DRUNet", sheet.requiredModelId == "enhance/drunet-color")
        await mark("neural-photo-restoration")
        sheet.cancel()
        await pause(0.3)
        guard r.model("enhance/drunet-color")?.installed == false else {
            return log("DRUNet is installed here: restoration missing-weights checks skipped")
        }
        guard let b = DocumentRetouch.backend(doc), let filters = doc.backend as? any DocumentFiltersBackend else {
            return check("restoration backend", false)
        }
        func atomic(_ outputs: [NeuralOutput], _ what: String) {
            for o in outputs {
                let n = doc.history.count, layers = doc.layers.count
                let smart = (try? filters.smartFilters(layer: layer).count) ?? -1
                do {
                    _ = try b.neuralFilter(layer: layer, kind: .photoRestoration, paramsJson: #"{"photo_enhancement":0.5}"#,
                                           output: o)
                    check("\(what) \(o.rawValue): missing DRUNet fails", false, "it applied")
                } catch {
                    let msg = "\(error)"
                    check("\(what) \(o.rawValue): missing DRUNet is named",
                          msg.contains("enhance/drunet-color") && msg.contains("weights"), msg)
                }
                check("\(what) \(o.rawValue): nothing changed",
                      doc.history.count == n && doc.layers.count == layers
                          && ((try? filters.smartFilters(layer: layer).count) ?? -1) == smart,
                      "\(doc.history.map(\.label))")
            }
        }
        atomic(NeuralOutput.allCases, "restoration on pixels")
        guard doc.run("Convert for Smart Filters", { try filters.convertForSmartFilters(layer: layer) }) != nil else {
            return check("convert for smart filters", false, model.statusMessage ?? "")
        }
        atomic([.currentLayer, .smartFilter], "restoration on a smart object")
    }

    /// 6. Model downloads (B5-09b) through a local stand-in for the engine's `ModelDownloads`: it reports
    /// queued, bytes and ready over about two seconds and writes nothing (no real weights are fetched), and a
    /// scratch preference suite stands in for Settings ▸ AI. So the checks are the flow: downloads off → nothing
    /// requested and the reason shown; allowed → inline progress, then the waiting Remove / Colorize runs by
    /// itself (and, with no weights actually written, reports the engine's missing-model error).
    private func downloads(_ doc: DocumentController, _ v: DocumentViewportView) async {
        let r = DocumentRetouch.shared
        let suite = "dev.tessera.retouch-selftest"
        let defaults = UserDefaults(suiteName: suite) ?? .standard
        defaults.removePersistentDomain(forName: suite)
        defer { defaults.removePersistentDomain(forName: suite) }
        let fake = SelfTestDownloads()
        let acquisition = ModelAcquisition(defaults: defaults) { _ in fake }
        let real = r.downloads
        r.downloads = RetouchModelDownloads(acquisition: acquisition)
        defer { r.downloads = real }
        doc.run("Deselect") { try doc.backend.clearSelection() }
        r.activate()
        r.refreshModels()
        guard r.model("remove/lama")?.installed == false else { log("LaMa is installed here: download steps skipped"); return }
        r.options.engine = .lama
        let (W, H) = (Double(doc.info.width), Double(doc.info.height))
        let stroke = stride(from: 0.0, through: 1.0, by: 0.05).map { t in CGPoint(x: W * (0.2 + 0.08 * t), y: H * 0.8) }

        // Downloads off: nothing requested, the bar says so and links to Settings ▸ AI; no history.
        acquisition.allowDownloads = false
        let n0 = doc.history.count
        await drag(stroke, in: v)
        await pause(0.3)
        check("downloads off: nothing requested", fake.requests == 0)
        check("downloads off: the reason is shown", r.notice?.contains("model downloads are off") ?? false, r.notice ?? "no notice")
        check("downloads off: stroke dropped, history unchanged", !r.strokeWaitingForModel && doc.history.count == n0)
        await mark("download-off")

        // Allowed: the stroke waits while LaMa downloads (inline progress), then Remove runs by itself.
        acquisition.allowDownloads = true
        r.clearNotice()
        var result: Result<RetouchOutcome, Error>?
        r.onFinished = { result = $0 }
        await drag(stroke, in: v)
        check("allowed: LaMa requested once", fake.requests == 1, "\(fake.requests)")
        check("allowed: Remove waits for the download", r.strokeWaitingForModel && r.downloads.waiting["remove/lama"] == "Remove")
        _ = await wait(5) { if case .downloading(_, let s) = r.downloads.phase(r.model("remove/lama")), s.fraction != nil { true } else { false } }
        await mark("download-progress", hold: 0.3)
        _ = await wait(60) { result != nil }
        r.onFinished = nil
        switch result {
        case .failure(let e)?:
            // The stand-in wrote no weights: the engine's own lookup still reports LaMa missing (the real file
            // lands in the same cache; see the Rust test `retouch_models_are_looked_up_where_model_downloads_put_them`).
            log("remove after the download: \(e.localizedDescription)")
            check("allowed: Remove ran after the download", e.localizedDescription.contains("remove/lama"), e.localizedDescription)
        case .success(let o)?:
            check("allowed: Remove ran after the download", o.backend == "LaMa", o.backend)
        case nil:
            check("allowed: Remove ran after the download", false, "it never ran")
        }
        await mark("download-then-remove")
        r.options.engine = .auto
        r.deactivate()

        // Neural Filters ▸ Colorize: Download and Apply, progress in the sheet, then the apply runs.
        r.openNeuralFilters()
        if let sheet = r.neuralSheet {
            sheet.choose(.colorize)
            sheet.apply()
            check("Colorize waits for DDColor", sheet.waitingForModel && fake.requests == 2, "\(fake.requests)")
            _ = await wait(5) { r.downloads.phase(r.model("filters/ddcolor")).isDownloading }
            await mark("download-neural-progress", hold: 0.3)
            _ = await wait(60) { !sheet.waitingForModel && !sheet.busy && (sheet.error != nil || r.neuralSheet == nil) }
            check("Colorize ran after the download", sheet.error?.detail.contains("filters/ddcolor") ?? (r.neuralSheet == nil),
                  sheet.error?.detail ?? "")
            await mark("download-neural-applied")
            sheet.cancel()
        }
    }

    private func finish() {
        log("done, \(failures) failure(s)")
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) { NSApp.terminate(nil) }
    }
}

/// The self-test's stand-in for `ModelDownloads` (B5-09b): queued, ten progress events, ready; writes nothing.
private final class SelfTestDownloads: ModelDownloadRequesting, @unchecked Sendable {
    private let lock = NSLock()
    private var count = 0
    var requests: Int { lock.withLock { count } }

    func request(id: String, version: String, listener: ModelDownloadListener) throws {
        lock.withLock { count += 1 }
        DispatchQueue.global(qos: .utility).async {
            listener.onEvent(event: .queued)
            let total: UInt64 = 208_000_000
            for i in 1...10 {
                Thread.sleep(forTimeInterval: 0.2)
                listener.onEvent(event: .downloading(bytes: total / 10 * UInt64(i), total: total))
            }
            listener.onEvent(event: .ready(path: "(self-test: nothing written)"))
        }
    }
}
