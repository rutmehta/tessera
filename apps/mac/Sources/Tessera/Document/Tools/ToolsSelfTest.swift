import AppKit
import TesseraCore

/// `--tools-selftest <dir>` (test aid, WP B5-04): ACCEPTANCE §V through the viewport's own mouse
/// path (synthesized mouse events sent to the document viewport, so the tools' stroke capture,
/// per-frame coalescing and engine calls are the ones a user drives). After the library loads it
/// focuses `sample.dng`, runs Edit in Layers, paints a brush stroke (checks it and undo / redo),
/// erases to transparency on a new layer, clicks the magic wand, runs Select ▸ Subject, applies a
/// Free Transform, and saves `<dir>/ToolsSelfTest.psd` and reopens it. Each step prints
/// `tools-selftest: step <n> <name> window <x> <y> <w> <h>` (for `screencapture -R`); checks print
/// `check <name> ok|FAIL`; strokes print the engine time per frame and the frames' render time.
@MainActor
final class ToolsSelfTest {
    private let model: AppModel
    private let dir: URL
    private let hold: Double
    private var failures = 0
    private var step = 0

    init(model: AppModel, dir: URL, hold: Double = 2) {
        self.model = model
        self.dir = dir
        self.hold = hold
    }

    private func log(_ s: String) { FileHandle.standardError.write(Data("tools-selftest: \(s)\n".utf8)) }

    private func check(_ name: String, _ ok: Bool, _ detail: @autoclosure () -> String = "") {
        if !ok { failures += 1 }
        log("check \(name) " + (ok ? "ok" : "FAIL \(detail())"))
    }

    private func pause(_ s: Double) async { try? await Task.sleep(for: .milliseconds(Int(s * 1000))) }

    private func wait(_ timeout: Double, _ condition: () -> Bool) async -> Bool {
        let end = Date().addingTimeInterval(timeout)
        while !condition() {
            if Date() > end { return false }
            await pause(0.05)
        }
        return true
    }

    private func mark(_ name: String) async {
        step += 1
        await pause(0.8)
        var frame = ""
        if let w = model.mainWindow, let screen = NSScreen.screens.first {
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
        guard !v.isEmpty else { return "none" }
        let s = v.sorted()
        let q = { (p: Double) in s[min(s.count - 1, Int((Double(s.count - 1) * p).rounded()))] }
        return String(format: "n %d, median %.2f ms, p90 %.2f ms, max %.2f ms", s.count, q(0.5), q(0.9), s.last!)
    }

    // MARK: Synthesized mouse input

    private func event(_ type: NSEvent.EventType, at canvas: CGPoint, in v: DocumentViewportView,
                       flags: NSEvent.ModifierFlags = [], clicks: Int = 1, pressure: Float = 1) -> NSEvent? {
        guard let w = v.window else { return nil }
        let p = v.convert(v.viewPoint(canvas: canvas), to: nil)
        return NSEvent.mouseEvent(with: type, location: p, modifierFlags: flags, timestamp: ProcessInfo.processInfo.systemUptime,
                                  windowNumber: w.windowNumber, context: nil, eventNumber: 0, clickCount: clicks, pressure: pressure)
    }

    private func click(_ canvas: CGPoint, in v: DocumentViewportView, flags: NSEvent.ModifierFlags = []) async {
        if let e = event(.leftMouseDown, at: canvas, in: v, flags: flags) { v.mouseDown(with: e) }
        if let e = event(.leftMouseUp, at: canvas, in: v, flags: flags) { v.mouseUp(with: e) }
        await pause(0.05)
    }

    /// A drag along `points` at ~120 Hz pointer rate.
    private func drag(_ points: [CGPoint], in v: DocumentViewportView) async {
        guard let first = points.first, let last = points.last else { return }
        if let e = event(.leftMouseDown, at: first, in: v) { v.mouseDown(with: e) }
        for p in points.dropFirst() {
            if let e = event(.leftMouseDragged, at: p, in: v) { v.mouseDragged(with: e) }
            await pause(1.0 / 120)
        }
        if let e = event(.leftMouseUp, at: last, in: v) { v.mouseUp(with: e) }
    }

    private func line(_ a: CGPoint, _ b: CGPoint, _ n: Int) -> [CGPoint] {
        (0...n).map { i in
            let t = Double(i) / Double(n)
            return CGPoint(x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t)
        }
    }

    // MARK: Run

    func run() async {
        let ws = model.documents
        let tools = DocumentTools.shared
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        guard await wait(60, { !model.isLoading && !model.library.items.isEmpty }) else {
            log("FAIL the library did not load"); return finish()
        }
        let items = model.library.items
        guard let item = items.first(where: { $0.name == "sample.dng" }) ?? items.first(where: { $0.kind == .raw }) else {
            log("FAIL no RAW in the library"); return finish()
        }
        model.select(id: item.id)
        model.showRenderReadout = true

        // 1. Edit in Layers.
        ws.editInLayers(model.focusedItem)
        guard await wait(120, { ws.current != nil && ws.opening == nil }), let doc = ws.current else {
            log("FAIL Edit in Layers opened nothing: \(model.statusMessage ?? "")"); return finish()
        }
        _ = await wait(10) { doc.lastFrame != nil && doc.viewport != nil }
        guard let v = doc.viewport, let t = doc.backend as? DocumentToolsBackend else { log("FAIL no viewport"); return finish() }
        tools.attach(ws)
        let (w, h) = (Double(doc.info.width), Double(doc.info.height))
        log("document \(doc.info.width) × \(doc.info.height) px, \(doc.info.depth.title), \(doc.info.backend)")
        guard let photo = doc.layers.first(where: { $0.kind == .pixel })?.id else { return finish() }
        await mark("edit-in-layers")

        // 2. Brush stroke (red, 60 px), through the viewport's mouse path.
        tools.select(.brush)
        tools.colors.foreground = ToolColor(r: 1, g: 0, b: 0)
        var b = tools.currentBrush
        b.size = Float(max(w, h) / 60)
        b.hardness = 1
        b.opacity = 1
        b.flow = 1
        tools.currentBrush = b
        var engineMs: [Double] = []
        var renderMs: [Double] = []
        var dabs: UInt32 = 0
        let epoch0 = doc.lastFrame?.epoch ?? 0
        tools.strokeObserver = { f in engineMs.append(f.rasterMs); dabs = f.totalDabs }
        doc.frameObserver = { f in if f.epoch > epoch0 { renderMs.append(f.renderMs) } }
        let rows = doc.history.count
        let a = CGPoint(x: w * 0.2, y: h * 0.3), c = CGPoint(x: w * 0.8, y: h * 0.35)
        await drag(line(a, c, 180), in: v)
        await tools.idle()
        await pause(0.3)
        tools.strokeObserver = nil
        doc.frameObserver = nil
        log("brush stroke: \(dabs) dabs; engine stroke_points " + Self.stats(engineMs) + "; frame render " + Self.stats(renderMs))
        check("brush stroke is one history row", doc.history.count == rows + 1 && doc.history.last?.label == "Brush Tool",
              "\(doc.history.map(\.label))")
        let mid = CanvasPoint(x: Float((a.x + c.x) / 2), y: Float((a.y + c.y) / 2))
        let painted = try? t.sampleColor(at: mid, sampleAll: true, layer: nil, radius: 0)
        check("stroke visible", painted.map { $0.r > 0.99 && $0.g < 0.01 && $0.b < 0.01 } ?? false, "\(String(describing: painted))")
        let sorted = renderMs.sorted()
        check("dab frames under 16 ms (median)", !sorted.isEmpty && sorted[sorted.count / 2] < 16, Self.stats(renderMs))
        await mark("brush-stroke")
        model.undo()
        let undone = try? t.sampleColor(at: mid, sampleAll: true, layer: nil, radius: 0)
        check("undo removes the stroke", undone.map { !($0.r > 0.99 && $0.g < 0.01) } ?? false, "\(String(describing: undone))")
        await mark("brush-undo")
        model.redo()
        await pause(0.3)

        // 3. Eraser to transparency on the photo layer.
        doc.select(photo)
        tools.select(.eraser)
        var e = tools.currentBrush
        e.size = Float(max(w, h) / 20)
        e.hardness = 1
        tools.currentBrush = e
        let ea = CGPoint(x: w * 0.3, y: h * 0.7), ec = CGPoint(x: w * 0.7, y: h * 0.7)
        await drag(line(ea, ec, 120), in: v)
        await tools.idle()
        check("eraser row", doc.history.last?.label == "Eraser", "\(doc.history.map(\.label))")
        let erased = CanvasPoint(x: Float(w * 0.5), y: Float(h * 0.7))
        let layerSample = Result { try t.sampleColor(at: erased, sampleAll: false, layer: photo, radius: 0) }
        if case .failure(let err) = layerSample {
            check("erased to transparency", err.localizedDescription.contains("transparent"), err.localizedDescription)
        } else {
            check("erased to transparency", false, "\(layerSample)")
        }
        await mark("eraser")

        // 4. Magic wand: a click in the sky / top band; the outline appears.
        tools.select(.wand)
        tools.tolerance = 24
        tools.contiguous = true
        await click(CGPoint(x: w * 0.5, y: h * 0.05), in: v)
        await tools.idle()
        _ = await wait(5) { !tools.outline.isEmpty }
        check("wand selection", doc.marquee != nil && doc.history.last?.label == "Magic Wand", "\(doc.history.map(\.label))")
        check("wand outline", !tools.outline.isEmpty, "\(tools.outline.count) loops")
        log("wand: selection \(doc.marquee.map { "\($0.width) × \($0.height)" } ?? "none"), outline \(tools.outline.count) loops, "
            + "\(tools.outline.map(\.points.count).reduce(0, +)) points")
        await mark("wand")

        // 5. Select ▸ Subject (on-device model; the first run may download it).
        tools.selectSubject()
        await tools.idle()
        _ = await wait(5) { !tools.outline.isEmpty }
        check("subject selection", doc.history.last?.label == "Select Subject" && doc.marquee != nil,
              "\(doc.history.map(\.label)) \(model.statusMessage ?? "")")
        log("subject: \(model.statusMessage ?? "")")
        await mark("subject")
        tools.deselect()

        // 6. Free Transform: scale 80 % and rotate 8° about the centre, then commit.
        doc.select(photo)
        tools.select(.move)
        tools.beginFreeTransform()
        check("transform began", tools.transform != nil, model.statusMessage ?? "")
        tools.updateTransform { $0.sx = 0.8; $0.sy = 0.8; $0.angle = 8 }
        await pause(1.5)
        await mark("transform-preview")
        let before = doc.history.count
        tools.commitTransform()
        await tools.idle()
        check("transform committed", doc.history.count == before + 1 && doc.history.last?.label == "Free Transform",
              "\(doc.history.map(\.label))")
        await mark("transform-commit")

        // 7. PSD save and reopen.
        let psd = dir.appendingPathComponent("ToolsSelfTest.psd")
        try? FileManager.default.removeItem(at: psd)
        let names = doc.layers.map(\.name)
        let saved = ws.write(doc, to: psd)
        check("save PSD", saved, model.statusMessage ?? "")
        ws.discard(doc)
        if saved {
            ws.open(psd)
            if await wait(60, { ws.current != nil && ws.opening == nil }), let p = ws.current {
                _ = await wait(10) { p.lastFrame != nil }
                check("PSD reopens with the layers", p.layers.map(\.name) == names, "\(p.layers.map(\.name)) vs \(names)")
                await mark("psd-reopen")
            } else {
                check("PSD reopens", false, model.statusMessage ?? "")
            }
        }
        finish()
    }

    private func finish() {
        log("done, \(failures) failure(s)")
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) { NSApp.terminate(nil) }
    }
}
