import AppKit
import ImageIO
import SwiftUI
import TesseraCore
import TesseraFFI

/// `--document-selftest <dir>` (test aid, WP B5-03): ACCEPTANCE §U part 2 through the same controller
/// calls the UI makes. After the library loads it focuses `sample.dng` (or the first RAW), runs Edit in
/// Layers, adds an Exposure adjustment layer and drags it, drags the photo layer's Opacity 100 → 40 % at
/// display rate (61 interactive steps, then the release), undoes, saves `<dir>/SelfTest.tessera-doc`,
/// closes and reopens it, exports `<dir>/SelfTest.png`, saves `<dir>/SelfTest.psd` and opens that. Each
/// step prints `document-selftest: step <n> <name> window <x> <y> <w> <h>` (screen points, top-left
/// origin, for `screencapture -R`) and holds `hold` seconds; checks print `check <name> ok|FAIL`; the
/// opacity drag prints the listener's frame timing. Quits at the end.
/// B5-16: a `--nonactivating` run (the Mac is in use) never floats or fronts a window; its steps are
/// captured with `screencapture -x -o -l <window>` instead.
enum BackgroundRun {
    static let active = ProcessInfo.processInfo.arguments.contains("--nonactivating")

    @MainActor private static var hostWindow: NSWindow?

    /// A background run whose SwiftUI `Window` scene did not open a window (seen while another
    /// instance of the app was running) hosts the shell in its own window, ordered to the back and
    /// never made key, so the document self-tests still have a viewport. Checked 5 s after launch.
    @MainActor static func ensureWindow(_ model: AppModel) {
        DispatchQueue.main.asyncAfter(deadline: .now() + 5) {
            MainActor.assumeIsolated {
                guard hostWindow == nil,
                      !NSApp.windows.contains(where: { !($0 is NSPanel) && $0.isVisible && $0.contentViewController != nil })
                else { return }
                let w = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1440, height: 900),
                                 styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
                                 backing: .buffered, defer: false)
                w.isReleasedWhenClosed = false
                w.toolbarStyle = .unified
                w.title = "Tessera"
                w.contentViewController = NSHostingController(rootView: ContentView.root(model: model))
                w.orderBack(nil)
                hostWindow = w
                FileHandle.standardError.write(Data("background-run: no scene window; hosting the shell in window \(w.windowNumber)\n".utf8))
            }
        }
    }
}

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
        if !perfMode, let w = model.mainWindow, let screen = NSScreen.screens.first {
            // Above other apps' windows while the test runs, so `screencapture -R` sees only Tessera
            // (not in a background run: B5-16).
            if !BackgroundRun.active {
                w.level = .floating
                w.orderFrontRegardless()
            }
            await pause(0.3)
            let f = w.frame
            frame = String(format: " window %.0f %.0f %.0f %.0f", f.minX, screen.frame.height - f.maxY, f.width, f.height)
                + " window-id \(w.windowNumber)"
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
        // B5-16: in a background run the viewport attaches when SwiftUI next updates the (unraised) window.
        let attached = await wait(180) { doc.viewport != nil }
        if !attached {
            log("viewport not attached: mode \(model.viewMode) window \(model.mainWindow != nil) "
                + "windows \(NSApp.windows.map { "\(type(of: $0)) \($0.isVisible)" })")
        }
        _ = await wait(20) { doc.lastFrame != nil }
        log(String(format: "edit in layers: %.2f s to first frame", Date().timeIntervalSince(t0)))
        check("engine backend", doc.backend is EngineDocumentBackend, "\(type(of: doc.backend))")
        check("one pixel layer", doc.layers.count == 1 && doc.layers.first?.kind == .pixel, "\(doc.layers.map(\.name))")
        check("source image", doc.info.sourceImageId == item.engineImage?.imageID, "\(doc.info.sourceImageId ?? "nil")")
        log("document \(doc.info.width) × \(doc.info.height) px, \(doc.info.depth.title), \(doc.info.backend), title \(doc.title)")
        check("first frame", doc.lastFrame != nil)
        if perfMode { await perf(doc, item: item); return finish() }   // B5-14
        await mark("edit-in-layers")

        // B5-16: the inspector's sub-tabs switch with ⌃1 / ⌃2 / ⌃3 (the window's key equivalents), and
        // History collapses and expands.
        model.showInspector = true
        await pause(0.3)
        if let w = doc.viewport?.window {
            let codes: [Character: UInt16] = ["1": 18, "2": 19, "3": 20]
            for tab in [DocumentInspectorTab.properties, .channels, .stack] {
                let c = String(tab.shortcutDigit)
                if let e = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: .control,
                                            timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: w.windowNumber,
                                            context: nil, characters: c, charactersIgnoringModifiers: c, isARepeat: false,
                                            keyCode: codes[tab.shortcutDigit] ?? 0) {
                    _ = w.performKeyEquivalent(with: e)
                }
                _ = await wait(2) { ws.inspectorTab == tab }
                check("B5-16 ⌃\(c) shows \(tab.title)", ws.inspectorTab == tab, ws.inspectorTab.title)
                await mark("inspector-\(tab.rawValue)")
            }
        } else {
            check("B5-16 document window", false)
        }

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

// MARK: - B5-14 perf scenarios

/// `TESSERA_DOC_PERF=1` with `--document-selftest <dir>` (WP B5-14): instead of the acceptance steps, builds a
/// 60-layer document from the developed photo, sizes the window so the viewport is 3840 × 2160 device pixels,
/// and measures (1) 100 % pans and edits to the completed frame (P13), (2) synchronous mutation calls while slow
/// 4K frames are in flight (P14), (3) input → presented frame while a photo export runs, against idle
/// (P17). Lines start with `document-perf:`; nothing is saved. The window is never raised or made key.
extension DocumentSelfTest {
    var perfMode: Bool { ProcessInfo.processInfo.environment["TESSERA_DOC_PERF"] != nil }

    private func plog(_ s: String) { FileHandle.standardError.write(Data("document-perf: \(s)\n".utf8)) }

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

    /// Waits for the first frame accepted by `match` after `since` (frame count); returns its arrival time.
    private func nextFrame(_ doc: DocumentController, after count: Int, frames: () -> [(Date, DocFrame)],
                           timeout: Double = 5, _ match: (DocFrame) -> Bool) async -> Date? {
        let end = Date().addingTimeInterval(timeout)
        while Date() < end {
            let f = frames()
            if f.count > count, let hit = f[count...].first(where: { match($0.1) }) { return hit.0 }
            try? await Task.sleep(for: .microseconds(500))
        }
        return nil
    }

    func perf(_ doc: DocumentController, item: PhotoItem) async {
        plog("start: document \(doc.info.width) × \(doc.info.height) \(doc.info.depth.title), backend \(doc.info.backend), footprint \(String(format: "%.0f", footprintMiB())) MiB")
        guard let photo = doc.layers.first(where: { $0.kind == .pixel })?.id else { plog("FAIL no pixel layer"); return }
        var frames: [(Date, DocFrame)] = []
        doc.frameObserver = { f in frames.append((Date(), f)) }
        let b = doc.backend
        // 1. 60 layers: duplicates of the photo (copy-on-write) in varied modes at 50 %.
        let modes = ["multiply", "screen", "overlay", "soft_light", "darken", "lighten", "difference", "color_dodge"]
        var ids = [photo]
        for i in 0..<59 {
            guard let c = try? b.duplicateLayer(id: ids[ids.count - 1]), let n = c.created.first else { break }
            _ = try? b.setBlendMode(id: n, mode: modes[i % modes.count])
            _ = try? b.setOpacity(id: n, value: 0.5, interactive: false)
            ids.append(n)
        }
        doc.reloadModel(); doc.reloadHistory()
        plog("layers \(doc.layers.count)")
        // 2. A 3840 × 2160 device-pixel viewport (content size in points), window stays where it is and in the back.
        if let w = model.mainWindow, let v = doc.viewport {
            let scale = w.backingScaleFactor
            let extraW = w.frame.width - v.bounds.width, extraH = w.frame.height - v.bounds.height
            let size = NSSize(width: 3840 / scale + extraW, height: 2160 / scale + extraH)
            w.setFrame(NSRect(origin: w.frame.origin, size: size), display: true)
            await pause(1.0)
            plog(String(format: "viewport %.0f × %.0f pt @%.0fx, window %.0f × %.0f", v.bounds.width, v.bounds.height, scale,
                        w.frame.width, w.frame.height))
        }
        guard let view = doc.viewport else { plog("FAIL no viewport"); return }
        view.zoomActual()
        _ = await wait(20) { doc.lastFrame?.level == 0 }
        await pause(4)   // specialized kernel compiles in the background
        if let f = doc.lastFrame { plog("100 %: L\(f.level) \(f.width) × \(f.height)") }

        // P13: pans at 100 % (input → completed frame showing the new region).
        var pan: [Double] = []
        for i in 0..<40 {
            let before = frames.count
            let old = doc.lastFrame.map { ($0.x, $0.y) }
            let t = Date()
            view.panBy(dx: i % 2 == 0 ? 97 : -61, dy: i % 3 == 0 ? 53 : -29)
            if let at = await nextFrame(doc, after: before, frames: { frames }, { f in old.map { $0 != (f.x, f.y) } ?? true }) {
                pan.append(at.timeIntervalSince(t) * 1000)
            }
            await pause(1.0 / 60)
        }
        report("p13 100% 4K pan input→frame", pan)
        // P13: opacity edits at 100 % (UI path: run + reloads).
        func edits(_ n: Int, _ label: String) async -> [Double] {
            var v: [Double] = []
            for i in 0..<n {
                let before = frames.count
                let t = Date()
                guard let c = doc.run("Opacity", { try b.setOpacity(id: ids[ids.count / 2], value: Float(0.3 + Double(i % 10) * 0.05), interactive: true) })
                else { continue }
                if let at = await nextFrame(doc, after: before, frames: { frames }, { $0.epoch >= c.epoch }) {
                    v.append(at.timeIntervalSince(t) * 1000)
                }
                await pause(1.0 / 60)
            }
            return v
        }
        _ = await edits(5, "warm")
        let idle = await edits(60, "idle")
        report("p13 100% 4K opacity input→frame", idle)
        let render = frames.suffix(100).map(\.1.renderMs)
        report("p13 100% 4K engine render_ms", render)
        _ = try? b.commit(label: "Opacity")

        // P17: the same edits while a photo export (Web preset, full-resolution develop) runs.
        if let lib = model.engineLibrary, let imageId = item.engineImage?.imageID,
           let preset = try? lib.engine.exportPresets().first,
           var json = (try? JSONSerialization.jsonObject(with: Data(preset.settingsJson.utf8))) as? [String: Any] {
            let out = dir.appendingPathComponent("perf-export")
            try? FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
            json["destination"] = out.path
            json["on_conflict"] = "unique"
            let settings = String(decoding: (try? JSONSerialization.data(withJSONObject: json)) ?? Data(), as: UTF8.self)
            let engine = lib.engine
            let stop = CancelFlag()
            final class Box: @unchecked Sendable { var done = 0; var secs: [Double] = []; let lock = NSLock() }
            let box = Box()
            let task = Task.detached(priority: .userInitiated) {
                while !stop.isCancelled() {
                    let t = Date()
                    let r = try? engine.exportBatch(target: .images(imageIds: [imageId]), settingsJson: settings, listener: nil, cancel: nil)
                    box.lock.withLock { if r?.exported == 1 { box.done += 1; box.secs.append(Date().timeIntervalSince(t)) } }
                }
            }
            await pause(1.5)
            let busy = await edits(120, "export")
            stop.cancel()
            await task.value
            report("p17 100% 4K opacity input→frame during export", busy)
            let (done, secs) = box.lock.withLock { (box.done, box.secs) }
            plog(String(format: "RESULT p17 ratio p95 export/idle %.2f (target ≤ 1.25); exports completed %d (%.2f–%.2f s)",
                        Self.pct(busy, 0.95) / Self.pct(idle, 0.95), done, secs.min() ?? .nan, secs.max() ?? .nan))
        } else {
            plog("p17 skipped: no export preset or image")
        }
        plog(String(format: "footprint %.0f MiB", footprintMiB()))

        // P14: synchronous calls while 100 % 4K frames are in flight (each tick of another layer's opacity
        // requests a frame; the calls below race it). Styled (CPU) frames of a 20 MP layer take minutes per
        // frame (perf audit hotspot 4, P15), so the slow-style case is measured by the Rust bench instead.
        var calls: [String: [Double]] = [:]
        let driver = ids[ids.count / 2], target = ids[ids.count - 1]
        for i in 0..<150 {
            _ = try? b.setOpacity(id: driver, value: Float(0.3 + Double(i % 10) * 0.05), interactive: true)
            await pause(0.004)
            let t = Date()
            let name: String
            switch i % 3 {
            case 0: name = "opacity"; _ = try? b.setOpacity(id: target, value: Float(0.3 + Double(i % 7) * 0.1), interactive: true)
            case 1: name = "rename"; _ = try? b.renameLayer(id: target, name: "perf \(i)")
            default: name = "visibility"; _ = try? b.setVisible(id: target, visible: i % 6 != 2)
            }
            calls[name, default: []].append(Date().timeIntervalSince(t) * 1000)
            await pause(0.012)
        }
        for (k, v) in calls.sorted(by: { $0.key < $1.key }) { report("p14 \(k) call during 4K frames", v) }
        report("p14 all mutation calls during 4K frames", calls.values.flatMap { $0 })
        _ = try? b.commit(label: "perf")
        doc.frameObserver = nil
        plog(String(format: "done, footprint %.0f MiB", footprintMiB()))
    }
}
