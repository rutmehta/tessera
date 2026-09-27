import AppKit
import TesseraCore

/// `--filter-selftest <dir>` (test aid, WP B5-05): ACCEPTANCE §U part 3 through the same controller
/// calls the menus and dialogs make. After the library loads it opens `sample.dng` with Edit in
/// Layers, opens Filter ▸ Blur ▸ Gaussian Blur…, drags Radius through 12 values measuring the preview
/// latency (value change → the listener's frame showing it), applies (one "Gaussian Blur" row),
/// undoes, runs Image ▸ Adjustments ▸ Levels…, converts the layer for smart filters, applies Gaussian
/// Blur as a smart filter, toggles it off and on, and saves `<dir>/FilterSelfTest.tessera-doc`. Each
/// step prints `filter-selftest: step <n> <name> window <x> <y> <w> <h>` (for `screencapture -R`) and
/// holds `hold` seconds; checks print `check <name> ok|FAIL`. Quits at the end.
/// Started by `DocumentFilters` when the argument is present (no hook in TesseraApp).
@MainActor
final class FilterSelfTest {
    fileprivate let model: AppModel
    fileprivate let dir: URL
    private let hold: Double
    private var failures = 0
    private var step = 0

    static func startIfRequested() {
        let args = CommandLine.arguments
        guard let i = args.firstIndex(of: "--filter-selftest"), i + 1 < args.count else { return }
        let dir = URL(fileURLWithPath: (args[i + 1] as NSString).expandingTildeInPath)
        let hold = args.firstIndex(of: "--filter-selftest-hold").flatMap { $0 + 1 < args.count ? Double(args[$0 + 1]) : nil } ?? 2.5
        DispatchQueue.main.async {
            MainActor.assumeIsolated {
                let test = FilterSelfTest(model: AppModel.shared, dir: dir, hold: hold)
                Task { @MainActor in await test.run() }
            }
        }
    }

    init(model: AppModel, dir: URL, hold: Double) {
        self.model = model
        self.dir = dir
        self.hold = hold
    }

    fileprivate func log(_ s: String) { FileHandle.standardError.write(Data("filter-selftest: \(s)\n".utf8)) }

    fileprivate func check(_ name: String, _ ok: Bool, _ detail: @autoclosure () -> String = "") {
        if !ok { failures += 1 }
        log("check \(name) " + (ok ? "ok" : "FAIL \(detail())"))
    }

    fileprivate func pause(_ s: Double) async { try? await Task.sleep(for: .milliseconds(Int(s * 1000))) }

    fileprivate func wait(_ timeout: Double, _ condition: () -> Bool) async -> Bool {
        let end = Date().addingTimeInterval(timeout)
        while !condition() {
            if Date() > end { return false }
            await pause(0.02)
        }
        return true
    }

    private func mark(_ name: String) async {
        step += 1
        await pause(0.8)
        var frame = ""
        // A dialog is a sheet: report its parent window (the canvas preview shows around it).
        if let key = model.mainWindow, let screen = NSScreen.screens.first {
            let w = key.sheetParent ?? key
            w.level = .floating
            w.orderFrontRegardless()
            await pause(0.3)
            let f = w.frame
            frame = String(format: " window %.0f %.0f %.0f %.0f", f.minX, screen.frame.height - f.maxY, f.width, f.height)
        }
        log("step \(step) \(name)\(frame)")
        await pause(hold)
    }

    private func stats(_ v: [Double]) -> String {
        guard !v.isEmpty else { return "no samples" }
        let s = v.sorted()
        let q = { (p: Double) in s[min(s.count - 1, Int((Double(s.count - 1) * p).rounded()))] }
        return String(format: "n %d, median %.1f ms, p90 %.1f ms, max %.1f ms", s.count, q(0.5), q(0.9), s.last!)
    }

    /// Waits for the listener's next frame after `action`; milliseconds, or nil on timeout.
    fileprivate func frameAfter(_ doc: DocumentController, _ action: () -> Void) async -> Double? {
        var got: Date?
        let start = Date()
        doc.frameObserver = { _ in if got == nil { got = Date() } }
        action()
        let ok = await wait(10) { got != nil }
        doc.frameObserver = nil
        return ok ? got.map { $0.timeIntervalSince(start) * 1000 } : nil
    }

    fileprivate func idle(_ filters: DocumentFilters) async -> Bool { await wait(120) { filters.busy == nil } }

    func run() async {
        let ws = model.documents
        let filters = ws.filters
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        guard await wait(60, { !model.isLoading && !model.library.items.isEmpty }) else {
            log("FAIL the library did not load"); return finish()
        }
        let items = model.library.items
        guard let item = items.first(where: { $0.name == "sample.dng" }) ?? items.first(where: { $0.kind == .raw }) else {
            log("FAIL no RAW in the library"); return finish()
        }
        model.select(id: item.id)
        let restore = model.showRenderReadout
        model.showRenderReadout = true
        defer { model.showRenderReadout = restore }

        ws.editInLayers(model.focusedItem)
        guard await wait(120, { ws.current != nil && ws.opening == nil }), let doc = ws.current else {
            log("FAIL Edit in Layers opened nothing: \(model.statusMessage ?? "")"); return finish()
        }
        _ = await wait(10) { doc.lastFrame != nil }
        log("document \(doc.info.width) × \(doc.info.height) px, \(doc.info.depth.title), \(doc.info.backend)")
        guard let photo = doc.layers.first(where: { $0.kind == .pixel })?.id else { return finish() }
        doc.select(photo)
        let catalogue = filters.catalogue(doc)
        check("filter menu", FilterCatalogEntry.grouped(catalogue).map(\.group) == FilterCatalogEntry.groupOrder,
              "\(catalogue.map(\.group))")
        guard let gaussian = catalogue.first(where: { $0.id == "gaussian_blur" }) else { return finish() }
        if perfMode { await perf(doc, photo: photo, gaussian: gaussian); return finish() }   // B5-15

        // 1. Gaussian Blur dialog: preview latency over a radius drag.
        filters.open(gaussian, doc)
        guard let sheet = filters.filterSheet else { check("dialog opens", false); return finish() }
        _ = await wait(10) { sheet.detail != nil }
        let rows = doc.history.count
        var latency: [Double] = []
        var levels = Set<UInt8>()
        for r in [3.0, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14] {
            if let ms = await frameAfter(doc, { sheet.set(gaussian.params[0], .number(r), final: false) }) {
                latency.append(ms)
                if let l = doc.lastFrame?.level { levels.insert(l) }
            }
            await pause(0.05)
        }
        sheet.set(gaussian.params[0], .number(12))
        let region = doc.lastFrame.map { "\($0.canvasRect.width) × \($0.canvasRect.height) at L\($0.level)" } ?? "?"
        log("gaussian preview latency (value → frame, viewport \(region)): " + stats(latency))
        check("preview frames", latency.count == 12, "\(latency.count)")
        check("preview records no history", doc.history.count == rows, "\(doc.history.map(\.label))")
        check("detail pane", sheet.detail != nil, sheet.error ?? "")
        await mark("gaussian-dialog")

        let t0 = Date()
        sheet.ok()
        _ = await idle(filters)
        log(String(format: "gaussian apply at full resolution: %.2f s", Date().timeIntervalSince(t0)))
        check("apply is one history row", doc.history.count == rows + 1 && doc.history.last?.label == "Gaussian Blur",
              "\(doc.history.map(\.label))")
        await mark("gaussian-applied")
        doc.undo()
        check("undo", doc.history.last(where: { $0.isCurrent })?.label != "Gaussian Blur", "\(doc.history.map(\.label))")
        await mark("gaussian-undone")

        // 2. Image ▸ Adjustments ▸ Levels….
        filters.openAdjustment(.levels, doc)
        guard let adj = filters.adjustmentSheet else { check("levels dialog", false); return finish() }
        var levelsModel = LevelsChannelModel()
        levelsModel.inBlack = 0.12
        levelsModel.gamma = 1.35
        levelsModel.inWhite = 0.9
        adj.edit(.levels(master: levelsModel, rgb: [.init(), .init(), .init()]), final: true)
        await mark("levels-dialog")
        let before = doc.history.filter { $0.label == "Levels" }.count
        adj.ok(filters)
        _ = await idle(filters)
        check("levels applied", doc.history.filter { $0.label == "Levels" }.count == before + 1, "\(doc.history.map(\.label))")
        await mark("levels-applied")

        // 3. Smart filter: convert, apply Gaussian Blur, toggle off and on.
        filters.convertForSmartFilters(doc)
        check("smart object", doc.node(photo)?.kind == .smartObject, "\(doc.node(photo)?.kind.title ?? "none")")
        filters.open(gaussian, doc)
        guard let smartSheet = filters.filterSheet else { check("smart dialog", false); return finish() }
        smartSheet.set(gaussian.params[0], .number(10))
        _ = await wait(10) { smartSheet.detail != nil }
        smartSheet.ok()
        _ = await idle(filters)
        let list = filters.smartFilters(doc, layer: photo)
        check("smart filter listed", list.map(\.filterId) == ["gaussian_blur"], "\(list.map(\.filterId))")
        _ = await wait(15) { doc.lastFrame != nil }
        await mark("smart-filter-on")
        if let row = list.first {
            let off = await frameAfter(doc) { filters.toggleSmartFilter(doc, layer: photo, row: row) }
            log(String(format: "smart filter off → frame %.0f ms", off ?? -1))
            check("smart filter off", filters.smartFilters(doc, layer: photo).first?.enabled == false)
            await mark("smart-filter-off")
            let on = await frameAfter(doc) {
                if let r = filters.smartFilters(doc, layer: photo).first { filters.toggleSmartFilter(doc, layer: photo, row: r) }
            }
            // The bake lands in a later frame; wait for the worker to finish.
            await pause(1.5)
            log(String(format: "smart filter on → first frame %.0f ms", on ?? -1))
            check("smart filter on", filters.smartFilters(doc, layer: photo).first?.enabled == true)
            await mark("smart-filter-on-again")
        }
        let labels = doc.history.suffix(4).map(\.label)
        check("history rows", labels == ["Convert to Smart Object", "Gaussian Blur", "Disable Smart Filter", "Enable Smart Filter"],
              "\(labels)")
        let path = dir.appendingPathComponent("FilterSelfTest.tessera-doc")
        try? FileManager.default.removeItem(at: path)
        check("save", ws.write(doc, to: path), model.statusMessage ?? "")
        finish()
    }

    fileprivate func finish() {
        log("done, \(failures) failure(s)")
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) { NSApp.terminate(nil) }
    }
}

// MARK: - B5-15 perf scenarios (P16, P19, memory)

/// `TESSERA_FILTER_PERF=1` with `--filter-selftest <dir>` (WP B5-15): instead of the acceptance steps,
/// (1) converts the developed photo for smart filters, applies Gaussian Blur 8 px as a smart filter and drags its
/// radius in the smart-filter dialog (value → the listener's next frame, 30 display-rate ticks) at fit and at 100 %
/// in a 3840 × 2160 device-pixel viewport, with the process footprint around each drag; (2) adds a text layer with
/// a drop shadow and an outer glow and runs Export Flat (PNG, sRGB) while a run-loop observer records every
/// main-thread busy span; (3) cancels an export at 30 % and checks the destination. Lines start with
/// `filter-perf:`. The window is never raised, made key or floated.
extension FilterSelfTest {
    var perfMode: Bool { ProcessInfo.processInfo.environment["TESSERA_FILTER_PERF"] != nil }

    private func plog(_ s: String) { FileHandle.standardError.write(Data("filter-perf: \(s)\n".utf8)) }

    private static func pct(_ v: [Double], _ p: Double) -> Double {
        guard !v.isEmpty else { return .nan }
        let s = v.sorted()
        return s[min(s.count - 1, Int((Double(s.count - 1) * p).rounded()))]
    }

    private func report(_ name: String, _ v: [Double]) {
        plog(String(format: "RESULT %@: n %d p50 %.2f ms p95 %.2f ms max %.2f ms", name, v.count, Self.pct(v, 0.5),
                    Self.pct(v, 0.95), v.max() ?? .nan))
    }

    /// Physical footprint of this process (MiB).
    private func footprintMiB() -> Double {
        var info = task_vm_info_data_t()
        var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<natural_t>.size)
        let kr = withUnsafeMutablePointer(to: &info) {
            $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) { task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count) }
        }
        return kr == KERN_SUCCESS ? Double(info.phys_footprint) / 1_048_576 : .nan
    }

    func perf(_ doc: DocumentController, photo: DocLayerID, gaussian: FilterCatalogEntry) async {
        let ws = model.documents
        let filters = ws.filters
        plog("start: document \(doc.info.width) × \(doc.info.height) \(doc.info.depth.title), backend \(doc.info.backend), footprint \(String(format: "%.0f", footprintMiB())) MiB")

        // P19: Gaussian Blur smart filter on the converted photo.
        filters.convertForSmartFilters(doc)
        filters.open(gaussian, doc)
        guard let add = filters.filterSheet else { plog("FAIL no dialog"); return }
        add.set(gaussian.params[0], .number(8))
        add.ok()
        _ = await idle(filters)
        await pause(3)
        guard let row = filters.smartFilters(doc, layer: photo).first else { plog("FAIL no smart filter"); return }
        plog(String(format: "smart filter applied, footprint %.0f MiB", footprintMiB()))
        // A 3840 × 2160 device-pixel viewport for the 100 % pass (the window stays where it is, in the back).
        if let w = model.mainWindow, let v = doc.viewport {
            let scale = w.backingScaleFactor
            let extraW = w.frame.width - v.bounds.width, extraH = w.frame.height - v.bounds.height
            w.setFrame(NSRect(origin: w.frame.origin, size: NSSize(width: 3840 / scale + extraW, height: 2160 / scale + extraH)),
                       display: true)
            await pause(1)
            plog(String(format: "viewport %.0f × %.0f pt @%.0fx", v.bounds.width, v.bounds.height, scale))
        }
        for (label, actual) in [("fit", false), ("100% 4K", true)] {
            if let view = doc.viewport { if actual { view.zoomActual() } else { view.zoomToFit() } }
            _ = await wait(20) { (doc.lastFrame?.level == 0) == actual }
            await pause(3)
            let level = doc.lastFrame?.level ?? 255
            filters.editSmartFilter(doc, layer: photo, row: row)
            guard let sheet = filters.filterSheet else { plog("FAIL no smart filter dialog"); return }
            await pause(1)
            let before = footprintMiB()
            var peak = before
            var ticks: [Double] = []
            for i in 0..<32 {
                let r = 6.0 + Double(i % 9)
                if let ms = await frameAfter(doc, { sheet.set(gaussian.params[0], .number(r), final: false) }), i >= 2 {
                    ticks.append(ms)
                }
                peak = max(peak, footprintMiB())
                await pause(1.0 / 60)
            }
            sheet.cancel()
            await pause(2)
            report("p19 \(label) (L\(level)) smart filter drag value→frame", ticks)
            plog(String(format: "RESULT p19 %@ footprint before %.0f MiB, peak %.0f MiB, 2 s after %.0f MiB", label, before, peak,
                        footprintMiB()))
        }
        if let view = doc.viewport { view.zoomToFit() }
        await pause(2)

        // P16: a styled layer (text with a drop shadow and an outer glow), then Export Flat.
        if let text = doc.backend as? DocumentTextBackend,
           let styles = doc.backend as? DocumentStylesBackend,
           let textModel = try? JSONDecoder().decode(TextSourceModel.self,
                                                 from: Data(#"{"runs":[{"text":"Tessera export","family":"Helvetica","size":220}]}"#.utf8)),
           let c = try? text.addTextLayer(name: "caption", parent: nil, index: nil, model: textModel,
                                          transform: .translation(600, 2600), interactive: false),
           let id = c.created.first {
            _ = try? styles.setLayerStylesJson(layer: id, json: #"{"effects":[{"kind":"drop_shadow","settings":{"distance":30,"size":40}},{"kind":"outer_glow","settings":{"size":30}}],"scale":1}"#,
                                               interactive: false)
            doc.reloadModel()
            plog("styled text layer \(id) added")
        } else {
            plog("FAIL could not add the styled layer")
        }
        await pause(3)
        let settings = ExportFlatSettings(format: .png, quality: 90, color: .srgb)
        let spans = MainThreadSpans()
        for i in 0..<2 {
            let url = dir.appendingPathComponent("perf-flat-\(i).png")
            try? FileManager.default.removeItem(at: url)
            spans.start()
            let t = Date()
            let ok = await exportUnderTest(ws, doc, settings, url)
            let secs = Date().timeIntervalSince(t)
            let v = spans.stop()
            plog(String(format: "export %d: %@ in %.2f s, %d main-thread spans", i, ok ? "ok" : "FAILED", secs, v.count))
            report("p16 main-thread spans during Export Flat \(i)", v)
            check("export \(i)", ok && FileManager.default.fileExists(atPath: url.path))
        }
        await cancelUnderTest(ws, doc, settings)
        plog(String(format: "done, footprint %.0f MiB", footprintMiB()))
    }
}

// B5-15 VARIANT begin (the baseline build calls `ws.exportFlat(doc, s, to: url)` synchronously here)
extension FilterSelfTest {
    /// Export Flat as the menu command runs it.
    fileprivate func exportUnderTest(_ ws: DocumentWorkspace, _ doc: DocumentController, _ s: ExportFlatSettings,
                                     _ url: URL) async -> Bool {
        var outcome: FlatExportTask.Outcome?
        guard ws.startExportFlat(doc, s, to: url, then: { outcome = $0 }) != nil else { return false }
        _ = await wait(900) { outcome != nil }
        return outcome == .exported
    }

    /// Cancel at 30 %: the file already at the destination is kept byte for byte, no temporary file is left.
    fileprivate func cancelUnderTest(_ ws: DocumentWorkspace, _ doc: DocumentController, _ s: ExportFlatSettings) async {
        let folder = dir.appendingPathComponent("cancel")
        try? FileManager.default.removeItem(at: folder)
        try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let url = folder.appendingPathComponent("kept.png")
        let original = Data("previous contents".utf8)
        try? original.write(to: url)
        var outcome: FlatExportTask.Outcome?
        guard let task = ws.startExportFlat(doc, s, to: url, then: { outcome = $0 }) else { check("cancel starts", false); return }
        _ = await wait(300) { task.fraction >= 0.3 || outcome != nil }
        let t = Date()
        ws.cancelExportFlat(task)
        _ = await wait(60) { outcome != nil }
        log(String(format: "cancel at %.0f %% → finished in %.0f ms", task.fraction * 100, Date().timeIntervalSince(t) * 1000))
        check("cancelled", outcome == .cancelled, "\(String(describing: outcome))")
        check("destination kept", (try? Data(contentsOf: url)) == original)
        let left = (try? FileManager.default.contentsOfDirectory(atPath: folder.path)) ?? []
        check("no temporary file", left == ["kept.png"], "\(left)")
    }
}
// B5-15 VARIANT end

/// Main-thread busy spans (run-loop wake → sleep), recorded by a common-modes observer.
final class MainThreadSpans: @unchecked Sendable {
    private var observer: CFRunLoopObserver?
    private var began = CFAbsoluteTimeGetCurrent()
    private var spans: [Double] = []

    func start() {
        spans = []
        began = CFAbsoluteTimeGetCurrent()
        let o = CFRunLoopObserverCreateWithHandler(nil, CFRunLoopActivity.afterWaiting.rawValue | CFRunLoopActivity.beforeWaiting.rawValue,
                                                   true, 0) { [weak self] _, activity in
            guard let self else { return }
            let now = CFAbsoluteTimeGetCurrent()
            if activity == .afterWaiting { self.began = now } else { self.spans.append((now - self.began) * 1000) }
        }
        observer = o
        CFRunLoopAddObserver(CFRunLoopGetMain(), o, .commonModes)
    }

    /// Stops recording; the spans (ms), including the one in progress.
    func stop() -> [Double] {
        if let o = observer { CFRunLoopRemoveObserver(CFRunLoopGetMain(), o, .commonModes) }
        observer = nil
        spans.append((CFAbsoluteTimeGetCurrent() - began) * 1000)
        return spans
    }
}
