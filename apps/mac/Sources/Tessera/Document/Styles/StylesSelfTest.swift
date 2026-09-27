import AppKit
import TesseraCore

/// `--styles-selftest <dir>` (test aid, WP B5-07): the ACCEPTANCE layer-style steps through the same
/// controller calls the menus, the inspector and the Layers panel make, on the engine. It makes a
/// 1000 × 700 document with a grey backdrop and two orange shapes, applies Drop Shadow and Stroke to
/// the first through the Layer Style inspector, sets its Fill to 0 %, gives the second a drop shadow,
/// moves the Global Light (both shadows follow), undoes and redoes it, saves `<dir>/StylesSelfTest.psd`
/// and `.tessera-doc`, closes and reopens the PSD. Each step prints
/// `styles-selftest: step <n> <name> window <x> <y> <w> <h> [panel <x> <y> <w> <h>]` (for
/// `screencapture -R`) and holds `hold` seconds; checks print `check <name> ok|FAIL`. Quits at the end.
@MainActor
final class StylesSelfTest {
    private let model: AppModel
    private let dir: URL
    private let hold: Double
    private var failures = 0
    private var step = 0
    private var styles: DocumentStyles { .shared }

    static func startIfRequested() {
        let args = CommandLine.arguments
        guard let i = args.firstIndex(of: "--styles-selftest"), i + 1 < args.count else { return }
        let dir = URL(fileURLWithPath: (args[i + 1] as NSString).expandingTildeInPath)
        let hold = args.firstIndex(of: "--styles-selftest-hold").flatMap { $0 + 1 < args.count ? Double(args[$0 + 1]) : nil } ?? 2
        DispatchQueue.main.async {
            MainActor.assumeIsolated {
                let test = StylesSelfTest(model: AppModel.shared, dir: dir, hold: hold)
                Task { @MainActor in await test.run() }
            }
        }
    }

    init(model: AppModel, dir: URL, hold: Double) {
        self.model = model
        self.dir = dir
        self.hold = hold
    }

    private func log(_ s: String) { FileHandle.standardError.write(Data("styles-selftest: \(s)\n".utf8)) }

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

    private func rect(_ w: NSWindow?) -> String {
        guard let w, w.isVisible, let screen = NSScreen.screens.first else { return "" }
        let f = w.frame
        return String(format: "%.0f %.0f %.0f %.0f", f.minX, screen.frame.height - f.maxY, f.width, f.height)
    }

    private func mark(_ name: String) async {
        step += 1
        await pause(0.8)
        var frame = ""
        if let main = model.mainWindow {
            if !BackgroundRun.active { main.orderFrontRegardless() }   // B5-16
            frame = " window " + rect(main)
        }
        let panels = NSApp.windows.filter { $0 is NSPanel && $0.isVisible }
        let panel = panels.first { $0.title == "Global Light" } ?? panels.first { $0.title == "Layer Style" }
        if let panel {
            if !BackgroundRun.active { panel.orderFrontRegardless() }
            frame += " panel " + rect(panel)
        }
        await pause(0.3)
        log("step \(step) \(name)\(frame)")
        await pause(hold)
    }

    /// A pixel layer named `name` with an opaque orange rectangle.
    private func shape(_ doc: DocumentController, _ name: String, _ r: CanvasRect) -> DocLayerID? {
        doc.addLayer(.pixel, name: name)
        guard let id = doc.primary?.id, let tools = doc.backend as? any DocumentToolsBackend else { return nil }
        doc.setMarquee(r)
        doc.run("Fill") { try tools.fillSelection(layer: id, fill: .color(ToolColor(r: 0.95, g: 0.55, b: 0.15)), opacity: 1) }
        doc.deselect()
        return id
    }

    func run() async {
        let ws = model.documents
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        _ = await wait(30) { model.mainWindow != nil }
        ws.newDocument(NewDocumentSettings(width: 1000, height: 700))
        guard await wait(60, { ws.current != nil && ws.opening == nil }), let doc = ws.current else {
            log("FAIL no document: \(model.statusMessage ?? "")"); return finish()
        }
        log("document \(doc.info.width) × \(doc.info.height) px, \(doc.info.backend)")
        // A grey backdrop on the document's first layer (a pixel layer, so the PSD keeps it).
        if let base = doc.primary?.id, let tools = doc.backend as? any DocumentToolsBackend {
            doc.selectAll()
            doc.run("Fill") { try tools.fillSelection(layer: base, fill: .color(ToolColor(r: 0.82, g: 0.82, b: 0.8)), opacity: 1) }
            doc.deselect()
        }
        guard let a = shape(doc, "Shape A", CanvasRect(x: 150, y: 200, width: 260, height: 260)),
              let b = shape(doc, "Shape B", CanvasRect(x: 590, y: 200, width: 260, height: 260)) else {
            check("shapes", false, model.statusMessage ?? ""); return finish()
        }
        _ = await wait(10) { doc.lastFrame != nil }
        await mark("shapes")

        // 1. Layer ▸ Layer Style ▸ Drop Shadow…, then Stroke from the inspector's list.
        styles.open(doc, layer: a, kind: .dropShadow)
        styles.addEffect(doc, a, .stroke)
        if var m = styles.model(doc, a), let s = m.indices(of: .stroke).first, let d = m.indices(of: .dropShadow).first {
            m.effects[s].setNumber("size", 6)
            m.effects[s].fill = .solid(color: [0.1, 0.2, 0.7])
            m.effects[d].setNumber("distance", 18)
            m.effects[d].setNumber("size", 12)
            // A distance drag: live steps, one node on release.
            let rows = doc.history.count
            for dist in [20.0, 24, 28] {
                m.effects[d].setNumber("distance", dist)
                styles.apply(doc, a, m, label: "Drop Shadow", final: dist == 28)
            }
            check("drag is one node", doc.history.count == rows + 1, "\(doc.history.suffix(4).map(\.label))")
        }
        styles.pane = styles.model(doc, a)?.indices(of: .dropShadow).first.map { .effect($0) } ?? .blending
        check("shadow + stroke", styles.rows(doc)[a]?.effects.map(\.kind) == [.stroke, .dropShadow],
              "\(styles.rows(doc)[a]?.effects.map(\.kind.rawValue) ?? [])")
        await mark("shadow-and-stroke")

        // 2. Fill 0 %: the shape's pixels vanish, its effects stay.
        styles.pane = .blending
        doc.setFillOpacity(0, final: true)
        check("fill 0", doc.node(a)?.fillOpacity == 0)
        check("effects kept at fill 0", styles.rows(doc)[a]?.effects.count == 2)
        await mark("fill-zero")

        // 3. A drop shadow on the second shape, then move the Global Light: both shadows follow.
        styles.open(doc, layer: b, kind: .dropShadow)
        if var m = styles.model(doc, b), let d = m.indices(of: .dropShadow).first {
            m.effects[d].setNumber("distance", 28)
            m.effects[d].setNumber("size", 12)
            styles.apply(doc, b, m, label: "Drop Shadow", final: true)
        }
        await mark("second-shadow")
        styles.closeInspector()
        styles.openGlobalLight(doc)
        // Keep the canvas in view: the light panel goes to the main window's lower left.
        if let main = model.mainWindow, let light = NSApp.windows.first(where: { $0.title == "Global Light" }) {
            light.setFrameOrigin(NSPoint(x: main.frame.minX + 40, y: main.frame.minY + 40))
        }
        let before = styles.globalLight(doc)
        for angle in stride(from: 120.0, through: 30, by: -15) {
            styles.setGlobalLight(doc, DocGlobalLight(angle: angle, altitude: before.altitude), final: angle == 30)
            await pause(0.05)
        }
        let g = styles.globalLight(doc)
        check("global light moved", g.angle == 30, "\(g)")
        check("both shadows follow", [a, b].allSatisfy { id in
            styles.model(doc, id).map { m in m.indices(of: .dropShadow).allSatisfy { m.angle(of: $0, global: g) == 30 } } ?? false
        })
        check("global light is one node", doc.history.last?.label == "Global Light", "\(doc.history.suffix(3).map(\.label))")
        await mark("global-light-30")

        // 4. Undo and redo the light.
        doc.undo()
        check("undo restores the light", styles.globalLight(doc).angle == before.angle, "\(styles.globalLight(doc))")
        await mark("undo-global-light")
        doc.redo()
        check("redo", styles.globalLight(doc).angle == 30)
        styles.closeGlobalLight()
        doc.select(a)
        await mark("redo-global-light")

        // 5. Save PSD (and the native file), close, reopen the PSD.
        let psd = dir.appendingPathComponent("StylesSelfTest.psd")
        let native = dir.appendingPathComponent("StylesSelfTest.tessera-doc")
        for u in [psd, native] { try? FileManager.default.removeItem(at: u) }
        check("save tessera-doc", ws.write(doc, to: native), model.statusMessage ?? "")
        check("save psd", ws.write(doc, to: psd), model.statusMessage ?? "")
        styles.closeInspector()
        ws.close(doc)
        _ = await wait(10) { ws.current == nil || ws.current !== doc }
        ws.open(psd)
        guard await wait(60, { ws.current != nil && ws.current !== doc && ws.opening == nil }), let re = ws.current else {
            check("reopen", false, model.statusMessage ?? ""); return finish()
        }
        _ = await wait(10) { re.lastFrame != nil }
        let ra = re.layers.first { $0.name == "Shape A" }, rb = re.layers.first { $0.name == "Shape B" }
        let rows = styles.rows(re)
        check("reopened A styles", ra.flatMap { rows[$0.id]?.effects.map(\.kind) } == [.stroke, .dropShadow],
              "\(ra.flatMap { rows[$0.id]?.effects.map(\.kind.rawValue) } ?? [])")
        check("reopened B styles", rb.flatMap { rows[$0.id]?.effects.map(\.kind) } == [.dropShadow])
        check("reopened fill 0", ra?.fillOpacity == 0, "\(ra?.fillOpacity ?? -1)")
        check("reopened global light", abs(styles.globalLight(re).angle - 30) < 0.01, "\(styles.globalLight(re))")
        if let ra { styles.open(re, layer: ra.id, kind: .stroke) }
        await mark("reopened-psd")
        styles.closeInspector()
        finish()
    }

    private func finish() {
        log("done, \(failures) failure(s)")
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) { NSApp.terminate(nil) }
    }
}
