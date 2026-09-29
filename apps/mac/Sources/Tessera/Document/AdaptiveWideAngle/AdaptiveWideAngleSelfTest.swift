import AppKit
import TesseraCore

/// `--adaptive-wide-angle-selftest <dir>` (test aid, WP B5-20): acceptance steps 460–479 through the calls the menu,
/// canvas and sheet make. After the library loads it opens `sample.dng` with Edit in Layers, opens Filter ▸ Adaptive
/// Wide Angle…, switches to Fisheye, sets the focal length, draws a vertical and a straight constraint (as a drag
/// does), toggles Preview, cancels (no history), reopens and applies (one row), undoes, converts for smart filters,
/// applies as a smart filter, re-opens the row (lines and camera kept), edits it (still one smart filter), and
/// checks that a conflicting constraint reports an error without history. Each step prints
/// `awa-selftest: step <n> <name> window <x> <y> <w> <h>` (for `screencapture -R`); checks print
/// `check <name> ok|FAIL`. Quits at the end. Launch only in the background: `open -g -n … --args
/// --adaptive-wide-angle-selftest <dir> --nonactivating`.
@MainActor
final class AdaptiveWideAngleSelfTest {
    private let model: AppModel
    private let dir: URL
    private let hold: Double
    private var failures = 0
    private var step = 0
    private static var started = false

    static func startIfRequested() {
        guard !started else { return }
        let args = CommandLine.arguments
        guard let i = args.firstIndex(of: "--adaptive-wide-angle-selftest"), i + 1 < args.count else { return }
        started = true
        let dir = URL(fileURLWithPath: (args[i + 1] as NSString).expandingTildeInPath)
        let hold = args.firstIndex(of: "--adaptive-wide-angle-selftest-hold")
            .flatMap { $0 + 1 < args.count ? Double(args[$0 + 1]) : nil } ?? 1.5
        DispatchQueue.main.async {
            MainActor.assumeIsolated {
                let test = AdaptiveWideAngleSelfTest(model: AppModel.shared, dir: dir, hold: hold)
                Task { @MainActor in await test.run() }
            }
        }
    }

    init(model: AppModel, dir: URL, hold: Double) {
        self.model = model
        self.dir = dir
        self.hold = hold
    }

    private func log(_ s: String) { FileHandle.standardError.write(Data("awa-selftest: \(s)\n".utf8)) }

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

    /// Opens the workspace and waits for it (and its first preview).
    private func open(_ body: () -> Void) async -> AdaptiveWideAngleWorkspaceModel? {
        let awa = DocumentAdaptiveWideAngle.shared
        body()
        guard await wait(120, { awa.workspace != nil && !awa.opening }), let m = awa.workspace else { return nil }
        _ = await wait(60) { m.original != nil && (m.corrected != nil || m.error != nil) }
        return m
    }

    /// Fisheye at `mm`, then a vertical line near the left edge and a straight one along the top third.
    private func constrain(_ m: AdaptiveWideAngleWorkspaceModel, mm: Double) async {
        m.projection = .equidistant
        m.setFocal(mm)
        let (w, h) = (Double(m.info.width), Double(m.info.height))
        m.pointerDown(CGPoint(x: w * 0.2, y: h * 0.15), tolerance: 1)
        m.pointerDragged(CGPoint(x: w * 0.2, y: h * 0.5))
        m.pointerUp(CGPoint(x: w * 0.21, y: h * 0.85), constrain: true)
        m.pointerDown(CGPoint(x: w * 0.3, y: h * 0.3), tolerance: 1)
        m.pointerUp(CGPoint(x: w * 0.7, y: h * 0.3), constrain: false)
        await idle(m)
    }

    private func idle(_ m: AdaptiveWideAngleWorkspaceModel) async {
        var landed = false
        m.onPreview = { landed = true }
        _ = await wait(60) { landed }
        m.onPreview = nil
    }

    private func applied(_ m: AdaptiveWideAngleWorkspaceModel) async -> Bool {
        var result: Result<DocumentChange, Error>?
        m.onApplied = { result = $0 }
        let t = Date()
        m.ok()
        _ = await wait(300) { result != nil }
        log(String(format: "apply at full resolution: %.2f s", Date().timeIntervalSince(t)))
        if case .failure(let e) = result { log("apply error: \(e.localizedDescription)") }
        if case .success = result { return true }
        return false
    }

    func run() async {
        let ws = model.documents
        let awa = DocumentAdaptiveWideAngle.shared
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
        log("document \(doc.info.width) × \(doc.info.height) px")
        guard let photo = doc.layers.first(where: { $0.kind == .pixel })?.id else { return finish() }
        doc.select(photo)

        // 460–466: open, camera, constraints, preview toggle, cancel.
        guard let m = await open({ awa.open(doc) }) else {
            check("workspace opens", false, model.statusMessage ?? ""); return finish()
        }
        log("preview \(m.info.previewWidth) × \(m.info.previewHeight) (1/\(m.info.previewFactor)), EXIF focal \(m.info.exifFocal35mm.map { "\($0) mm" } ?? "none")")
        check("original preview", m.original != nil, m.error ?? "")
        await mark("opened")
        let rows = doc.history.count
        await constrain(m, mm: 16)
        check("two traced constraints", m.draft.lines.count == 2 && m.draft.lines.allSatisfy { $0.points.count > 2 },
              "\(m.draft.lines.map(\.points.count))")
        check("shift-drag is vertical", m.draft.lines.first?.orientation == .vertical)
        await mark("constraints")
        m.preview = true
        _ = await wait(30) { m.corrected != nil || m.error != nil }
        check("corrected preview", m.corrected != nil, m.error ?? "")
        await mark("preview")
        check("preview records no history", doc.history.count == rows, "\(doc.history.map(\.label))")
        m.cancel()
        check("cancel closes, no history", awa.workspace == nil && doc.history.count == rows)
        await mark("cancelled")

        // 467–469: apply to the pixel layer, one row, undo.
        guard let again = await open({ awa.open(doc) }) else { check("reopens", false); return finish() }
        await constrain(again, mm: 16)
        check("apply is one history row", await applied(again) && doc.history.count == rows + 1,
              "\(doc.history.map(\.label))")
        check("history label", doc.history.contains { $0.label == AdaptiveWideAngleFilter.title }, "\(doc.history.map(\.label))")
        await mark("applied")
        doc.undo()
        await mark("undone")

        // 470–476: smart filter, re-edit.
        DocumentFilters.active?.convertForSmartFilters(doc)
        check("smart object", doc.node(photo)?.kind == .smartObject)
        guard let smart = await open({ awa.open(doc) }) else { check("smart opens", false); return finish() }
        await constrain(smart, mm: 16)
        let recipe = smart.draft
        check("smart apply", await applied(smart))
        var list = DocumentFilters.active?.smartFilters(doc, layer: photo) ?? []
        check("smart filter listed", list.map(\.filterId) == [AdaptiveWideAngleFilter.id], "\(list.map(\.filterId))")
        check("smart filter name", list.first?.name == AdaptiveWideAngleFilter.title, list.first?.name ?? "")
        await mark("smart-filter")
        if let row = list.first {
            guard let edit = await open({ awa.edit(doc, layer: photo, row: row) }) else {
                check("re-edit opens", false, model.statusMessage ?? ""); return finish()
            }
            check("re-edit keeps lines and camera", edit.draft == recipe && edit.info.stageIndex == 0)
            edit.setScale(90)
            await idle(edit)
            await mark("smart-reedit")
            check("re-edit apply", await applied(edit))
            list = DocumentFilters.active?.smartFilters(doc, layer: photo) ?? []
            check("re-edit replaces", list.count == 1 && list[0].filterJson.contains("\"scale\":0.9"), "\(list.map(\.filterJson))")
        }

        // 477–479: a conflicting constraint is an error, with no history.
        if let bad = await open({ awa.open(doc) }) {
            let (w, h) = (Double(bad.info.width), Double(bad.info.height))
            bad.addLine(from: CGPoint(x: w * 0.5, y: h * 0.1), to: CGPoint(x: w * 0.5, y: h * 0.9), orientation: .horizontal)
            await idle(bad)
            check("conflict reported", bad.error != nil)
            let n = doc.history.count
            let ok = await applied(bad)
            check("conflict never commits", !ok && doc.history.count == n && bad.error != nil, bad.error ?? "")
            await mark("conflict")
            bad.cancel()
        } else {
            check("conflict sheet", false)
        }
        finish()
    }

    private func finish() {
        log("done, \(failures) failure(s)")
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) { NSApp.terminate(nil) }
    }
}
