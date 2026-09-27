import AppKit
import ImageIO
import TesseraCore
import TesseraFFI
import UniformTypeIdentifiers

/// `--transform-selftest=<dir>` (test aid, WP B5-12): runs ACCEPTANCE §B5-12 (steps 380–399) through the
/// app's own paths without ever taking focus: it writes its test images into `<dir>`, opens them in the
/// workspace, drives Edit ▸ Transform through `DocumentTransforms` (the menu items call the same entry
/// points), synthesizes mouse events into the viewport for drags, clicks, pins and handles, and reads the
/// engine back for checks. Screenshots: writes `<dir>/<name>.req` with this window's number and waits
/// for `<name>.png`, which a watcher captures with `screencapture -x -o -l <window>`. Checks print
/// `transform-selftest: check <name> ok|FAIL`; the run ends with `done, <n> failure(s)` and quits.
/// `TRANSFORM_SELFTEST_20MP=0` skips the 20 MP latency run.
@MainActor
final class TransformSelfTest {
    private let workspace: DocumentWorkspace
    private let dir: URL
    private var failures = 0
    private var t: DocumentTransforms { .shared }
    private static var started = false

    static func startIfRequested(_ workspace: DocumentWorkspace) {
        guard !started else { return }
        let args = CommandLine.arguments
        let path: String
        if let a = args.first(where: { $0.hasPrefix("--transform-selftest=") }) {
            path = String(a.dropFirst("--transform-selftest=".count))
        } else if let i = args.firstIndex(of: "--transform-selftest"), i + 1 < args.count {
            path = args[i + 1]
        } else { return }
        started = true
        DocumentTransforms.shared.workspace = workspace
        let test = TransformSelfTest(workspace: workspace, dir: URL(fileURLWithPath: (path as NSString).expandingTildeInPath))
        Task { @MainActor in await test.run() }
    }

    private init(workspace: DocumentWorkspace, dir: URL) {
        self.workspace = workspace
        self.dir = dir
    }

    // MARK: Plumbing

    private func log(_ s: String) { FileHandle.standardError.write(Data("transform-selftest: \(s)\n".utf8)) }

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

    private var doc: DocumentController? { workspace.current }
    private var viewport: DocumentViewportView? { doc?.viewport }
    private var engine: DocumentSession? { (doc?.backend as? EngineDocumentBackend)?.session }

    private func settle(_ extra: Double = 0.25) async {
        await t.idle()
        await DocumentTools.shared.idle()
        // The frame for the newest epoch (exact stage renders can take seconds on large images).
        if let d = doc, let e = try? d.backend.info().epoch {
            _ = await wait(30) { (d.lastFrame?.epoch ?? 0) >= e }
        }
        await pause(extra)
    }

    private func shot(_ name: String) async {
        await settle(0.4)
        guard let w = viewport?.window else { return }
        w.displayIfNeeded()
        let png = dir.appendingPathComponent("\(name).png")
        try? FileManager.default.removeItem(at: png)
        try? "\(w.windowNumber)".write(to: dir.appendingPathComponent("\(name).req"), atomically: true, encoding: .utf8)
        let ok = await wait(60) { FileManager.default.fileExists(atPath: png.path) }
        log(String(format: "shot %@ window %ld %.0f × %.0f", name, w.windowNumber, w.frame.width, w.frame.height) + (ok ? "" : " (no watcher)"))
    }

    /// Keeps the window 1440 pt wide without bringing it forward.
    private func setWindowWidth(_ width: CGFloat) async {
        guard let win = viewport?.window, let screen = win.screen ?? NSScreen.main else { return }
        if win.styleMask.contains(.fullScreen) { win.toggleFullScreen(nil); await pause(1.5) }
        let top = screen.visibleFrame.maxY - CGFloat(40)
        let h = min(CGFloat(900), top - screen.visibleFrame.minY)
        win.setFrame(CGRect(x: screen.visibleFrame.minX, y: top - h, width: min(width, screen.frame.width), height: h), display: true)
        await pause(0.6)
    }

    private func mouse(_ type: NSEvent.EventType, view p: CGPoint, flags: NSEvent.ModifierFlags = []) -> NSEvent? {
        guard let v = viewport, let w = v.window else { return nil }
        let wp = v.convert(p, to: nil)
        return NSEvent.mouseEvent(with: type, location: wp, modifierFlags: flags, timestamp: ProcessInfo.processInfo.systemUptime,
                                  windowNumber: w.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: 1)
    }

    /// A drag between two viewport points in `steps` coalesced moves (`perStep` seconds apart).
    private func drag(_ a: CGPoint, _ b: CGPoint, flags: NSEvent.ModifierFlags = [], steps: Int = 12, perStep: Double = 1.0 / 60) async {
        guard let v = viewport, let d = mouse(.leftMouseDown, view: a, flags: flags) else { return }
        v.mouseDown(with: d)
        for i in 1...steps {
            let s = CGFloat(i) / CGFloat(steps)
            if let m = mouse(.leftMouseDragged, view: CGPoint(x: a.x + (b.x - a.x) * s, y: a.y + (b.y - a.y) * s), flags: flags) {
                v.mouseDragged(with: m)
            }
            await pause(perStep)
        }
        if let u = mouse(.leftMouseUp, view: b, flags: flags) { v.mouseUp(with: u) }
        await pause(0.05)
    }

    private func click(_ p: CGPoint, flags: NSEvent.ModifierFlags = []) async {
        guard let v = viewport, let d = mouse(.leftMouseDown, view: p, flags: flags), let u = mouse(.leftMouseUp, view: p, flags: flags) else { return }
        v.mouseDown(with: d)
        v.mouseUp(with: u)
        await pause(0.05)
    }

    private func key(code: UInt16, chars: String) -> Bool {
        guard let w = viewport?.window,
              let e = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
                                       windowNumber: w.windowNumber, context: nil, characters: chars,
                                       charactersIgnoringModifiers: chars, isARepeat: false, keyCode: code) else { return false }
        return DocumentTools.shared.handleKey(e)
    }

    /// Child pixels → viewport points.
    private func view(_ child: CGPoint) -> CGPoint {
        guard let v = viewport else { return child }
        return t.viewPoint(child, in: v)
    }

    private func history() -> Int { (try? doc?.backend.historyItems().count) ?? 0 }
    private func kind(_ id: DocLayerID) -> LayerKindTag? { doc?.node(id)?.kind }

    private func began(_ tag: AdvancedTransformTag) async -> Bool {
        t.begin(tag)
        let ok = await wait(20) { t.session != nil }
        await settle()
        if !ok { log("\(tag.title) did not start: \(t.status ?? "-")") }
        return ok
    }

    private func finished(_ body: () -> Void) async -> Result<DocumentChange?, Error>? {
        var result: Result<DocumentChange?, Error>?
        t.onFinished = { result = $0 }
        body()
        _ = await wait(120) { result != nil }
        t.onFinished = nil
        await settle()
        return result
    }

    // MARK: Test images

    /// A photo-like test card: warm ramp, a grid every 1/12, rings and a bar (warps are easy to read).
    private func writeCard(_ name: String, width: Int, height: Int) -> URL? {
        let url = dir.appendingPathComponent(name)
        if FileManager.default.fileExists(atPath: url.path) { return url }
        guard let space = CGColorSpace(name: CGColorSpace.sRGB),
              let ctx = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0, space: space,
                                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return nil }
        let (w, h) = (CGFloat(width), CGFloat(height))
        let colors = [CGColor(srgbRed: 0.85, green: 0.62, blue: 0.38, alpha: 1), CGColor(srgbRed: 0.22, green: 0.34, blue: 0.46, alpha: 1)]   // lint:allow (test image data)
        if let g = CGGradient(colorsSpace: space, colors: colors as CFArray, locations: [0, 1]) {
            ctx.drawLinearGradient(g, start: .zero, end: CGPoint(x: w, y: h), options: [])
        }
        ctx.setStrokeColor(CGColor(srgbRed: 0.97, green: 0.96, blue: 0.93, alpha: 1))   // lint:allow (test image data)
        ctx.setLineWidth(max(w / 400, 2))
        for i in 1..<12 {
            let x = w * CGFloat(i) / 12, y = h * CGFloat(i) / 12
            ctx.move(to: CGPoint(x: x, y: 0)); ctx.addLine(to: CGPoint(x: x, y: h))
            ctx.move(to: CGPoint(x: 0, y: y)); ctx.addLine(to: CGPoint(x: w, y: y))
        }
        ctx.strokePath()
        ctx.setLineWidth(max(w / 150, 4))
        for r in stride(from: CGFloat(0.08), through: 0.32, by: 0.08) {
            ctx.strokeEllipse(in: CGRect(x: w / 2 - h * r, y: h / 2 - h * r, width: 2 * h * r, height: 2 * h * r))
        }
        ctx.setFillColor(CGColor(srgbRed: 0.12, green: 0.1, blue: 0.09, alpha: 1))   // lint:allow (test image data)
        ctx.fill(CGRect(x: w * 0.1, y: h * 0.15, width: w * 0.2, height: h * 0.1))
        guard let img = ctx.makeImage(),
              let dest = CGImageDestinationCreateWithURL(url as CFURL, UTType.png.identifier as CFString, 1, nil) else { return nil }
        CGImageDestinationAddImage(dest, img, nil)
        return CGImageDestinationFinalize(dest) ? url : nil
    }

    private func open(_ url: URL) async -> DocumentController? {
        let before = workspace.documents.count
        workspace.open(url)
        guard await wait(180, { workspace.documents.count > before && workspace.current?.viewport != nil }) else { return nil }
        _ = await wait(30) { workspace.current?.lastFrame != nil }
        await pause(0.5)
        return workspace.current
    }

    // MARK: Run

    private func run() async {
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        log("waiting for the workspace")
        _ = await wait(90) { workspace.app != nil }
        guard let url = writeCard("card.png", width: 1600, height: 1000), let d = await open(url) else {
            log("FAIL could not open the test card"); return finish()
        }
        await setWindowWidth(1440)
        viewport?.zoomToFit()
        await pause(0.5)
        log("document \(d.info.width) × \(d.info.height), \(d.info.backend)")
        guard let photo = d.layers.first(where: { $0.kind == .pixel })?.id else { log("FAIL no pixel layer"); return finish() }
        d.select(photo)

        // 380: Edit ▸ Transform exposes the four commands.
        // SwiftUI fills menus lazily: ask each one to update, as opening it would.
        func refresh(_ m: NSMenu?) {
            guard let m else { return }
            m.delegate?.menuNeedsUpdate?(m)
            m.update()
            for i in m.items { refresh(i.submenu) }
        }
        refresh(NSApp.mainMenu?.item(withTitle: "Edit")?.submenu)
        let transformMenu = NSApp.mainMenu?.item(withTitle: "Edit")?.submenu?.item(withTitle: "Transform")?.submenu
        let titles = transformMenu?.items.map(\.title) ?? []
        if titles.isEmpty { log("Edit menu: " + (NSApp.mainMenu?.item(withTitle: "Edit")?.submenu?.items.map(\.title).joined(separator: " | ") ?? "-")) }
        check("380 Edit ▸ Transform items", ["Warp", "Perspective Warp", "Puppet Warp", "Content-Aware Scale"].allSatisfy(titles.contains),
              titles.joined(separator: ", "))

        // 381: pixel layer: cancel leaves Pixel and history; Apply asks and wraps in one node.
        let h0 = history()
        guard await began(.warp) else { return finish() }
        check("381 pixel target needs consent", t.session?.start.needsConversion == true)
        t.warpBend = 40
        t.applyWarpPreset("Arc")
        await settle()
        await shot("381-warp-preview-pixel")
        _ = await finished { t.cancel() }
        check("381 cancel keeps Pixel", kind(photo) == .pixel, "\(String(describing: kind(photo)))")
        check("381 cancel leaves history", history() == h0, "\(history()) vs \(h0)")
        guard await began(.warp) else { return finish() }
        t.warpBend = 40
        t.applyWarpPreset("Arc")
        await settle()
        t.apply()
        check("381 Apply asks before converting", t.consentPending)
        await shot("381-convert-alert")
        let r381 = await finished { t.confirmConversion() }
        check("381 committed", (try? r381?.get()) != nil, "\(String(describing: r381))")
        check("381 smart object after consent", kind(photo) == .smartObject)
        check("381 one node", history() == h0 + 1, "\(history()) vs \(h0 + 1)")
        check("381 label", doc?.history.last?.label == "Warp", doc?.history.last?.label ?? "-")

        // 382–384: re-edit the stage; drag an anchor and a handle; split; zero bend.
        guard await began(.warp) else { return finish() }
        if let tb = doc?.backend as? any DocumentTransformsBackend, let stage = try? tb.transformStages(layer: photo).first {
            _ = await finished { t.cancel() }
            t.editStage(d, layer: photo, index: stage.index)
            _ = await wait(20) { t.session != nil }
            await settle()
        }
        check("382 re-edit opens the stage", t.session?.start.editingIndex == 0)
        t.resetLatencies()
        if case .warp(let m) = t.session?.op {
            let anchor = m.controlPoints[3][3]
            await drag(view(anchor), view(CGPoint(x: anchor.x + 180, y: anchor.y + 120)), steps: 20)
            await settle()
            if case .warp(let m2) = t.session?.op {
                check("382 anchor follows the drag", abs(Double(m2.controlPoints[3][3].x - anchor.x) - 180) < 12,
                      "\(m2.controlPoints[3][3]) from \(anchor)")
            }
            let handle = m.controlPoints[0][1]
            await drag(view(handle), view(CGPoint(x: handle.x, y: handle.y + 150)), steps: 10)
        }
        await shot("382-warp-drag")
        log("382 latency \(t.latencyReadout ?? "-")")
        let before = t.session.flatMap { s -> Int? in if case .warp(let m) = s.op { return m.columns } else { return nil } } ?? 0
        t.warpSplit = .cross
        await click(view(CGPoint(x: 550, y: 330)))
        await settle()
        if case .warp(let m) = t.session?.op {
            check("383 split adds a column and a row", m.columns == before + 3 && m.rows == before + 3, "\(m.columns)×\(m.rows)")
        }
        await shot("383-warp-split")
        t.warpBend = 0
        t.applyWarpPreset("Bulge")
        await settle()
        if case .warp(let m) = t.session?.op, let s = t.session {
            check("384 zero bend is identity", m == WarpMeshModel.identity(width: Double(s.start.childWidth), height: Double(s.start.childHeight)))
        }
        t.warpBend = -60
        t.applyWarpPreset("Flag")
        await shot("384-warp-flag-bend")
        let h1 = history()
        _ = await finished { _ = key(code: 36, chars: "\r") }
        check("392 Return applies one node", history() == h1 + 1, "\(history()) vs \(h1 + 1)")
        check("394 re-edit kept one stage", ((try? (doc?.backend as? any DocumentTransformsBackend)?.transformStages(layer: photo))?.count) == 1)

        // 392: undo / redo exactly once.
        let applied = (try? d.backend.info().historyHead) ?? 0
        let stagesNow = { ((try? (self.doc?.backend as? any DocumentTransformsBackend)?.transformStages(layer: photo)) ?? []).first?.json }
        let appliedStage = stagesNow()
        d.undo()
        await settle()
        let undone = (try? d.backend.info().historyHead) ?? 0
        check("392 undo steps back once", undone != applied && stagesNow() != appliedStage, "\(undone) vs \(applied)")
        d.redo()
        await settle()
        check("392 redo restores the stage", (try? d.backend.info().historyHead) == applied && stagesNow() == appliedStage)

        // 385–386: linked perspective planes on a second layer (duplicate the smart object).
        guard await began(.perspective) else { return finish() }
        t.splitPerspective(vertical: true)
        await settle()
        if case .perspective(let p) = t.session?.op {
            check("385 two linked planes", p.quads.count == 2 && p.columns == 2)
        }
        t.perspectiveLayout = false
        if case .perspective(let p) = t.session?.op {
            let shared = p.destination[0][1]
            await drag(view(shared), view(CGPoint(x: shared.x + 120, y: shared.y + 60)), steps: 12)
            await settle()
            if case .perspective(let p2) = t.session?.op {
                check("385 shared vertex moved once for both planes", p2.destination[0][1] != shared && p2.isValid)
            }
            await shot("385-perspective-linked")
            // 386: drag a corner across the plane: rejected, the previous shape stays.
            let valid = t.session?.op
            let corner = p.destination[1][0]
            await drag(view(corner), view(CGPoint(x: p.destination[0][2].x + 300, y: p.destination[0][2].y - 200)), steps: 6)
            await settle()
            if case .perspective(let p3) = t.session?.op {
                check("386 crossing quad rejected", p3.isValid && t.session?.op != nil, t.status ?? "")
            }
            check("386 status explains", (t.status ?? "").contains("convex") || t.session?.op == valid, t.status ?? "-")
            await shot("386-perspective-rejected")
        }
        _ = await finished { t.apply() }
        check("385 applied", doc?.history.last?.label == "Perspective Warp", doc?.history.last?.label ?? "-")

        // 387–388: puppet pins, drag, rotate, delete, density.
        guard await began(.puppet) else { return finish() }
        if case .puppet(let p) = t.session?.op {
            check("387 mesh", p.restVertices.count > 20, "\(p.restVertices.count) vertices")
            log("387 puppet note \(t.puppetNote ?? "-")")
        }
        let W = Double(d.info.width), H = Double(d.info.height)
        for (x, y) in [(0.2, 0.5), (0.8, 0.5), (0.5, 0.3)] { await click(view(CGPoint(x: W * x, y: H * y))) }
        await settle()
        let pins: Int = { if case .puppet(let p) = t.session?.op { return p.pins.count } else { return 0 } }()
        check("387 three pins", pins == 3, "\(pins)")
        await drag(view(CGPoint(x: W * 0.8, y: H * 0.5)), view(CGPoint(x: W * 0.8, y: H * 0.25)), steps: 12)
        await settle()
        await drag(view(CGPoint(x: W * 0.2 + 30, y: H * 0.5)), view(CGPoint(x: W * 0.2, y: H * 0.5 + 30)), flags: .option, steps: 8)
        await settle()
        if case .puppet(let p) = t.session?.op {
            check("387 pin rotated", p.pins.contains { $0.rotation != nil }, "\(p.pins.map(\.rotation))")
        }
        await shot("387-puppet-pins")
        await click(view(CGPoint(x: W * 0.5, y: H * 0.3)), flags: .option)
        await settle()
        let pinsAfter: Int = { if case .puppet(let p) = t.session?.op { return p.pins.count } else { return 0 } }()
        check("387 ⌥-click removes a pin", pinsAfter == 2, "\(pinsAfter)")
        t.puppetDensity = .sparse
        t.remesh()
        await settle(0.5)
        if case .puppet(let p) = t.session?.op { check("388 density remesh keeps pins", p.density == "Sparse" && p.pins.count == 2) }
        t.setPuppetRigid(true)
        await settle()
        if let s = engine, let e = try? s.puppetMeshFromLayer(layer: photo, density: "Normal", expansion: 65) {
            check("388 expansion 65 rejected", false, "\(e.vertexCount)")
        } else {
            check("388 expansion 65 rejected", true)
        }
        await shot("388-puppet-rigid-sparse")
        _ = await finished { t.apply() }

        // 391: zoom 200 % and pan: handles map to child / document coordinates.
        viewport?.zoomActual()
        viewport?.zoomIn()
        viewport?.panBy(dx: -300, dy: -150)
        await pause(0.5)
        guard await began(.warp) else { return finish() }
        if case .warp(let m) = t.session?.op, let v = viewport {
            let anchor = m.controlPoints[3][3]
            let expected = v.viewPoint(canvas: t.session!.start.mapping.document(anchor))
            check("391 mapping at 200 %", hypot(Double(view(anchor).x - expected.x), Double(view(anchor).y - expected.y)) < 0.01)
            await drag(view(anchor), view(CGPoint(x: anchor.x - 40, y: anchor.y - 40)), steps: 6)
            await settle()
            if case .warp(let m2) = t.session?.op {
                let moved = m2.controlPoints[3][3]
                check("391 drag at 200 % moves 40 child px", abs(Double(moved.x - anchor.x) + 40) < 3, "\(moved) from \(anchor)")
            }
        }
        await shot("391-zoom-200")
        _ = await finished { t.cancel() }
        viewport?.zoomToFit()
        await pause(0.4)

        // 389–390 on a fresh single-layer document (amount 0 / 1, then a protection channel).
        if let casURL = writeCard("cas.png", width: 800, height: 500), let dc = await open(casURL),
           let l = dc.layers.first(where: { $0.kind == .pixel })?.id {
            dc.select(l)
            viewport?.zoomToFit()
            if let s = engine {
                _ = try? s.setSelectionRect(x: 300, y: 0, width: 220, height: 500, feather: 0)
                _ = try? s.saveSelectionChannel(name: "Protect rings", target: nil, op: .replace)
                _ = try? s.clearSelection()
                dc.reloadModel()
            }
            if await began(.contentAwareScale) {
                t.setScale(width: 560, amount: 0)
                await settle(4)
                await shot("389-cas-amount-0")
                t.setScale(amount: 1)
                await settle(8)
                await shot("389-cas-amount-1")
                if let c = t.channels.first(where: { $0.name == "Protect rings" }) {
                    t.setScale(protect: .some(c.id))
                    await settle(8)
                    await shot("390-cas-protected-channel")
                }
                _ = await finished { t.cancel() }
                check("389 fresh CAS cancelled to Pixel", dc.node(l)?.kind == .pixel)
            }
            workspace.close(dc)
            await pause(0.5)
            workspace.select(d)
            await settle(1)
        }
        // 389–390 on the stacked smart object: content-aware scale above three geometric stages.
        if let s = engine {
            _ = try? s.setSelectionRect(x: 0, y: 0, width: Int64(W * 0.3), height: Int64(H), feather: 0)
            _ = try? s.saveSelectionChannel(name: "Protect left", target: nil, op: .replace)
            _ = try? s.clearSelection()
            d.reloadModel()
        }
        guard await began(.contentAwareScale) else { return finish() }
        check("390 channel offered, no skin option", t.channels.contains { $0.name == "Protect left" } && !(t.session?.start.limitations.joined().contains("skin detector") ?? true),
              t.channels.map(\.name).joined(separator: ","))
        t.setScale(width: UInt32(W * 0.7), amount: 0)
        t.setScale(amount: 1)
        if let c = t.channels.first(where: { $0.name == "Protect left" }) { t.setScale(protect: .some(c.id)) }
        await settle(3)
        if case .contentAwareScale(let c) = t.session?.op, let s = t.session {
            // Drag the right handle back to 80 %.
            await drag(view(c.point(.right)), view(CGPoint(x: Double(s.start.childWidth) * 0.8, y: Double(c.point(.right).y))), steps: 6)
            await settle(2)
        }
        let r389 = await finished { t.apply() }
        check("389 CAS applied", (try? r389?.get()) != nil, "\(String(describing: r389))")

        // 395: position lock rejects geometry.
        if let id = doc?.primary?.id {
            doc?.toggleLock(.position)
            await settle()
            t.begin(.warp)
            _ = await wait(3) { t.session != nil || (t.status ?? "").contains("locked") }
            check("395 position lock rejects", t.session == nil && (t.status ?? "").contains("locked"), t.status ?? "-")
            doc?.toggleLock(.position)
            await settle()
        }

        // 396: live text: explicit conversion only; cancel keeps the text layer.
        if let tb = doc?.backend as? any DocumentTextBackend {
            let m = TextSourceModel(runs: [TextRunModel(text: "Warp me", family: "Helvetica", size: Float(H / 8))])
            _ = try? tb.addTextLayer(name: "Title", parent: nil, index: nil, model: m, transform: .translation(W * 0.1, H * 0.8),
                                     interactive: false)
            d.reloadModel()
            if let text = d.layers.first(where: { $0.kind == .text })?.id {
                d.select(text)
                guard await began(.warp) else { return finish() }
                check("396 text needs explicit conversion", t.session?.start.needsConversion == true)
                t.warpBend = 50
                t.applyWarpPreset("Flag")
                await settle()
                await shot("396-text-warp-preview")
                _ = await finished { t.cancel() }
                check("396 cancel keeps live text", kind(text) == .text)
            }
        }

        // 393: a preview, then switching documents cancels without a late mutation.
        let hBeforeSwitch = history()
        d.select(photo)
        guard await began(.warp) else { return finish() }
        t.warpBend = 30
        t.applyWarpPreset("Wave")
        guard let url2 = writeCard("second.png", width: 800, height: 500), let d2 = await open(url2) else { return finish() }
        await pause(1)
        check("393 switch ended the session", t.session == nil,
              "current \(workspace.current?.id ?? "-") session \(t.session?.doc?.id ?? "-") d2 \(d2.id) d \(d.id)")
        workspace.select(d)
        await settle(1)
        check("393 no late history node", history() == hBeforeSwitch, "\(history()) vs \(hBeforeSwitch)")
        workspace.close(d2)
        await pause(0.5)

        // 397–398: native save / reopen keeps the stages; PSD refuses, the rasterized copy works.
        workspace.select(d)
        let native = dir.appendingPathComponent("transforms.tessera-doc")
        try? FileManager.default.removeItem(at: native)
        do {
            try d.backend.saveAs(path: native.path)
            check("397 native save", true)
        } catch { check("397 native save", false, error.localizedDescription) }
        let psd = dir.appendingPathComponent("transforms.psd")
        do {
            try d.backend.saveAs(path: psd.path)
            check("398 PSD refuses native-only stages", false, "saved")
        } catch {
            check("398 PSD refuses native-only stages", error.localizedDescription.contains("rasterize"), error.localizedDescription)
            log("398 message: \(error.localizedDescription)")
        }
        if let tb = d.backend as? any DocumentTransformsBackend {
            let copy = dir.appendingPathComponent("transforms-rasterized.psd")
            let started = Date()
            let r = await Task.detached { Result { try tb.savePSDRasterizingTransforms(path: copy.path) } }.value
            switch r {
            case .success: check("398 rasterized copy", FileManager.default.fileExists(atPath: copy.path))
            case .failure(let e): check("398 rasterized copy", false, e.localizedDescription)
            }
            log(String(format: "398 rasterized copy took %.0f ms", Date().timeIntervalSince(started) * 1000))
        }
        if let reopened = await open(native) {
            let stages = reopened.layers.filter { $0.kind == .smartObject }.flatMap {
                (try? (reopened.backend as? any DocumentTransformsBackend)?.transformStages(layer: $0.id)) ?? []
            }
            check("397 reopened stages", stages.count >= 4, stages.map(\.kind.title).joined(separator: ", "))
            await shot("397-reopened-native")
            // Re-edit after reopen.
            if let so = stages.first(where: { $0.kind == .warp }) {
                reopened.select(so.layer)
                t.editStage(reopened, layer: so.layer, index: so.index)
                _ = await wait(20) { t.session != nil }
                check("397 re-edit after reopen", t.session?.start.existing?.kind == .warp)
                _ = await finished { t.cancel() }
            }
        }
        await shot("399-panels-1440")

        // 399: drag latency on a 20 MP smart object (draft proxy while dragging).
        if ProcessInfo.processInfo.environment["TRANSFORM_SELFTEST_20MP"] != "0",
           let big = writeCard("card-20mp.png", width: 5472, height: 3648), let d3 = await open(big) {
            viewport?.zoomToFit()
            await pause(1)
            if let l = d3.layers.first(where: { $0.kind == .pixel })?.id {
                d3.select(l)
                (d3.backend as? any DocumentFiltersBackend).map { b in _ = try? b.convertForSmartFilters(layer: l) }
                d3.reloadModel()
                await settle(1)
                let started = Date()
                guard await began(.warp) else { return finish() }
                log(String(format: "399 begin on 20 MP: %.0f ms, draft level %d", Date().timeIntervalSince(started) * 1000,
                           Int(t.session?.start.draftLevel ?? 0)))
                t.resetLatencies()
                if case .warp(let m) = t.session?.op {
                    let a = m.controlPoints[3][3]
                    for round in 0..<3 {
                        let dx = CGFloat(round % 2 == 0 ? 600 : -600)
                        await drag(view(CGPoint(x: a.x + (round % 2 == 0 ? 0 : 600), y: a.y)),
                                   view(CGPoint(x: a.x + (round % 2 == 0 ? 0 : 600) + dx, y: a.y + 300)), steps: 30, perStep: 1.0 / 60)
                    }
                }
                await settle(1)
                let lat = t.latencies.sorted()
                if !lat.isEmpty {
                    log(String(format: "399 20 MP drag latency (preview call → presented frame): n %d median %.1f ms p95 %.1f ms max %.1f ms",
                               lat.count, lat[lat.count / 2], lat[min(lat.count - 1, Int(Double(lat.count) * 0.95))], lat.last ?? 0))
                }
                check("399 20 MP drags presented", !lat.isEmpty)
                await shot("399-20mp-warp-drag")
                let t0 = Date()
                _ = await finished { t.apply() }
                log(String(format: "399 20 MP apply → exact frame: %.0f ms", Date().timeIntervalSince(t0) * 1000))
                await shot("399-20mp-applied")
            }
        }
        finish()
    }

    private func finish() {
        log("done, \(failures) failure(s)")
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) { NSApp.terminate(nil) }
        // Documents left dirty by the run may hold termination at a save prompt: a test aid just exits.
        let code: Int32 = failures == 0 ? 0 : 1
        DispatchQueue.main.asyncAfter(deadline: .now() + 6) { exit(code) }
    }
}
