import AppKit
import ImageIO
import TesseraCore

/// `--document-selftest <dir>` (test aid, WP M5-13b): ACCEPTANCE §U part 2 through the same controller
/// calls the UI makes. After the library loads it focuses `sample.dng` (or the first RAW), runs Edit in
/// Layers, adds an Exposure adjustment layer and drags it, drags the photo layer's Opacity 100 → 40 % at
/// display rate (61 interactive steps, then the release), undoes, saves `<dir>/SelfTest.tessera-doc`,
/// closes and reopens it, exports `<dir>/SelfTest.png`, saves `<dir>/SelfTest.psd` and opens that. Each
/// step prints `document-selftest: step <n> <name> window <x> <y> <w> <h>` (screen points, top-left
/// origin, for `screencapture -R`) and holds `hold` seconds; checks print `check <name> ok|FAIL`; the
/// opacity drag prints the listener's frame timing. Quits at the end.
@MainActor
final class DocumentSelfTest {
    private let model: AppModel
    private let dir: URL
    private let hold: Double
    private var failures = 0

    init(model: AppModel, dir: URL, hold: Double = 2.5) {
        self.model = model
        self.dir = dir
        self.hold = hold
    }

    private func log(_ s: String) { FileHandle.standardError.write(Data("document-selftest: \(s)\n".utf8)) }

    private func check(_ name: String, _ ok: Bool, _ detail: @autoclosure () -> String = "") {
        if !ok { failures += 1 }
        log("check \(name) " + (ok ? "ok" : "FAIL \(detail())"))
    }

    private func pause(_ s: Double) async { try? await Task.sleep(for: .milliseconds(Int(s * 1000))) }

    /// Polls `condition` every 50 ms for up to `timeout` seconds.
    private func wait(_ timeout: Double, _ condition: () -> Bool) async -> Bool {
        let end = Date().addingTimeInterval(timeout)
        while !condition() {
            if Date() > end { return false }
            await pause(0.05)
        }
        return true
    }

    private var step = 0
    private func mark(_ name: String) async {
        step += 1
        // Let SwiftUI and the viewport settle, then report the window for the screenshot.
        await pause(0.8)
        var frame = ""
        if let w = model.mainWindow, let screen = NSScreen.screens.first {
            // Above other apps' windows while the test runs, so `screencapture -R` sees only Tessera.
            w.level = .floating
            w.orderFrontRegardless()
            await pause(0.3)
            let f = w.frame
            frame = String(format: " window %.0f %.0f %.0f %.0f", f.minX, screen.frame.height - f.maxY, f.width, f.height)
        }
        log("step \(step) \(name)\(frame)")
        await pause(hold)
    }

    private static func stats(_ v: [Double]) -> String {
        guard !v.isEmpty else { return "no frames" }
        let s = v.sorted()
        let q = { (p: Double) in s[min(s.count - 1, Int((Double(s.count - 1) * p).rounded()))] }
        return String(format: "frames %d, render median %.2f ms, p90 %.2f ms, max %.2f ms", s.count, q(0.5), q(0.9), s.last!)
    }

    func run() async {
        let ws = model.documents
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        guard await wait(60, { !model.isLoading && !model.library.items.isEmpty }) else {
            log("FAIL the library did not load"); return finish()
        }
        let items = model.library.items
        guard let item = items.first(where: { $0.name == "sample.dng" }) ?? items.first(where: { $0.kind == .raw }) else {
            log("FAIL no RAW in the library"); return finish()
        }
        model.select(id: item.id)
        // Show the render readout while the test runs; the preference is restored at the end.
        restoreReadout = model.showRenderReadout
        model.showRenderReadout = true

        // 1. Edit in Layers (developed, on the engine).
        let t0 = Date()
        ws.editInLayers(model.focusedItem)
        guard await wait(120, { ws.current != nil && ws.opening == nil }), let doc = ws.current else {
            log("FAIL Edit in Layers opened nothing: \(model.statusMessage ?? "")"); return finish()
        }
        _ = await wait(10) { doc.lastFrame != nil }
        log(String(format: "edit in layers: %.2f s to first frame", Date().timeIntervalSince(t0)))
        check("engine backend", doc.backend is EngineDocumentBackend, "\(type(of: doc.backend))")
        check("one pixel layer", doc.layers.count == 1 && doc.layers.first?.kind == .pixel, "\(doc.layers.map(\.name))")
        check("source image", doc.info.sourceImageId == item.engineImage?.imageID, "\(doc.info.sourceImageId ?? "nil")")
        log("document \(doc.info.width) × \(doc.info.height) px, \(doc.info.depth.title), \(doc.info.backend), title \(doc.title)")
        check("first frame", doc.lastFrame != nil)
        await mark("edit-in-layers")

        guard let photo = doc.layers.first(where: { $0.kind == .pixel })?.id else { return finish() }

        // 2. Adjustment layer: Exposure, dragged 0 → +1 EV.
        doc.addAdjustment(.exposure)
        guard let adj = doc.primary, adj.kind == .adjustment else {
            check("adjustment layer added", false, "\(doc.layers.map(\.name))"); return finish()
        }
        check("adjustment layer added", doc.layers.count == 2, "\(doc.layers.map(\.name))")
        var adjTimes: [Double] = []
        var epoch = doc.lastFrame?.epoch ?? 0
        doc.frameObserver = { f in if f.epoch > epoch { adjTimes.append(f.renderMs) } }
        for i in 0...20 {
            doc.setAdjustment(adj.id, .exposure(exposure: Double(i) / 20, offset: 0, gamma: 1), final: i == 20)
            await pause(1.0 / 60)
        }
        await pause(0.3)
        log("exposure drag: " + Self.stats(adjTimes))
        check("exposure committed as one row", doc.history.last?.label == "Exposure", "\(doc.history.map(\.label))")
        await mark("adjustment")

        // 3. Opacity drag on the photo layer, 100 → 40 % at display rate.
        doc.select(photo)
        let rowsBefore = doc.history.count
        var times: [Double] = []
        var levels = Set<UInt8>()
        epoch = doc.lastFrame?.epoch ?? 0
        doc.frameObserver = { f in
            guard f.epoch > epoch else { return }
            times.append(f.renderMs)
            levels.insert(f.level)
        }
        var callMs: [Double] = []
        for i in 0...60 {
            let v = 100 - 60 * Double(i) / 60
            let c = Date()
            doc.setOpacity(v, final: i == 60)
            callMs.append(Date().timeIntervalSince(c) * 1000)
            await pause(1.0 / 60)
        }
        await pause(0.4)
        doc.frameObserver = nil
        let viewport = doc.lastFrame.map { "L\($0.level) \($0.width) × \($0.height)" } ?? "-"
        log("opacity drag: " + Self.stats(times) + ", levels \(levels.sorted()), viewport \(viewport)")
        log("opacity drag: main-thread call " + Self.stats(callMs).replacingOccurrences(of: "render ", with: ""))
        check("opacity 40 %", abs((doc.node(photo)?.opacity ?? 0) - 0.4) < 0.01, "\(doc.node(photo)?.opacity ?? -1)")
        check("one history row for the drag", doc.history.count == rowsBefore + 1 && doc.history.last?.label == "Opacity 40 %",
              "\(doc.history.map(\.label))")
        let sorted = times.sorted()
        check("opacity frame median < 16 ms", !sorted.isEmpty && sorted[sorted.count / 2] < 16, Self.stats(times))
        await mark("opacity-drag")

        // 4. Undo (⌘Z routes to the document in document mode).
        model.undo()
        check("undo restores opacity", abs((doc.node(photo)?.opacity ?? 0) - 1) < 1e-4, "\(doc.node(photo)?.opacity ?? -1)")
        await mark("undo")

        // 5. Save .tessera-doc, close, reopen.
        let native = dir.appendingPathComponent("SelfTest.tessera-doc")
        try? FileManager.default.removeItem(at: native)
        check("save .tessera-doc", ws.write(doc, to: native) && !doc.isDirty, model.statusMessage ?? "")
        let names = doc.layers.map(\.name)
        let adjJSON = doc.node(adj.id)?.adjustmentJson
        await mark("saved")
        ws.discard(doc)
        ws.open(native)
        guard await wait(60, { ws.current != nil && ws.opening == nil }), let reopened = ws.current else {
            check("reopen", false, model.statusMessage ?? ""); return finish()
        }
        _ = await wait(10) { reopened.lastFrame != nil }
        check("reopen layers", reopened.layers.map(\.name) == names, "\(reopened.layers.map(\.name)) vs \(names)")
        check("reopen adjustment", reopened.layers.contains { $0.kind == .adjustment && $0.adjustmentJson == adjJSON },
              "\(reopened.layers.map { $0.adjustmentJson ?? "-" })")
        check("reopen title", reopened.title == "SelfTest.tessera-doc", reopened.title)
        await mark("reopened")

        // 6. Export flat PNG.
        let png = dir.appendingPathComponent("SelfTest.png")
        try? FileManager.default.removeItem(at: png)
        let exported = ws.exportFlat(reopened, ExportFlatSettings(format: .png, quality: 90, color: .srgb), to: png)
        var size = (0, 0)
        if let src = CGImageSourceCreateWithURL(png as CFURL, nil), let img = CGImageSourceCreateImageAtIndex(src, 0, nil) {
            size = (img.width, img.height)
        }
        check("export flat PNG", exported && size.0 == Int(reopened.info.width) && size.1 == Int(reopened.info.height),
              "\(size) \(model.statusMessage ?? "")")
        await mark("exported")

        // 7. Save As PSD, close, open the PSD.
        let psd = dir.appendingPathComponent("SelfTest.psd")
        try? FileManager.default.removeItem(at: psd)
        let psdSaved = ws.write(reopened, to: psd)
        check("save as PSD", psdSaved, model.statusMessage ?? "")
        ws.discard(reopened)
        if psdSaved {
            ws.open(psd)
            if await wait(60, { ws.current != nil && ws.opening == nil }), let p = ws.current {
                _ = await wait(10) { p.lastFrame != nil }
                check("PSD layers", p.layers.map(\.name) == names, "\(p.layers.map(\.name)) vs \(names)")
                check("PSD adjustment kind", p.layers.contains { $0.kind == .adjustment }, "\(p.layers.map(\.kind))")
                await mark("psd-open")
                if let pixel = p.layers.first(where: { $0.kind == .pixel })?.id { profileCalls(p, layer: pixel) }
            } else {
                check("open PSD", false, model.statusMessage ?? "")
            }
        }
        finish()
    }

    /// Main-thread cost of the calls one slider tick makes (median of 10), then the release.
    private func profileCalls(_ doc: DocumentController, layer: DocLayerID) {
        let b = doc.backend
        func time(_ name: String, _ body: () throws -> Void) -> String {
            var v: [Double] = []
            for _ in 0..<10 {
                let t = Date()
                try? body()
                v.append(Date().timeIntervalSince(t) * 1000)
            }
            return String(format: "%@ %.2f", name, v.sorted()[5])
        }
        let parts = [
            time("setOpacity(interactive)") { _ = try b.setOpacity(id: layer, value: 0.4, interactive: true) },
            time("layers") { _ = try b.layers() },
            time("info") { _ = try b.info() },
            time("historyItems") { _ = try b.historyItems() },
            time("snapshots") { _ = try b.snapshots() },
            time("historyMemoryBytes") { _ = try b.historyMemoryBytes() },
            time("layerThumbnail(after a property change)") {
                _ = try b.setOpacity(id: layer, value: Float.random(in: 0.3...0.5), interactive: true)
                _ = try b.layerThumbnail(id: layer, maxPx: 64)
            },
            time("reloadModel") { doc.reloadModel() },
            time("reloadHistory") { doc.reloadHistory() },
        ]
        log("call cost (ms, median of 10): " + parts.joined(separator: ", "))
        _ = try? b.commit(label: "Opacity 40 %")
        doc.reloadModel()
        doc.reloadHistory()
    }

    private var restoreReadout: Bool?

    private func finish() {
        if let r = restoreReadout { model.showRenderReadout = r }
        log("done, \(failures) failure(s)")
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) { NSApp.terminate(nil) }
    }
}
