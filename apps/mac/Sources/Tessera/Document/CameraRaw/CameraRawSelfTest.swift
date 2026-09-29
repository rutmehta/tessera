import AppKit
import TesseraCore

/// `--camera-raw-selftest <dir>` (test aid, WP B5-18): acceptance steps 420–439 through the calls the menu and
/// sheet make. After the library loads it opens `sample.dng` with Edit in Layers, opens Filter ▸ Camera Raw
/// Filter…, visits every tab, sets Exposure / Contrast / Vibrance, toggles Before, cancels (no history),
/// reopens and applies (one row), undoes, converts for smart filters, applies as a smart filter, re-opens the
/// row (values kept), edits it (still one smart filter), and checks the selection refusal. Each step prints
/// `camera-raw-selftest: step <n> <name> window <x> <y> <w> <h>` (for `screencapture -R`); checks print
/// `check <name> ok|FAIL`. Quits at the end. Launch only in the background: `open -g -n … --args
/// --camera-raw-selftest <dir> --nonactivating`.
@MainActor
final class CameraRawSelfTest {
    private let model: AppModel
    private let dir: URL
    private let hold: Double
    private var failures = 0
    private var step = 0
    private static var started = false

    static func startIfRequested() {
        guard !started else { return }
        let args = CommandLine.arguments
        guard let i = args.firstIndex(of: "--camera-raw-selftest"), i + 1 < args.count else { return }
        started = true
        let dir = URL(fileURLWithPath: (args[i + 1] as NSString).expandingTildeInPath)
        let hold = args.firstIndex(of: "--camera-raw-selftest-hold").flatMap { $0 + 1 < args.count ? Double(args[$0 + 1]) : nil } ?? 1.5
        DispatchQueue.main.async {
            MainActor.assumeIsolated {
                let test = CameraRawSelfTest(model: AppModel.shared, dir: dir, hold: hold)
                Task { @MainActor in await test.run() }
            }
        }
    }

    init(model: AppModel, dir: URL, hold: Double) {
        self.model = model
        self.dir = dir
        self.hold = hold
    }

    /// A line starting "FAIL" (an early exit: no library, no document, …) counts as a failure, so the
    /// closing `done, <n> failure(s)` is never a silent 0 for a run that did not happen.
    private func log(_ s: String) {
        if s.hasPrefix("FAIL") { failures += 1 }
        FileHandle.standardError.write(Data("camera-raw-selftest: \(s)\n".utf8))
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

    /// Never orders the window front or changes its level: the run must not take the user's focus.
    private func mark(_ name: String) async {
        step += 1
        await pause(0.6)
        var frame = ""
        if let w = model.mainWindow, let screen = NSScreen.screens.first {
            let f = (w.sheetParent ?? w).frame
            frame = String(format: " window %.0f %.0f %.0f %.0f", f.minX, screen.frame.height - f.maxY, f.width, f.height)
        }
        log("step \(step) \(name)\(frame)")
        await pause(hold)
    }

    private func idle(_ cr: DocumentCameraRaw) async -> Bool { await wait(180) { cr.busy == nil } }

    func run() async {
        let ws = model.documents
        let cr = DocumentCameraRaw.shared
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        guard await wait(60, { !model.isLoading && !model.library.items.isEmpty }) else {
            log("FAIL the library did not load"); return finish()
        }
        let items = model.library.items
        guard let item = items.first(where: { $0.name == "sample.dng" }) ?? items.first(where: { $0.kind == .raw }) else {
            log("FAIL no RAW in the library"); return finish()
        }
        model.select(id: item.id)
        ws.editInLayers(model.focusedItem)
        guard await wait(120, { ws.current != nil && ws.opening == nil }), let doc = ws.current else {
            log("FAIL Edit in Layers opened nothing: \(model.statusMessage ?? "")"); return finish()
        }
        _ = await wait(10) { doc.lastFrame != nil }
        log("document \(doc.info.width) × \(doc.info.height) px, \(doc.info.depth.title)")
        guard let photo = doc.layers.first(where: { $0.kind == .pixel })?.id else { log("FAIL no pixel layer"); return finish() }
        doc.select(photo)

        // 420–425: open, every tab.
        cr.open(doc)
        guard let sheet = cr.sheet else { check("sheet opens", false, model.statusMessage ?? ""); return finish() }
        _ = await wait(30) { sheet.detail != nil || sheet.error != nil }
        check("detail pane", sheet.detail != nil, sheet.error ?? "")
        for panel in CameraRawPanel.allCases {
            sheet.panel = panel
            await mark("panel-\(panel.rawValue)")
        }
        sheet.panel = .basic

        // 426–429: sliders, preview without history, before/after, cancel.
        let rows = doc.history.count
        let t0 = Date()
        sheet.set(CameraRawControls.exposure, 0.8, final: true)
        sheet.set(CameraRawControls.contrast, 30, final: true)
        sheet.set(CameraRawControls.vibrance, 40, final: true)
        _ = await wait(30) { sheet.detail != nil }
        log(String(format: "preview/detail after three edits: %.2f s", Date().timeIntervalSince(t0)))
        check("preview records no history", doc.history.count == rows, "\(doc.history.map(\.label))")
        await mark("edited-after")
        sheet.showBefore = true
        await mark("edited-before")
        sheet.showBefore = false
        sheet.cancel()
        check("cancel closes, no history", cr.sheet == nil && doc.history.count == rows, "\(doc.history.map(\.label))")
        await mark("cancelled")

        // 430–432: apply, one row, undo.
        cr.open(doc)
        guard let again = cr.sheet else { check("sheet reopens", false); return finish() }
        again.set(CameraRawControls.exposure, 0.8, final: true)
        again.set(CameraRawControls.vibrance, 40, final: true)
        let t1 = Date()
        again.ok()
        await mark("applying")
        _ = await idle(cr)
        log(String(format: "apply at full resolution: %.2f s", Date().timeIntervalSince(t1)))
        check("apply is one history row", doc.history.count == rows + 1, "\(doc.history.map(\.label))")
        await mark("applied")
        doc.undo()
        await mark("undone")

        // 433–437: smart filter, re-edit.
        DocumentFilters.active?.convertForSmartFilters(doc)
        check("smart object", doc.node(photo)?.kind == .smartObject)
        cr.open(doc)
        guard let smart = cr.sheet else { check("smart sheet", false); return finish() }
        smart.set(HSLProperty.saturation.control(.blue), -60, final: true)
        smart.ok()
        _ = await idle(cr)
        var list = DocumentFilters.active?.smartFilters(doc, layer: photo) ?? []
        check("smart filter listed", list.map(\.filterId) == ["camera_raw"], "\(list.map(\.filterId))")
        await mark("smart-filter")
        if let row = list.first {
            cr.edit(doc, layer: photo, row: row)
            if let edit = cr.sheet {
                check("re-edit keeps values", edit.value(HSLProperty.saturation.control(.blue)) == -60)
                edit.panel = .hsl
                edit.set(HSLProperty.saturation.control(.blue), -20, final: true)
                await mark("smart-reedit")
                edit.ok()
                _ = await idle(cr)
                list = DocumentFilters.active?.smartFilters(doc, layer: photo) ?? []
                check("re-edit replaces", list.count == 1
                      && CameraRawDraft(filterJson: list[0].filterJson)?.value(HSLProperty.saturation.control(.blue)) == -20,
                      "\(list.map(\.filterJson))")
            } else {
                check("re-edit opens", false, model.statusMessage ?? "")
            }
        }

        // 438–439: smart object with a selection is refused with a reason.
        _ = doc.run("Select") { try doc.backend.setSelectionRect(x: 0, y: 0, width: 200, height: 200, feather: 0) }
        doc.reloadModel()
        cr.open(doc)
        check("selection refusal", cr.sheet == nil)
        await mark("selection-refused")
        finish()
    }

    private func finish() {
        log("done, \(failures) failure(s)")
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) { NSApp.terminate(nil) }
    }
}
