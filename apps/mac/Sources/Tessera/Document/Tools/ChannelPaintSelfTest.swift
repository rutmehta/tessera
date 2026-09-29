import AppKit
import TesseraCore

/// `--channel-paint-selftest=<dir>` (test aid, WP B5-17c): ACCEPTANCE steps 491–499 through the models the
/// UI drives, with synthesized mouse events sent straight to the document viewport (the app is never
/// activated or brought to the front). Creates its own 1200 × 800 document, then: Quick Mask in, paint
/// black, Quick Mask out (the selection); paint white into a saved alpha channel targeted as a plain click
/// in the Channels panel does, with its eye on, checking the thumbnail revision moves while the stroke is
/// live; eraser on the channel; undo / redo per stroke; a selection clips channel paint; a click on RGB paints
/// the layer again; the RGB composite never changes under channel strokes; save as `.tessera-doc` and PSD,
/// reopen, and the painted channel is back. Each step prints `channel-paint-selftest: step <n>-<name>
/// window-id <id>` and waits (up to `hold` + 15 s) for `<dir>/ack-<n>` so an external `screencapture -l <id>`
/// can capture the window; checks print `check <name> ok|FAIL …`. Prints `done, <n> failure(s)` and quits.
@MainActor
final class ChannelPaintSelfTest {
    private let model: AppModel
    private let dir: URL
    private let hold: Double
    private var failures = 0
    private var step = 0
    private static var started = false

    static func startIfRequested() {
        guard !started else { return }
        let args = CommandLine.arguments
        guard let a = args.first(where: { $0.hasPrefix("--channel-paint-selftest=") }) else { return }
        started = true
        let dir = URL(fileURLWithPath: (String(a.dropFirst("--channel-paint-selftest=".count)) as NSString).expandingTildeInPath)
        let hold = args.firstIndex(of: "--channel-paint-selftest-hold")
            .flatMap { $0 + 1 < args.count ? Double(args[$0 + 1]) : nil } ?? 0.4
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) {
            MainActor.assumeIsolated {
                let test = ChannelPaintSelfTest(model: AppModel.shared, dir: dir, hold: hold)
                Task { @MainActor in await test.run() }
            }
        }
    }

    private init(model: AppModel, dir: URL, hold: Double) {
        self.model = model
        self.dir = dir
        self.hold = hold
    }

    private let tools = DocumentTools.shared
    private let channels = DocumentChannels.shared

    /// A line starting "FAIL" (an early exit: no library, no document, …) counts as a failure, so the
    /// closing `done, <n> failure(s)` is never a silent 0 for a run that did not happen.
    private func log(_ s: String) {
        if s.hasPrefix("FAIL") { failures += 1 }
        FileHandle.standardError.write(Data("channel-paint-selftest: \(s)\n".utf8))
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
            await pause(0.05)
        }
        return true
    }

    /// Logs the step and waits for the capture script's acknowledgement (never raises the window).
    private func mark(_ name: String, _ doc: DocumentController) async {
        step += 1
        await tools.idle()
        doc.viewport?.toolOverlay.needsDisplay = true
        await pause(hold)
        let id = doc.viewport?.window?.windowNumber ?? 0
        log("step \(String(format: "%02d", step))-\(name) window-id \(id)")
        let ack = dir.appendingPathComponent(String(format: "ack-%02d", step))
        _ = await wait(15) { FileManager.default.fileExists(atPath: ack.path) }
    }

    private func finish() {
        log("done, \(failures) failure(s)")
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { NSApp.terminate(nil) }
    }

    // MARK: Synthesized input

    private func event(_ type: NSEvent.EventType, at canvas: CGPoint, in v: DocumentViewportView) -> NSEvent? {
        guard let w = v.window else { return nil }
        let p = v.convert(v.viewPoint(canvas: canvas), to: nil)
        return NSEvent.mouseEvent(with: type, location: p, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
                                  windowNumber: w.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: 1)
    }

    /// A horizontal drag at ~120 Hz; `during` runs once half way (the stroke still open).
    private func stroke(_ doc: DocumentController, _ v: DocumentViewportView, y: Double, from x0: Double, to x1: Double,
                        during: (() async -> Void)? = nil) async {
        let (w, h) = (Double(doc.info.width), Double(doc.info.height))
        let pts = (0...60).map { i in CGPoint(x: w * (x0 + (x1 - x0) * Double(i) / 60), y: h * y) }
        if let e = event(.leftMouseDown, at: pts[0], in: v) { v.mouseDown(with: e) }
        for (i, p) in pts.dropFirst().enumerated() {
            if let e = event(.leftMouseDragged, at: p, in: v) { v.mouseDragged(with: e) }
            await pause(1.0 / 120)
            if i == 30 { await during?() }
        }
        if let e = event(.leftMouseUp, at: pts[pts.count - 1], in: v) { v.mouseUp(with: e) }
        await tools.idle()
        await pause(0.2)
    }

    private func brush(_ tool: DocumentTool, _ color: ToolColor, size: Float) {
        tools.select(tool)
        tools.colors.foreground = color
        var b = tools.currentBrush
        b.size = size
        b.hardness = 1
        b.opacity = 1
        b.flow = 1
        b.symmetry = .none
        tools.currentBrush = b
    }

    private func label(_ d: DocumentController) -> String? { d.history.first { $0.id == d.info.historyHead }?.label }

    private func revision(_ id: UInt64) -> UInt64? {
        channels.records.first { $0.id == id }?.revision
    }

    /// Selection bounds after loading channel `id` (the load is undone again).
    private func channelBounds(_ doc: DocumentController, _ id: UInt64) -> CanvasRect? {
        guard let b = channels.backend(doc) else { return nil }
        _ = doc.run("Load Selection") { try b.loadSelectionChannel(id: id, op: .replace, invert: false) }
        let r = doc.marquee
        doc.undo()
        return r
    }

    // MARK: Run

    func run() async {
        let ws = model.documents
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        ws.newDocument(NewDocumentSettings(width: 1200, height: 800))
        guard await wait(60, { ws.current?.info.width == 1200 && ws.current?.viewport != nil }), let doc = ws.current,
              let v = doc.viewport, let t = doc.backend as? any DocumentToolsBackend else {
            log("FAIL no document / viewport: \(model.statusMessage ?? "")")
            return finish()
        }
        channels.attach(ws)
        tools.attach(ws)
        _ = await wait(20) { doc.lastFrame != nil }
        v.zoomToFit()
        log("document \(doc.info.width) × \(doc.info.height) px, \(doc.info.backend)")
        guard let layer = doc.layers.first(where: { $0.kind == .pixel })?.id else { log("FAIL no pixel layer"); return finish() }
        doc.select(layer)
        // An opaque layer so the composite is not blank.
        _ = doc.run("Fill") { try t.fillSelection(layer: layer, fill: .color(ToolColor(r: 0.2, g: 0.45, b: 0.8)), opacity: 1) }
        let probe = CanvasPoint(x: 600, y: 400)
        let composite = try? t.sampleColor(at: probe, sampleAll: true, layer: nil, radius: 0)
        let size = Float(60)

        // 491. Quick Mask: Q, paint black, Q → the painted band is masked out of the selection.
        tools.deselect()
        channels.toggleQuickMask()
        let quick = channels.quickMask[doc.id]
        check("Quick Mask on", quick != nil, model.statusMessage ?? "")
        brush(.brush, .black, size: size)
        var rows = doc.history.count
        await stroke(doc, v, y: 0.5, from: 0.2, to: 0.8)
        check("Quick Mask stroke is one Brush Tool row", doc.history.count == rows + 1 && label(doc) == "Brush Tool",
              "\(doc.history.map(\.label))")
        check("Quick Mask stroke leaves the composite", (try? t.sampleColor(at: probe, sampleAll: true, layer: nil, radius: 0)) == composite)
        await mark("quick-mask-painted", doc)
        channels.toggleQuickMask()
        check("Quick Mask off", channels.quickMask[doc.id] == nil && doc.marquee != nil, "\(String(describing: doc.marquee))")
        await tools.idle()
        _ = await wait(5) { !tools.outline.isEmpty }
        check("the selection has the painted hole (two outline loops or more)", tools.outline.count >= 2, "\(tools.outline.count) loops")
        await mark("quick-mask-selection", doc)
        tools.deselect()

        // 492. Paint white into a saved alpha channel, its eye on, targeted as a panel click does.
        channels.newChannel()
        guard let alpha = channels.selectedChannel, let row = channels.rows(doc).first(where: { $0.channelID == alpha }) else {
            log("FAIL New Channel made nothing: \(model.statusMessage ?? "")"); return finish()
        }
        if !row.visible { channels.toggleVisible(row) }
        tools.targetChannel(alpha, in: doc)
        brush(.brush, .white, size: size)
        let rev0 = revision(alpha)
        var live: UInt64?
        rows = doc.history.count
        await stroke(doc, v, y: 0.3, from: 0.1, to: 0.6) { [self] in
            await pause(0.3)
            live = revision(alpha)
        }
        check("channel thumbnail revision moves while the stroke is live", live != nil && live != rev0, "\(String(describing: rev0)) → \(String(describing: live))")
        check("channel stroke is one Brush Tool row", doc.history.count == rows + 1 && label(doc) == "Brush Tool",
              "\(doc.history.map(\.label))")
        let painted = channelBounds(doc, alpha)
        check("channel holds the stroke", painted.map { $0.x <= 110 && $0.x + $0.width >= 730 && $0.height < 120 } ?? false,
              "\(String(describing: painted))")
        check("RGB composite unchanged", (try? t.sampleColor(at: probe, sampleAll: true, layer: nil, radius: 0)) == composite)
        log("status: \(model.statusMessage ?? "")")
        await mark("alpha-painted", doc)

        // 493. Eraser on the channel (toward 0): erase the right part of the stroke.
        brush(.eraser, .white, size: size * 2)
        await stroke(doc, v, y: 0.3, from: 0.35, to: 0.7)
        check("eraser row", label(doc) == "Eraser", "\(doc.history.map(\.label))")
        let erased = channelBounds(doc, alpha)
        check("eraser shrinks the channel", erased.map { $0.x + $0.width < 450 } ?? false, "\(String(describing: erased))")
        await mark("alpha-erased", doc)

        // 494. Undo per stroke.
        let beforeUndo = revision(alpha)
        doc.undo()
        channels.reload(doc, force: true)
        check("undo reverts the eraser stroke", revision(alpha) != beforeUndo && label(doc) == "Brush Tool", "\(String(describing: label(doc)))")
        doc.redo()
        channels.reload(doc, force: true)
        check("redo", label(doc) == "Eraser")

        // 495. A selection clips channel paint.
        _ = doc.run("Rectangular Marquee") {
            try t.selectMarquee(.rect, rect: CGRect(x: 0, y: 500, width: 600, height: 300), feather: 0, antialias: false, op: .replace)
        }
        brush(.brush, .white, size: size)
        await stroke(doc, v, y: 0.8, from: 0.1, to: 0.9)
        tools.deselect()
        let clipped = channelBounds(doc, alpha)
        check("selection clips channel paint", clipped.map { $0.x + $0.width <= 600 && $0.y + $0.height >= 660 } ?? false,
              "\(String(describing: clipped))")
        await mark("alpha-clipped", doc)

        // 496. RGB click: strokes paint the layer again.
        tools.targetChannel(nil, in: doc)
        brush(.brush, ToolColor(r: 1, g: 0, b: 0), size: size)
        await stroke(doc, v, y: 0.5, from: 0.45, to: 0.55)
        let red = try? t.sampleColor(at: probe, sampleAll: true, layer: nil, radius: 0)
        check("after RGB the brush paints pixels", red.map { $0.r > 0.99 && $0.g < 0.01 } ?? false, "\(String(describing: red))")
        doc.undo()
        check("composite back", (try? t.sampleColor(at: probe, sampleAll: true, layer: nil, radius: 0)) == composite)

        // 497–499. Save as .tessera-doc and PSD, reopen, the painted channel is back.
        let name = channels.records.first { $0.id == alpha }?.name ?? ""
        let expected = channelBounds(doc, alpha)
        var current = doc
        for ext in ["tessera-doc", "psd"] {
            let url = dir.appendingPathComponent("ChannelPaintSelfTest.\(ext)")
            try? FileManager.default.removeItem(at: url)
            let saved = ws.write(current, to: url)
            check("save \(ext)", saved, model.statusMessage ?? "")
            ws.discard(current)
            guard saved else { continue }
            ws.open(url)
            guard await wait(60, { ws.current != nil && ws.opening == nil }), let r = ws.current else {
                check("\(ext) reopens", false, model.statusMessage ?? ""); continue
            }
            _ = await wait(10) { r.lastFrame != nil }
            channels.reload(r, force: true)
            let back = channels.records.first { $0.name == name }
            check("\(ext) keeps the painted channel", back != nil, "\(channels.records.map(\.name))")
            if let back {
                let bounds = channelBounds(r, back.id)
                check("\(ext) channel samples survive", bounds == expected, "\(String(describing: bounds)) vs \(String(describing: expected))")
            }
            await mark("reopen-\(ext)", r)
            current = r
        }
        finish()
    }
}
