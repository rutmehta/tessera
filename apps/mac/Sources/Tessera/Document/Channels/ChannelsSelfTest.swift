import AppKit
import TesseraCore

/// Test aid (WP B5-08): with `TESSERA_CHANNELS_SELFTEST=<dir>` in the environment, once a document is
/// open, runs ACCEPTANCE §Y through the Channels model the panel uses: save a selection as a channel,
/// add a spot channel, open each sheet, set Sky's alpha display (B5-17b: one node, undo / redo, kept on
/// reopen), preview the overlay, Quick Mask in and out, save as
/// `<dir>/ChannelsSelfTest.tessera-doc` and `.psd`, close, reopen, and load the saved channel back.
/// Each step prints `channels-selftest: step <n> <name> window <x> <y> <w> <h>` (for `screencapture -R`),
/// checks print `check <name> ok|FAIL`, and the run ends with `done, <n> failure(s)`.
@MainActor
final class ChannelsSelfTest {
    private let workspace: DocumentWorkspace
    private let dir: URL
    private let hold: Double
    private var failures = 0
    private var step = 0
    private var channels: DocumentChannels { .shared }

    private static var started = false

    /// Starts the run once when the environment asks for it.
    static func startIfRequested(_ workspace: DocumentWorkspace) {
        guard !started, let path = ProcessInfo.processInfo.environment["TESSERA_CHANNELS_SELFTEST"] else { return }
        started = true
        let hold = Double(ProcessInfo.processInfo.environment["TESSERA_CHANNELS_SELFTEST_HOLD"] ?? "") ?? 2
        let test = ChannelsSelfTest(workspace: workspace, dir: URL(fileURLWithPath: path), hold: hold)
        Task { @MainActor in await test.run() }
    }

    private init(workspace: DocumentWorkspace, dir: URL, hold: Double) {
        self.workspace = workspace
        self.dir = dir
        self.hold = hold
    }

    /// A line starting "FAIL" (an early exit: no library, no document, …) counts as a failure, so the
    /// closing `done, <n> failure(s)` is never a silent 0 for a run that did not happen.
    private func log(_ s: String) {
        if s.hasPrefix("FAIL") { failures += 1 }
        FileHandle.standardError.write(Data("channels-selftest: \(s)\n".utf8))
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

    private func mark(_ name: String) async {
        step += 1
        await pause(0.8)
        var frame = ""
        if let w = workspace.current?.viewport?.window, let screen = NSScreen.screens.first {
            SelfTestHost.raiseForCapture(w, floating: false)
            await pause(0.3)
            let f = w.frame
            frame = String(format: " window %.0f %.0f %.0f %.0f", f.minX, screen.frame.height - f.maxY, f.width, f.height)
        }
        log("step \(step) \(name)\(frame)")
        await pause(hold)
    }

    private func names() -> [String] { channels.records.map(\.name) }

    private static let skyColor = ToolColor(r: 0, g: 0.8, b: 0.2)
    private static let skyStyle = ChannelOverlayStyle(color: skyColor, opacity: 0.3, indicatesSelected: true)

    /// Equal within PSD's quantisation (16-bit colour, whole-percent opacity).
    private static func close(_ a: ChannelOverlayStyle) -> (ChannelOverlayStyle) -> Bool {
        { b in
            a.indicatesSelected == b.indicatesSelected && abs(a.opacity - b.opacity) < 0.006
                && max(abs(a.color.r - b.color.r), abs(a.color.g - b.color.g), abs(a.color.b - b.color.b)) < 0.001
        }
    }

    private func run() async {
        log("waiting for a document")
        guard await wait(60, { workspace.current != nil && workspace.current?.viewport != nil }), let doc = workspace.current else {
            log("FAIL no document"); log("done, \(failures) failure(s)"); return
        }
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let panelKey = "InspectorPanel.Channels"
        let wasExpanded = UserDefaults.standard.object(forKey: panelKey)
        UserDefaults.standard.set(true, forKey: panelKey)
        defer {
            if let wasExpanded { UserDefaults.standard.set(wasExpanded, forKey: panelKey) } else {
                UserDefaults.standard.removeObject(forKey: panelKey)
            }
        }
        let w = Int64(doc.info.width), h = Int64(doc.info.height)
        channels.reload(doc, force: true)
        let existing = channels.records.count

        // 1. Save a selection as a channel.
        let sky = CanvasRect(x: 0, y: 0, width: w, height: h * 2 / 5)
        doc.setMarquee(sky)
        channels.saveSelection(SaveSelectionForm(name: "Sky"))
        check("save selection", names() == Array(names().prefix(existing)) + ["Sky"], "\(names())")
        check("save is a history node", doc.history.last?.label == "Save Selection", doc.history.last?.label ?? "-")

        // 2. A spot channel from a second selection (undo removes it, redo restores it), shown in the
        //    overlay with the alpha channel.
        doc.setMarquee(CanvasRect(x: w / 2, y: h / 2, width: w / 3, height: h / 3))
        let count = channels.records.count
        channels.newSpot(name: "Varnish", color: ToolColor(r: 0.95, g: 0.75, b: 0.1), solidity: 0.6, fromSelection: true)
        let spot = channels.records.last
        check("spot channel", spot?.kind == .spot && spot?.name == "Varnish", "\(String(describing: spot))")
        doc.undo()
        channels.reload(doc, force: true)
        let afterUndo = channels.records.count
        doc.redo()
        channels.reload(doc, force: true)
        check("undo / redo spot channel", afterUndo == count && channels.records.last?.name == "Varnish",
              "\(afterUndo) \(names())")
        if let skyID = channels.records.first(where: { $0.name == "Sky" })?.id, let row = channels.rows(doc).first(where: { $0.channelID == skyID }) {
            channels.selectedChannel = skyID
            channels.toggleVisible(row)
        }
        if let spot, let row = channels.rows(doc).first(where: { $0.channelID == spot.id }) { channels.toggleVisible(row) }
        doc.setMarquee(nil)
        await mark("panel-overlay")

        // 3. The sheets.
        channels.sheet = .save
        doc.setMarquee(sky)
        await mark("save-sheet")
        channels.sheet = nil
        await pause(0.4)
        channels.sheet = .load
        await mark("load-sheet")
        channels.sheet = nil
        await pause(0.4)
        if let spot { channels.sheet = .options(spot.id) }
        await mark("spot-options-sheet")
        channels.sheet = nil
        await pause(0.4)

        // 3b. (B5-17b, ACCEPTANCE 485–487) Sky's Channel Options: green, 30 %, Selected Areas is one
        //     history node that undo / redo restore; the overlay reads the saved record.
        if let skyRecord = channels.records.first(where: { $0.name == "Sky" }) {
            channels.sheet = .options(skyRecord.id)
            await mark("alpha-options-sheet")
            channels.sheet = nil
            await pause(0.4)
            let nodes = doc.history.count
            var form = ChannelOptionsForm(skyRecord)
            form.indicates = .selectedAreas
            form.setColor(Self.skyColor)
            form.setOpacity(0.3)
            channels.applyOptions(skyRecord.id, form)
            check("alpha options: one node", doc.history.count == nodes + 1 && doc.history.last?.label == "Channel Options",
                  "\(doc.history.count - nodes) \(doc.history.last?.label ?? "-")")
            check("alpha options: overlay from the record", channels.style(skyRecord.id) == Self.skyStyle,
                  "\(channels.style(skyRecord.id))")
            doc.undo()
            channels.reload(doc, force: true)
            check("alpha options: undo", channels.style(skyRecord.id) == .alphaDefault, "\(channels.style(skyRecord.id))")
            doc.redo()
            channels.reload(doc, force: true)
            check("alpha options: redo", channels.style(skyRecord.id) == Self.skyStyle, "\(channels.style(skyRecord.id))")
            if let row = channels.rows(doc).first(where: { $0.channelID == skyRecord.id }), !row.visible { channels.toggleVisible(row) }
            await mark("alpha-options-overlay")
            if let row = channels.rows(doc).first(where: { $0.channelID == skyRecord.id }), row.visible { channels.toggleVisible(row) }
        } else {
            check("alpha options: Sky channel", false, "\(names())")
        }

        // 4. Only the alpha channel: components hidden (grey view).
        if let rgb = channels.rows(doc).first(where: { $0.kind == .composite }) { channels.toggleVisible(rgb) }
        if let spot, let row = channels.rows(doc).first(where: { $0.channelID == spot.id }) { channels.toggleVisible(row) }
        await mark("alpha-only")
        if let rgb = channels.rows(doc).first(where: { $0.kind == .composite }) { channels.toggleVisible(rgb) }
        for r in channels.rows(doc) where r.editable && r.visible { channels.toggleVisible(r) }

        // 5. Quick Mask in and out.
        let before = doc.marquee
        channels.toggleQuickMask()
        check("quick mask on", channels.isQuickMask(doc) && names().contains(QuickMask.channelName), "\(names())")
        await mark("quick-mask")
        channels.toggleQuickMask()
        check("quick mask off", !channels.isQuickMask(doc) && !names().contains(QuickMask.channelName), "\(names())")
        check("quick mask restores the selection", doc.marquee == before, "\(String(describing: doc.marquee))")

        // 7. Save, close, reopen, load back (native and PSD).
        for ext in ["tessera-doc", "psd"] {
            guard let current = workspace.current else { break }
            let url = dir.appendingPathComponent("ChannelsSelfTest.\(ext)")
            try? FileManager.default.removeItem(at: url)
            do { try current.backend.saveAs(path: url.path) } catch { check("save \(ext)", false, error.localizedDescription); continue }
            current.reloadHistory()
            workspace.close(current)
            _ = await wait(5) { workspace.current !== current }
            workspace.open(url)
            let ok = await wait(30) { workspace.current?.title == url.lastPathComponent }
            guard ok, let reopened = workspace.current else { check("reopen \(ext)", false); continue }
            channels.reload(reopened, force: true)
            check("reopen \(ext) keeps channels", names().contains("Sky") && names().contains("Varnish"), "\(names())")
            check("reopen \(ext) keeps the spot ink", channels.records.first { $0.name == "Varnish" }?.kind == .spot)
            let st = channels.records.first { $0.name == "Sky" }?.overlayStyle
            check("reopen \(ext) keeps the alpha display (B5-17b)", st.map(Self.close(Self.skyStyle)) ?? false,
                  "\(String(describing: st))")
            reopened.setMarquee(nil)
            if let id = channels.records.first(where: { $0.name == "Sky" })?.id { channels.load(id) }
            check("load \(ext) channel back", reopened.marquee == sky, "\(String(describing: reopened.marquee))")
            await mark("reopened-\(ext)")
        }
        log("done, \(failures) failure(s)")
    }
}
