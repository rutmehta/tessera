import AppKit
import SwiftUI
import TesseraCore

/// `--liquify-selftest=<dir>` (test aid, WP B5-13): background-only verification of ACCEPTANCE §B5-13 (400–419) on the
/// engine. Launch with `open -g -n … --args --open-document <image> --liquify-selftest=<dir>`; the app is never
/// activated. It drives the same paths the UI takes: Filter ▸ Liquify… and the workspace canvas (synthesized mouse
/// events into the canvas view), the Content-Aware Move tool (synthesized events into the viewport), and the options.
/// Each step prints `liquify-selftest: step <n> <name> window <id>` and, for screenshots, writes `<dir>/<name>.req`
/// holding the window number and waits for `<name>.png` (a watcher runs `screencapture -x -o -l <id>`). Checks print
/// `check <name> ok|FAIL`; the run ends with `done, <n> failure(s)` and quits.
@MainActor
final class LiquifySelfTest {
    private let model: AppModel
    private let dir: URL
    private var failures = 0
    private var step = 0
    private static var started = false

    static func startIfRequested() {
        guard !started else { return }
        guard let a = CommandLine.arguments.first(where: { $0.hasPrefix("--liquify-selftest=") }) else { return }
        started = true
        let dir = URL(fileURLWithPath: (String(a.dropFirst("--liquify-selftest=".count)) as NSString).expandingTildeInPath)
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) {
            MainActor.assumeIsolated {
                let t = LiquifySelfTest(model: AppModel.shared, dir: dir)
                Task { @MainActor in await t.run() }
            }
        }
    }

    private init(model: AppModel, dir: URL) {
        self.model = model
        self.dir = dir
    }

    // MARK: Plumbing

    /// A line starting "FAIL" (an early exit: no library, no document, …) counts as a failure, so the
    /// closing `done, <n> failure(s)` is never a silent 0 for a run that did not happen.
    private func log(_ s: String) {
        if s.hasPrefix("FAIL") { failures += 1 }
        FileHandle.standardError.write(Data("liquify-selftest: \(s)\n".utf8))
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
            await pause(0.03)
        }
        return true
    }

    private var docWindow: NSWindow? {
        model.mainWindow ?? NSApp.windows.first { !($0 is NSPanel) && $0.isVisible && $0.contentView != nil && $0.sheetParent == nil }
    }

    /// A screenshot of `window` (the sheet when one is up) through the watcher; never raises anything.
    private func shot(_ name: String, sheet: Bool = false) async {
        step += 1
        await pause(0.6)
        guard let main = docWindow else { log("step \(step) \(name) (no window)"); return }
        let w = sheet ? (main.attachedSheet ?? main) : main
        w.displayIfNeeded()
        await pause(0.3)
        log("step \(step) \(name) window \(w.windowNumber) \(Int(w.frame.width)) × \(Int(w.frame.height))")
        let png = dir.appendingPathComponent("\(name).png")
        try? FileManager.default.removeItem(at: png)
        try? "\(w.windowNumber)".write(to: dir.appendingPathComponent("\(name).req"), atomically: true, encoding: .utf8)
        if !(await wait(20) { FileManager.default.fileExists(atPath: png.path) }) { log("shot \(name): no watcher") }
    }

    private func mouse(_ type: NSEvent.EventType, _ p: CGPoint, in v: NSView, option: Bool = false) -> NSEvent? {
        guard let w = v.window else { return nil }
        let q = v.convert(p, to: nil)
        return NSEvent.mouseEvent(with: type, location: q, modifierFlags: option ? .option : [], timestamp: ProcessInfo.processInfo.systemUptime,
                                  windowNumber: w.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: 1)
    }

    /// A drag in `v`'s own coordinates.
    private func drag(_ pts: [CGPoint], in v: NSView, option: Bool = false, hold: Double = 0) async {
        guard let f = pts.first, let l = pts.last else { return }
        if let e = mouse(.leftMouseDown, f, in: v, option: option) { v.mouseDown(with: e) }
        for p in pts.dropFirst() {
            if let e = mouse(.leftMouseDragged, p, in: v, option: option) { v.mouseDragged(with: e) }
            await pause(1.0 / 90)
        }
        if hold > 0 { await pause(hold) }
        if let e = mouse(.leftMouseUp, l, in: v, option: option) { v.mouseUp(with: e) }
    }

    private func line(_ a: CGPoint, _ b: CGPoint, _ n: Int = 24) -> [CGPoint] {
        (0...n).map { i in let t = Double(i) / Double(n); return CGPoint(x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t) }
    }

    private func canvasView() -> LiquifyCanvasView? {
        func find(_ v: NSView) -> LiquifyCanvasView? {
            if let c = v as? LiquifyCanvasView { return c }
            for s in v.subviews { if let c = find(s) { return c } }
            return nil
        }
        return docWindow?.attachedSheet?.contentView.flatMap(find)
    }

    private func finish() {
        log("done, \(failures) failure(s)")
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) { NSApp.terminate(nil) }
    }

    // MARK: Run

    func run() async {
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let ws = model.documents
        // `--open-document` may fire before the workspace is attached: open it here if nothing arrived.
        if !(await wait(20, { ws.current != nil || ws.opening != nil })),
           let i = CommandLine.arguments.firstIndex(of: "--open-document"), i + 1 < CommandLine.arguments.count {
            _ = await wait(60) { ws.app != nil }
            ws.open(URL(fileURLWithPath: (CommandLine.arguments[i + 1] as NSString).expandingTildeInPath))
        }
        guard await wait(120, { ws.current?.viewport != nil && ws.opening == nil }), let doc = ws.current, let v = doc.viewport else {
            log("FAIL no document (launch with --open-document <image>): \(model.statusMessage ?? "") current \(ws.current != nil) "
                + "viewport \(ws.current?.viewport != nil) mode \(model.viewMode) window \(model.mainWindow != nil)"
                + " windows \(NSApp.windows.map { "\(type(of: $0)) visible \($0.isVisible)" })")
            return finish()
        }
        _ = await wait(20) { doc.lastFrame != nil }
        if let w = docWindow, w.frame.width != 1440 {
            // 1440-pt window for the inspector evidence (resizing never activates or raises it).
            w.setFrame(NSRect(x: w.frame.minX, y: w.frame.minY, width: 1440, height: 900), display: true)
        }
        guard let layer = doc.layers.first(where: { $0.kind == .pixel }) else { log("FAIL no pixel layer"); return finish() }
        doc.select(layer.id)
        let (W, H) = (Double(doc.info.width), Double(doc.info.height))
        log("document \(doc.info.width) × \(doc.info.height) px, \(doc.info.backend)")
        let L = DocumentLiquify.shared
        guard let lb = DocumentLiquify.backend(doc), let tools = doc.backend as? any DocumentToolsBackend,
              let filters = doc.backend as? any DocumentFiltersBackend, let retouch = doc.backend as? any DocumentRetouchBackend else {
            log("FAIL backends"); return finish()
        }

        // 400. Filter ▸ Liquify… opens on the selected layer.
        L.open(doc)
        guard await wait(60, { L.workspace != nil && docWindow?.attachedSheet != nil }), let m = L.workspace else {
            check("400 Liquify opens", false, "no workspace"); return finish()
        }
        _ = await wait(10) { m.image != nil && canvasView() != nil }
        check("400 opens on the selected pixel layer at canvas size",
              m.info.layer == layer.id && Double(m.info.width) == W && Double(m.info.height) == H && !m.info.smartObject,
              "\(m.info)")
        log("preview \(m.previewNote)")
        await shot("400-liquify-open", sheet: true)
        guard let c = canvasView() else { check("canvas view", false); return finish() }
        let t = m.info.token
        func src(_ x: Double, _ y: Double) -> CGPoint { m.view.viewPoint(source: CGPoint(x: W * x, y: H * y)) }
        func meshNow() -> LiquifyMeshData? { try? lb.liquifyMesh(token: t) }
        func dispAt(_ x: Double, _ y: Double) -> (Double, Double) {
            guard let me = meshNow() else { return (0, 0) }
            let cc = min(Int((W * x / Double(me.cellSize)).rounded()), me.columns - 1)
            let rr = min(Int((H * y / Double(me.cellSize)).rounded()), me.rows - 1)
            let i = rr * me.columns + cc
            return (Double(me.displacement[2 * i]), Double(me.displacement[2 * i + 1]))
        }

        // 401. Forward Warp follows the drag (inverse map: output samples upstream).
        m.tool = .forwardWarp
        m.brush.setSize(W / 8)
        await drag(line(src(0.25, 0.3), src(0.40, 0.3)), in: c)
        await m.idle()
        let d = dispAt(0.36, 0.3)
        check("401 forward warp moves content along the drag", d.0 < -1, "displacement \(d)")
        await shot("401-forward-warp", sheet: true)

        // 402. Twirl CW / CCW, Pucker, Bloat, Push Left are distinct.
        var meshes: [LiquifyToolKind: LiquifyMeshData] = [:]
        for (i, tool) in [LiquifyToolKind.twirlClockwise, .twirlCounterClockwise, .pucker, .bloat, .pushLeft].enumerated() {
            let before = meshNow()
            m.tool = tool
            let x = 0.15 + 0.17 * Double(i)
            let pts = tool == .pushLeft ? line(src(x - 0.04, 0.7), src(x + 0.04, 0.7)) : Array(repeating: src(x, 0.7), count: 2)
            await drag(pts, in: c, hold: tool == .pushLeft ? 0 : 0.5)
            await m.idle()
            if let b = before, let a = meshNow() { meshes[tool] = LiquifyMeshData(columns: a.columns, rows: a.rows, cellSize: a.cellSize,
                displacement: zip(a.displacement, b.displacement).map { $0 - $1 }, freeze: a.freeze, maxDisplacement: 0) }
        }
        let deltas = Array(meshes.values.map(\.displacement))
        let distinct = deltas.count == 5 && (0..<5).allSatisfy { i in (i + 1..<5).allSatisfy { deltas[i] != deltas[$0] } }
        check("402 twirl cw/ccw, pucker, bloat, push left are distinct", distinct)
        if let cw = meshes[.twirlClockwise], let ccw = meshes[.twirlCounterClockwise] {
            check("402 twirl directions differ", cw.displacement != ccw.displacement)
        }
        await shot("402-tools", sheet: true)

        // 403. Reconstruct (50 %) and Restore All work against the original.
        let before403 = meshNow()?.maxDisplacement ?? 0
        m.reconstructAmount = 50
        m.reconstructAll()
        await m.idle()
        let after403 = meshNow()?.maxDisplacement ?? 0
        check("403 reconstruct halves the distortion", after403 < before403 * 0.6 && after403 > 0, "\(before403) → \(after403)")
        m.restoreAll()
        await m.idle()
        check("403 restore all removes it", (meshNow()?.maxDisplacement ?? 1) == 0)
        await shot("403-restored", sheet: true)

        // 404. Freeze protects; thaw restores editing; the mask overlay follows zoom / pan.
        m.tool = .freeze
        m.showMask = true
        await drag(line(src(0.45, 0.2), src(0.45, 0.8)), in: c)
        await m.idle()
        m.tool = .forwardWarp
        await drag(line(src(0.35, 0.5), src(0.55, 0.5)), in: c)
        await m.idle()
        let frozen = dispAt(0.45, 0.5)
        check("404 frozen area is not deformed", abs(frozen.0) < 0.01 && abs(frozen.1) < 0.01, "\(frozen)")
        m.zoom(by: 2, around: CGPoint(x: c.bounds.midX, y: c.bounds.midY))
        m.pan(dx: 60, dy: -30)
        await shot("404-freeze-mask-zoomed", sheet: true)
        m.thawAll()
        await m.idle()
        await drag(line(src(0.40, 0.5), src(0.50, 0.5)), in: c)
        await m.idle()
        let thawed = dispAt(0.45, 0.5)
        check("404 thaw restores editing", abs(thawed.0) > 0.5, "\(thawed)")
        m.fit(in: c.bounds.size)

        // 405. Brush controls and mesh display.
        m.brush.setSize(W / 12); m.brush.setDensity(20); m.brush.setPressure(40); m.brush.setRate(30)
        m.tool = .bloat
        let pre405 = meshNow()?.displacement
        await drag(Array(repeating: src(0.8, 0.35), count: 2), in: c, hold: 0.3)
        await m.idle()
        check("405 lower pressure/rate still deform", meshNow()?.displacement != pre405)
        m.showMesh = true
        m.meshSize = .small
        await m.idle()
        await shot("405-mesh-small", sheet: true)
        m.meshSize = .large
        await shot("405-mesh-large", sheet: true)
        m.showMesh = false
        m.brush = LiquifyBrushSettings(size: W / 8)

        // 406. Before / after does not touch the document or history.
        let hist406 = doc.history.count
        let rev406 = doc.layers.first { $0.id == layer.id }?.revision
        m.showOriginal = true
        await m.idle()
        await shot("406-before", sheet: true)
        m.showOriginal = false
        await m.idle()
        check("406 before/after leaves document and history", doc.history.count == hist406
              && doc.layers.first { $0.id == layer.id }?.revision == rev406)

        // 407. Apply is one undoable edit.
        var applied: Result<DocumentChange, Error>?
        m.onApplied = { applied = $0 }
        m.output = .currentLayer
        m.apply()
        _ = await wait(120) { applied != nil }
        check("407 apply is one Liquify node", (try? applied?.get()) != nil && doc.history.count == hist406 + 1
              && doc.history.last?.label == "Liquify", "\(doc.history.map(\.label).suffix(3))")
        check("407 workspace closed", await wait(5) { L.workspace == nil && docWindow?.attachedSheet == nil })
        await shot("407-applied")
        doc.run("Undo") { try doc.backend.undo() }
        check("407 undo", doc.history.last(where: { $0.id == doc.info.historyHead })?.label != "Liquify")
        doc.run("Redo") { try doc.backend.redo() }
        check("407 redo", doc.history.last(where: { $0.id == doc.info.historyHead })?.label == "Liquify")

        // 409. Cancelling a busy apply: no late result, no history change.
        L.open(doc)
        guard await wait(60, { L.workspace != nil }), let m9 = L.workspace else { check("409 reopen", false); return finish() }
        _ = await wait(10) { canvasView() != nil }
        if let c9 = canvasView() {
            m9.tool = .forwardWarp
            m9.brush.setSize(W / 2)
            await drag(line(m9.view.viewPoint(source: CGPoint(x: W * 0.1, y: H * 0.5)), m9.view.viewPoint(source: CGPoint(x: W * 0.9, y: H * 0.5))), in: c9)
            await m9.idle()
        }
        let hist409 = doc.history.count
        var late: Result<DocumentChange, Error>?
        m9.onApplied = { late = $0 }
        m9.apply()
        let t0 = Date()
        m9.cancel()
        let back = Date().timeIntervalSince(t0)
        _ = await wait(30) { late != nil }
        await pause(0.5)
        check("409 cancel returns at once", back < 0.25, String(format: "%.3f s", back))
        check("409 no late history change", doc.history.count == hist409, "\(doc.history.count) vs \(hist409); late \(String(describing: late))")
        await shot("409-cancelled")

        // 410–416: Content-Aware Move on a pixel layer.
        do {
        let (doc2, v2, tools2, pixel) = (doc, v, tools, layer)
        doc2.select(pixel.id)
        let sel = CGRect(x: W * 0.30, y: H * 0.40, width: W * 0.12, height: H * 0.14)
        _ = doc2.run("Marquee") { try tools2.selectMarquee(.rect, rect: sel, feather: 0, antialias: false, op: .replace) }
        _ = await wait(5) { doc2.marquee != nil }
        let cam = DocumentContentAware.shared
        cam.activate()
        check("410 Content-Aware Move on with a selection", cam.active)
        let hist410 = doc2.history.count
        var pv: Result<ContentAwarePreviewInfo, Error>?
        cam.onPreview = { pv = $0 }
        let from = CGPoint(x: sel.midX, y: sel.midY)
        let to = CGPoint(x: sel.midX + W * 0.30, y: sel.midY + H * 0.05)
        await drag(line(v2.viewPoint(canvas: from), v2.viewPoint(canvas: to), 20), in: v2)
        _ = await wait(300) { pv != nil && !cam.computing }
        let expect = (Int32((to.x - from.x).rounded()), Int32((to.y - from.y).rounded()))
        check("411 preview computed outside the selection", (try? pv?.get()) != nil, "\(String(describing: pv))")
        check("415 integer document-pixel offset matches the drag", cam.offset == expect, "\(cam.offset) vs \(expect)")
        check("416 preview adds no history", doc2.history.count == hist410)
        await shot("411-move-preview")

        // 412. Extend keeps the original.
        pv = nil
        cam.options.mode = .extend
        _ = await wait(300) { pv != nil && !cam.computing }
        check("412 extend preview", (try? pv?.get()) != nil && cam.session?.mode == .extend)
        await shot("412-extend-preview")

        // 414. Settings re-run the preview; the same seed repeats (engine-tested exactly; here: it runs).
        pv = nil
        cam.options.seed = 7
        _ = await wait(300) { pv != nil && !cam.computing }
        check("414 seed change re-previews", (try? pv?.get()) != nil)

        // 415. At 200 %, a drag moves by the same document pixels.
        let before415 = cam.offset
        let zoomFrom0 = CGPoint(x: sel.midX + Double(before415.dx), y: sel.midY + Double(before415.dy))
        if let ze = mouse(.leftMouseDown, v2.viewPoint(canvas: zoomFrom0), in: v2) { v2.zoomStep(in: true, at: ze) }
        await pause(0.5)
        let zoomFrom = CGPoint(x: sel.midX + Double(before415.dx), y: sel.midY + Double(before415.dy))
        pv = nil
        await drag(line(v2.viewPoint(canvas: zoomFrom), v2.viewPoint(canvas: CGPoint(x: zoomFrom.x + 10, y: zoomFrom.y - 6)), 10), in: v2)
        _ = await wait(300) { pv != nil && !cam.computing }
        check("415 zoomed drag adds exact pixels", cam.offset == (before415.dx + 10, before415.dy - 6), "\(cam.offset)")
        await shot("415-zoomed")

        // 416. Apply is one step; a fresh move cancelled with Esc changes nothing.
        var ap: Result<DocumentChange, Error>?
        cam.onApplied = { ap = $0 }
        cam.apply()
        _ = await wait(300) { ap != nil }
        check("416 apply is one node", doc2.history.count == hist410 + 1 && doc2.history.last?.label == "Content-Aware Extend",
              "\(doc2.history.map(\.label).suffix(3))")
        await shot("416-applied")
        cam.options.mode = .move
        let hist416 = doc2.history.count
        pv = nil
        await drag(line(v2.viewPoint(canvas: from), v2.viewPoint(canvas: CGPoint(x: from.x - W * 0.1, y: from.y)), 10), in: v2)
        _ = await wait(300) { pv != nil && !cam.computing }
        cam.cancel()
        await pause(0.5)
        check("416 cancel leaves history and selection", doc2.history.count == hist416 && doc2.marquee != nil)

        // 413 is engine-tested (fractional feather applied once); here a feathered selection previews and applies.
        _ = doc2.run("Marquee") { try tools2.selectMarquee(.ellipse, rect: sel, feather: 6, antialias: true, op: .replace) }
        pv = nil
        await drag(line(v2.viewPoint(canvas: from), v2.viewPoint(canvas: CGPoint(x: from.x, y: from.y + H * 0.25)), 10), in: v2)
        _ = await wait(300) { pv != nil && !cam.computing }
        check("413 feathered selection previews", (try? pv?.get()) != nil)
        await shot("413-feathered")
        cam.cancel()

        // 417. Locks, no selection: no partial result.
        _ = doc2.run("Lock") { try doc2.backend.setLocks(id: pixel.id, locks: LayerLockFlags(pixels: true)) }
        let hist417 = doc2.history.count
        await drag(line(v2.viewPoint(canvas: from), v2.viewPoint(canvas: CGPoint(x: from.x + 40, y: from.y)), 6), in: v2)
        await pause(0.5)
        check("417 locked layer refuses the move", cam.session == nil && doc2.history.count == hist417)
        L.open(doc2)
        await pause(1.0)
        check("417 locked layer refuses Liquify", L.workspace == nil)
        _ = doc2.run("Unlock") { try doc2.backend.setLocks(id: pixel.id, locks: LayerLockFlags()) }
        _ = doc2.run("Deselect") { try doc2.backend.clearSelection() }
        await drag(line(v2.viewPoint(canvas: from), v2.viewPoint(canvas: CGPoint(x: from.x + 40, y: from.y)), 6), in: v2)
        check("417 no selection: nothing starts", cam.session == nil)
        cam.deactivate()

        // 419. B5-09 regressions and the 1440-pt window.
        _ = doc2.run("Marquee") { try tools2.selectMarquee(.rect, rect: CGRect(x: W * 0.6, y: H * 0.1, width: W * 0.05, height: H * 0.05),
                                                         feather: 0, antialias: false, op: .replace) }
        let h419 = doc2.history.count
        let rm = Result { try retouch.removeSelection(layer: pixel.id, engine: .patchMatch, paramsJson: #"{"dilation":0}"#) }
        if case .success(let o) = rm { _ = doc2.run("Remove") { o.change } }
        check("419 Remove (PatchMatch) still one node", (try? rm.get())?.backend == "PatchMatch" && doc2.history.count == h419 + 1)
        let caf = Result { try retouch.contentAwareFill(layer: pixel.id, paramsJson: "{}") }
        if case .success(let o) = caf { _ = doc2.run("Content-Aware Fill") { o.change } }
        check("419 Content-Aware Fill still one node", (try? caf.get()) != nil && doc2.history.count == h419 + 2)
        let neural = Result { try retouch.neuralFilter(layer: pixel.id, kind: .colorize, paramsJson: "{}", output: .currentLayer) }
        if case .failure(let e) = neural {
            check("419 neural missing weights named", e.localizedDescription.contains("filters/ddcolor"), e.localizedDescription)
        } else { check("419 neural missing weights named", false, "applied without weights?") }
        }

        // 408. Smart filter output preserves the source; re-edit replaces without duplicating.
        _ = doc.run("Convert to Smart Object") { try filters.convertForSmartFilters(layer: layer.id) }
        doc.select(layer.id)
        L.open(doc)
        guard await wait(120, { L.workspace != nil }), let m8 = L.workspace else { check("408 smart open", false); return finish() }
        _ = await wait(10) { canvasView() != nil }
        check("408 smart object opens as smart", m8.info.smartObject && m8.output == .smartFilter)
        if let c8 = canvasView() {
            m8.tool = .pucker
            await drag(Array(repeating: m8.view.viewPoint(source: CGPoint(x: W * 0.5, y: H * 0.5)), count: 2), in: c8, hold: 0.6)
            await m8.idle()
        }
        var a8: Result<DocumentChange, Error>?
        m8.onApplied = { a8 = $0 }
        m8.apply()
        _ = await wait(180) { a8 != nil }
        var rows = (try? filters.smartFilters(layer: layer.id)) ?? []
        check("408 one Liquify smart filter", rows.count == 1 && rows.first?.filterId == "liquify", "\(rows.map(\.filterId))")
        if let row = rows.first {
            L.editSmartFilter(doc, layer: layer.id, row: row)
            guard await wait(120, { L.workspace != nil }), let r8 = L.workspace else { check("408 re-edit opens", false); return finish() }
            _ = await wait(10) { canvasView() != nil && r8.image != nil }
            check("408 re-edit loads the stored mesh", r8.info.stageIndex == 0 && r8.info.edited)
            await shot("408-re-edit", sheet: true)
            if let c8 = canvasView() {
                r8.tool = .forwardWarp
                await drag(line(r8.view.viewPoint(source: CGPoint(x: W * 0.2, y: H * 0.2)), r8.view.viewPoint(source: CGPoint(x: W * 0.3, y: H * 0.25))), in: c8)
                await r8.idle()
            }
            var a = Optional<Result<DocumentChange, Error>>.none
            r8.onApplied = { a = $0 }
            r8.apply()
            _ = await wait(180) { a != nil }
            rows = (try? filters.smartFilters(layer: layer.id)) ?? []
            check("408 re-edit replaces in place", rows.count == 1, "\(rows.map(\.filterId))")
        }
        await shot("408-smart-filter")

        // 418. Native save / reopen keeps the smart filter and its appearance.
        let file = dir.appendingPathComponent("LiquifySelfTest.tessera-doc")
        try? FileManager.default.removeItem(at: file)
        check("418 save", ws.write(doc, to: file))
        ws.discard(doc)
        ws.open(file)
        guard await wait(120, { ws.current?.viewport != nil && ws.opening == nil }), let doc2 = ws.current, let v2 = doc2.viewport else {
            check("418 reopen", false); return finish()
        }
        _ = await wait(20) { doc2.lastFrame != nil }
        guard let so = doc2.layers.first(where: { $0.kind == .smartObject }) else { check("418 smart object reopened", false); return finish() }
        let rows2 = ((doc2.backend as? any DocumentFiltersBackend).flatMap { try? $0.smartFilters(layer: so.id) }) ?? []
        check("418 reopened smart filter", rows2.first?.filterId == "liquify", "\(rows2.map(\.filterId))")
        L.editSmartFilter(doc2, layer: so.id, row: rows2.first ?? SmartFilterRow(index: 0, filterId: "", name: "", enabled: true,
                                                                                  filterJson: "", opacity: 1, blendMode: "normal", hasMask: false))
        if await wait(120, { L.workspace != nil }), let r = L.workspace {
            _ = await wait(10) { r.image != nil }
            check("418 reopened stage opens edited", r.info.edited && r.info.stageIndex == 0)
            await shot("418-reopened-re-edit", sheet: true)
            r.cancel()
            await pause(0.5)
        } else { check("418 reopened stage opens", false) }
        await shot("418-reopened")
        _ = v2

        check("419 window is 1440 pt", Int(docWindow?.frame.width ?? 0) == 1440, "\(docWindow?.frame.width ?? 0)")
        await shot("419-inspector-1440")
        finish()
    }

}
