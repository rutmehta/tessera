import AppKit
import TesseraCore

/// `--filter-selftest <dir>` (test aid, WP M5-12): ACCEPTANCE §U part 3 through the same controller
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
    private let model: AppModel
    private let dir: URL
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

    private func log(_ s: String) { FileHandle.standardError.write(Data("filter-selftest: \(s)\n".utf8)) }

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
    private func frameAfter(_ doc: DocumentController, _ action: () -> Void) async -> Double? {
        var got: Date?
        let start = Date()
        doc.frameObserver = { _ in if got == nil { got = Date() } }
        action()
        let ok = await wait(10) { got != nil }
        doc.frameObserver = nil
        return ok ? got.map { $0.timeIntervalSince(start) * 1000 } : nil
    }

    private func idle(_ filters: DocumentFilters) async -> Bool { await wait(120) { filters.busy == nil } }

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

    private func finish() {
        log("done, \(failures) failure(s)")
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) { NSApp.terminate(nil) }
    }
}
