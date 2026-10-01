import AppKit
import SwiftUI
import TesseraCore

/// Test aid (WP B5-10): with `TESSERA_TEXT_SELFTEST=<dir>` in the environment, once a document is
/// open, runs ACCEPTANCE §AA (steps 340–359) through the real app paths: synthesized mouse events
/// into the viewport, key events posted to the application queue (so `KeyRouter`'s monitor, the
/// menu and `TextInputView` all see them), `NSTextInputClient` calls for IME composition, and the
/// Character / Paragraph model the inspector uses. Checks print `text-selftest: check <name> ok|FAIL`,
/// screenshots of the Tessera window region go to `<dir>/<step>.png` (`screencapture -x -R`), and the
/// run ends with `done, <n> failure(s)`. `TESSERA_TEXT_SELFTEST_ONLY=inspector` stops after the
/// 1440-pt inspector capture.
@MainActor
final class TextSelfTest {
    private let workspace: DocumentWorkspace
    private let dir: URL
    private var failures = 0
    private var text: DocumentText { .shared }
    private static var started = false

    static func startIfRequested(_ workspace: DocumentWorkspace) {
        guard !started, let path = ProcessInfo.processInfo.environment["TESSERA_TEXT_SELFTEST"] else { return }
        started = true
        let t = TextSelfTest(workspace: workspace, dir: URL(fileURLWithPath: path))
        Task { @MainActor in await t.run() }
    }

    private init(workspace: DocumentWorkspace, dir: URL) {
        self.workspace = workspace
        self.dir = dir
    }

    /// A line starting "FAIL" (an early exit: no library, no document, …) counts as a failure, so the
    /// closing `done, <n> failure(s)` is never a silent 0 for a run that did not happen.
    private func log(_ s: String) {
        if s.hasPrefix("FAIL") { failures += 1 }
        FileHandle.standardError.write(Data("text-selftest: \(s)\n".utf8))
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

    private func settle() async {
        await text.idle()
        await DocumentTools.shared.idle()
        await pause(0.25)
    }

    /// Screenshot of this window only, without bringing the app forward: writes `<dir>/<name>.req`
    /// holding the window number and waits for `<name>.png`, which a watcher captures with
    /// `screencapture -x -o -l <window>` (the window is captured even behind other windows).
    private func shot(_ name: String) async {
        await settle()
        guard let w = workspace.current?.viewport?.window ?? NSApp.mainWindow else { return }
        w.displayIfNeeded()
        await pause(0.4)
        let png = dir.appendingPathComponent("\(name).png")
        try? FileManager.default.removeItem(at: png)
        try? "\(w.windowNumber)".write(to: dir.appendingPathComponent("\(name).req"), atomically: true, encoding: .utf8)
        let ok = await wait(60) { FileManager.default.fileExists(atPath: png.path) }
        log(String(format: "shot %@ window %ld %.0f × %.0f", name, w.windowNumber, w.frame.width, w.frame.height) + (ok ? "" : " (no watcher)"))
    }

    // MARK: Synthesized input

    private var viewport: DocumentViewportView? { workspace.current?.viewport }

    private func windowPoint(_ canvas: CGPoint) -> CGPoint? {
        guard let v = viewport else { return nil }
        return v.convert(v.viewPoint(canvas: canvas), to: nil)
    }

    private func mouse(_ type: NSEvent.EventType, _ canvas: CGPoint, flags: NSEvent.ModifierFlags = [], clicks: Int = 1) -> NSEvent? {
        guard let v = viewport, let w = v.window, let p = windowPoint(canvas) else { return nil }
        return NSEvent.mouseEvent(with: type, location: p, modifierFlags: flags, timestamp: ProcessInfo.processInfo.systemUptime,
                                  windowNumber: w.windowNumber, context: nil, eventNumber: 0, clickCount: clicks, pressure: 1)
    }

    private func click(_ p: CGPoint, flags: NSEvent.ModifierFlags = [], clicks: Int = 1) async {
        await quiet()
        guard let v = viewport, let d = mouse(.leftMouseDown, p, flags: flags, clicks: clicks),
              let u = mouse(.leftMouseUp, p, flags: flags, clicks: clicks) else { return }
        v.mouseDown(with: d)
        v.mouseUp(with: u)
        await pause(0.05)
    }

    private func drag(_ a: CGPoint, _ b: CGPoint, flags: NSEvent.ModifierFlags = [], steps: Int = 8) async {
        await quiet()
        guard let v = viewport, let d = mouse(.leftMouseDown, a, flags: flags) else { return }
        v.mouseDown(with: d)
        for i in 1...steps {
            let t = Double(i) / Double(steps)
            if let m = mouse(.leftMouseDragged, CGPoint(x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t), flags: flags) {
                v.mouseDragged(with: m)
            }
            await pause(0.02)
        }
        if let u = mouse(.leftMouseUp, b, flags: flags) { v.mouseUp(with: u) }
        await pause(0.05)
    }

    private static let keyCodes: [Character: UInt16] = [
        "a": 0, "s": 1, "d": 2, "f": 3, "h": 4, "g": 5, "z": 6, "x": 7, "c": 8, "v": 9, "b": 11, "q": 12, "w": 13, "e": 14,
        "r": 15, "y": 16, "t": 17, "1": 18, "2": 19, "3": 20, "4": 21, "6": 22, "5": 23, "9": 25, "7": 26, "8": 28, "0": 29,
        "o": 31, "u": 32, "i": 34, "p": 35, "l": 37, "j": 38, "k": 40, "n": 45, "m": 46, " ": 49, ",": 43, ".": 47,
    ]

    /// The app's key routing, reproduced for a window that is not frontmost (the app never takes
    /// focus): the document key monitor (`KeyRouter`, the same logic the installed monitor runs), then
    /// key equivalents (window, then the main menu), then the window's first responder.
    private lazy var router = KeyRouter(model: AppModel.shared)
    private func deliver(_ e: NSEvent, in w: NSWindow) {
        if e.type == .keyDown {
            if router.handle(e) { return }
            if e.modifierFlags.contains(.command) {
                if w.performKeyEquivalent(with: e) { return }
                if NSApp.mainMenu?.performKeyEquivalent(with: e) == true { return }
            }
        } else if router.handleKeyUp(e) { return }
        w.sendEvent(e)
    }

    /// A key down/up pair routed as the app routes keys (see `deliver`).
    private func key(_ ch: String, code: UInt16? = nil, flags: NSEvent.ModifierFlags = []) {
        guard let w = viewport?.window else { return }
        let lower = ch.lowercased()
        let kc = code ?? Self.keyCodes[lower.first ?? " "] ?? 0
        let chars = flags.contains(.shift) ? ch.uppercased() : ch
        for type in [NSEvent.EventType.keyDown, .keyUp] {
            if let e = NSEvent.keyEvent(with: type, location: .zero, modifierFlags: flags, timestamp: ProcessInfo.processInfo.systemUptime,
                                        windowNumber: w.windowNumber, context: nil, characters: chars,
                                        charactersIgnoringModifiers: lower, isARepeat: false, keyCode: kc) {
                deliver(e, in: w)
            }
        }
    }

    /// Keeps the window at the width under test (the app never comes forward; nothing else touches it).
    private func quiet() async {
        if let w = viewport?.window, abs(w.frame.width - wantedWidth) > 1 { await setWindowWidth(wantedWidth) }
    }

    private func type(_ s: String, interval: Double = 0.012) async {
        await quiet()
        guard text.isEditing, viewport?.window?.firstResponder === text.input else {
            log("type: no text session has the keyboard; not typing \"\(s)\"")
            return
        }
        for ch in s {
            if let d = doc, d.tool != .type { log("type: tool changed to \(d.tool) before \"\(ch)\"") }
            if Self.keyCodes[Character(ch.lowercased())] != nil {
                key(String(ch), flags: ch.isUppercase ? .shift : [])
            } else {
                text.input.insertText(String(ch), replacementRange: NSRange(location: NSNotFound, length: 0))
            }
            await pause(interval)
        }
        await pause(0.2)
    }

    private func enterKey() async {
        await pause(0.3)
        key("\u{3}", code: 76)
        await pause(0.4)
        if let d = doc, d.tool != .type { log("enter: tool is \(d.tool)") }
    }
    private func escKey() async { key("\u{1b}", code: 53); await pause(0.3) }

    // MARK: Model reads

    private var doc: DocumentController? { workspace.current }
    private var tb: (any DocumentTextBackend)? { doc?.backend as? any DocumentTextBackend }
    private func model(_ id: DocLayerID) -> TextSourceModel? { try? tb?.textLayer(id: id).model }
    private func historyCount() -> Int { doc?.history.count ?? 0 }
    private var statusMessage: String { workspace.app?.statusMessage ?? "" }
    private func textLayers() -> [DocLayerID] { doc?.layers.filter { $0.kind == .text }.map(\.id) ?? [] }
    /// The text layer added since `before` (ids, not row order: rows are top-first).
    private func added(since before: [DocLayerID]) -> DocLayerID? { textLayers().first { !before.contains($0) } }

    private var wantedWidth: CGFloat = 1440
    private func setWindowWidth(_ w: CGFloat) async {
        wantedWidth = w
        guard let win = viewport?.window, let screen = win.screen ?? NSScreen.main else { return }
        if win.styleMask.contains(.fullScreen) { win.toggleFullScreen(nil); await pause(1.5) }
        if win.isZoomed { win.zoom(nil); await pause(0.5) }
        // Top edge 470 pt below the screen top (clear of floating notch widgets), left edge at the screen's.
        let top = screen.frame.maxY - (Double(ProcessInfo.processInfo.environment["TESSERA_TEXT_SELFTEST_TOP"] ?? "") ?? 470)
        let h = min(850, top - screen.visibleFrame.minY)
        win.setFrame(CGRect(x: screen.frame.minX, y: top - h, width: min(w, screen.frame.width), height: h), display: true)
        await pause(0.5)
        if abs(win.frame.width - min(w, screen.frame.width)) > 1 {
            log(String(format: "window resize to %.0f refused (now %.0f × %.0f, style %lu)", w, win.frame.width, win.frame.height, win.styleMask.rawValue))
        }
    }

    // MARK: Run

    private func run() async {
        log("waiting for a document")
        guard await wait(90, { workspace.current?.viewport != nil }), let doc else {
            log("FAIL no document"); log("done, \(failures) failure(s)"); return
        }
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        for p in ["Properties", "History", "Channels", "Color", "Brushes"] { UserDefaults.standard.set(true, forKey: "InspectorPanel." + p) }
        workspace.app?.showInspector = true
        await setWindowWidth(1440)
        await pause(1.5)
        let W = Double(doc.info.width), H = Double(doc.info.height)
        let size = Float(max(W, H) / 20)
        text.size = size
        text.family = "Helvetica"

        if ProcessInfo.processInfo.environment["TESSERA_TEXT_SELFTEST_ONLY"] == "latency" {
            await measureLatency(doc, W: W, H: H)
            log("done, \(failures) failure(s)")
            return
        }

        // 341: T (posted key) selects Type; a canvas click creates point text.
        doc.viewport?.window?.makeFirstResponder(doc.viewport)
        key("t")
        _ = await wait(3) { doc.tool == .type }
        check("341 T selects Type", doc.tool == .type, "\(doc.tool)")
        let layersBefore = doc.layers.count
        let texts0 = textLayers()
        await click(CGPoint(x: W * 0.1, y: H * 0.2))
        check("341 click starts a session", text.isEditing(doc))
        await type("Hello World")
        await enterKey()
        await settle()
        let point = added(since: texts0)
        check("341 point text layer created", doc.layers.count == layersBefore + 1 && point != nil, "\(doc.layers.count)")
        if let point { check("341 text", model(point)?.text == "Hello World", model(point)?.text ?? "-") }

        // 340: all panels expanded at 1440 pt with a text layer selected.
        if let point { doc.selection = [point] }
        layoutReport()
        await shot("340-inspector-1440")
        if ProcessInfo.processInfo.environment["TESSERA_TEXT_SELFTEST_ONLY"] == "inspector" {
            log(String(format: "initial window %.0f", viewport?.window?.frame.width ?? 0))
            let order = (ProcessInfo.processInfo.environment["TESSERA_TEXT_WIDTHS"] ?? "1280,1366,1440,1600").split(separator: ",").compactMap { Double($0) }
            for w in order {
                await setWindowWidth(w)
                layoutReport()
                await shot("340-inspector-\(Int(w))")
            }
            log("done, \(failures) failure(s)")
            return
        }
        await shot("341-point-text")

        // 342: a drag creates area text that wraps inside its box.
        let texts1 = textLayers()
        let boxRect = CGRect(x: W * 0.1, y: H * 0.35, width: W * 0.35, height: H * 0.4)
        await drag(boxRect.origin, CGPoint(x: boxRect.maxX, y: boxRect.maxY))
        check("342 drag starts an area session", text.session?.edit.model.textBox.isParagraph == true)
        await type("Area text wraps inside its box without stretching")
        let lines = text.index?.layout.lines.count ?? 0
        check("342 wraps", lines > 1, "\(lines) lines")
        await enterKey()
        await settle()
        if ProcessInfo.processInfo.environment["TESSERA_TEXT_SELFTEST_ONLY"] == "type" { log("tool \(doc.tool)"); log("done, \(failures) failure(s)"); return }
        let area = added(since: texts1)
        await shot("342-area-text")

        // 343 / 344: select on canvas, style the selection, insert and delete across mixed runs.
        guard let area, let areaModel = model(area) else { check("342 area layer", false); log("done, \(failures) failure(s)"); return }
        let tA = (try? tb?.textLayer(id: area).transform) ?? .identity
        text.beginExisting(doc, layer: area)
        await settle()
        if let i = text.index {
            // Drag-select "text" (bytes 5..<9) with real mouse events.
            let a = tA.apply(CGPoint(x: i.caret(at: 5).top.x + 0.5, y: (i.caret(at: 5).top.y + i.caret(at: 5).bottom.y) / 2))
            let b = tA.apply(CGPoint(x: i.caret(at: 9).top.x - 0.5, y: (i.caret(at: 9).top.y + i.caret(at: 9).bottom.y) / 2))
            await drag(a, b)
        }
        check("343 drag selects", text.session?.edit.selection == 5..<9, "\(String(describing: text.session?.edit.selection))")
        let h344 = historyCount()
        text.character(doc, "Font Style") { $0.weight = 700 }
        text.character(doc, "Text Color") { $0.color = [200, 30, 30, 255] }
        text.character(doc, "Font Size", final: true) { $0.size = size * 1.4 }
        await settle()
        let styled = model(area)
        check("344 three nodes", historyCount() == h344 + 3, "\(historyCount() - h344)")
        check("344 styled run", styled?.runs.contains { $0.text == "text" && $0.weight == 700 && $0.color == [200, 30, 30, 255] } == true,
              "\(styled?.runs.map(\.text) ?? [])")
        check("344 untouched runs", styled?.runs.first?.text == "Area " && styled?.runs.first?.weight == areaModel.runs.first?.weight)
        // Insert inside the styled run and delete across the run boundary.
        text.session.map { _ in text.command(#selector(NSResponder.moveLeft(_:))) }
        await type("X")
        text.command(#selector(NSResponder.deleteBackward(_:)))
        text.command(#selector(NSResponder.deleteBackward(_:)))
        await settle()
        // ← collapses the selection to its start (5), X is typed, two ⌫ remove X and the space before it.
        check("343 edit across runs", text.session?.edit.text.hasPrefix("Areatext wraps") == true, text.session?.edit.text ?? "-")
        check("343 styled run intact", text.session?.edit.model.runs.contains { $0.text.hasPrefix("text") && $0.weight == 700 } == true,
              "\(text.session?.edit.model.runs.map(\.text) ?? [])")
        await shot("343-344-mixed-styles")

        // 345: tracking, leading, baseline shift: one node each (drags preview live).
        text.selectAll()
        let h345 = historyCount()
        for v in stride(from: 0.0, through: 6.0, by: 1.5) { text.character(doc, "Tracking", final: false) { $0.tracking = Float(v) } }
        text.character(doc, "Tracking", final: true) { $0.tracking = 6 }
        text.character(doc, "Leading", final: true) { $0.leading = size * 1.6 }
        text.character(doc, "Baseline Shift", final: true) { $0.baselineShift = size * 0.2 }
        await settle()
        check("345 one node per control edit", historyCount() - h345 <= 4 && historyCount() - h345 >= 3, "\(historyCount() - h345)")
        let labels = doc.history.suffix(3).map(\.label)
        check("345 labels", labels == ["Tracking", "Leading", "Baseline Shift"], "\(labels)")

        // 346: alignment, indents, spacing keep run styles and geometry.
        let runsBefore = model(area)?.runs
        let boxBefore = model(area)?.textBox
        text.paragraph(doc, "Align Center") { $0.paragraph.alignment = .center }
        text.paragraph(doc, "Left indent") { $0.paragraph.leftIndent = size * 0.5 }
        text.paragraph(doc, "Space after") { $0.paragraph.spaceAfter = size * 0.3 }
        await settle()
        let m346 = model(area)
        check("346 runs kept", m346?.runs == runsBefore)
        check("346 box kept", m346?.textBox == boxBefore)
        check("346 paragraph", m346?.paragraph.alignment == .center && m346?.paragraph.leftIndent == size * 0.5)
        await shot("345-346-character-paragraph")

        // 347: resize the box with its handle: wrap changes, no stretching (the model's size is new).
        if let box = m346?.textBox.size {
            let corner = tA.apply(CGPoint(x: box.width, y: box.height))
            let linesBefore = text.index?.layout.lines.count ?? 0
            await drag(corner, CGPoint(x: corner.x - box.width * 0.45, y: corner.y - box.height * 0.3))
            await settle()
            let nb = model(area)?.textBox.size
            check("347 box resized", nb.map { $0.width < box.width * 0.7 } ?? false, "\(String(describing: nb))")
            let after = text.index?.layout
            check("347 rewraps or overflows", (after?.lines.count ?? 0) != linesBefore || after?.overflow == true,
                  "\(linesBefore) → \(after?.lines.count ?? 0), overflow \(after?.overflow ?? false)")
            check("347 same glyph size", model(area)?.runs.map(\.size) == runsBefore?.map(\.size))
            await shot("347-resized-box")
            // B5-10c (1): the handle drag gives the keyboard back to the text; ⌘Return applies.
            check("B5-10c 347 keyboard back on the text", viewport?.window?.firstResponder === text.input,
                  "\(String(describing: viewport?.window?.firstResponder))")
            await type(" ok")
            let h = historyCount()
            key("\r", code: 36, flags: .command)
            await settle()
            check("B5-10c 347 ⌘Return applies after a handle drag", !text.isEditing && historyCount() == h + 1,
                  "editing \(text.isEditing), \(historyCount() - h) node(s)")
            // …and with another view of the window focused.
            text.beginExisting(doc, layer: area)
            await settle()
            await type("!")
            viewport?.window?.makeFirstResponder(viewport)
            let h2 = historyCount()
            key("\r", code: 36, flags: .command)
            await settle()
            check("B5-10c ⌘Return applies with the viewport focused", !text.isEditing && historyCount() == h2 + 1,
                  "editing \(text.isEditing), \(historyCount() - h2) node(s)")
        }

        // 348: caret alignment at fit / 100 % / 200 % with pan and a rotated layer.
        text.apply()
        await settle()
        if let point {
            text.beginExisting(doc, layer: point)
            await settle()
            // B5-10c (3): the hint follows the session (not the area text's).
            check("B5-10c hint for point text", statusMessage.hasPrefix("Point text"), statusMessage)
            let c = tapCenter()
            await drag(CGPoint(x: c.x + W * 0.25, y: c.y), CGPoint(x: c.x + W * 0.25, y: c.y + H * 0.12), flags: .command)
            await settle()
            let rotated = (try? tb?.textLayer(id: point).transform) ?? .identity
            check("348 rotated", abs(rotated.b) > 0.05, "\(rotated)")
            for (name, zoom) in [("fit", 0.0), ("100", 1.0), ("200", 2.0)] {
                if zoom == 0 { doc.viewport?.zoomToFit() } else {
                    doc.viewport?.zoomToFit()
                    while (doc.viewport?.zoom ?? 1) < zoom * (doc.viewport?.window?.backingScaleFactor ?? 2) * 0.99 { doc.viewport?.zoomIn() }
                }
                if let o = text.session.map({ tr in tr.transform.apply(.zero) }) { centre(on: o) }
                await settle()
                check("348 caret maps at \(name)", caretMatchesGlyph(), "")
                await shot("348-caret-\(name)")
            }
            doc.viewport?.zoomToFit()
            text.apply()
            await settle()
        }

        // 349: ligatures, combining marks and non-BMP input keep clusters whole.
        await click(CGPoint(x: W * 0.55, y: H * 0.2))
        await type("office e")
        text.input.insertText("\u{301} \u{1D400}\u{1F44D}\u{1F3FD}", replacementRange: NSRange(location: NSNotFound, length: 0))
        await settle()
        let s349 = text.session?.edit.text ?? ""
        check("349 text", s349 == "office e\u{301} \u{1D400}\u{1F44D}\u{1F3FD}", s349)
        text.command(#selector(NSResponder.deleteBackward(_:)))
        check("349 ⌫ removes the whole emoji", text.session?.edit.text == "office e\u{301} \u{1D400}", text.session?.edit.text ?? "-")
        let stops = text.index?.caretStops ?? []
        check("349 no stop inside a mark or a scalar", !stops.contains(8) && !stops.contains(12), "\(stops)")
        await shot("349-clusters")
        await enterKey()
        await settle()

        // 350: mixed RTL / LTR: click in the RTL run, replace, source intact.
        await click(CGPoint(x: W * 0.55, y: H * 0.85))
        await type("abc ")
        text.input.insertText("\u{5D0}\u{5D1}\u{5D2}", replacementRange: NSRange(location: NSNotFound, length: 0))
        await type(" def")
        await settle()
        if let i = text.index, let s = text.session {
            let hebrew = i.layout.glyphs.filter { $0.rtl }
            check("350 RTL glyphs", hebrew.count == 3, "\(hebrew.count)")
            if let bet = hebrew.first(where: { $0.cluster == 6 }) {
                // Right half of ב: the caret goes before ב (its visual right edge).
                let p = s.transform.apply(CGPoint(x: bet.x + bet.advance * 0.8, y: bet.y - 2))
                await click(p)
                check("350 visual hit", text.session?.edit.caret == 6, "\(String(describing: text.session?.edit.caret))")
                text.command(#selector(NSResponder.moveRightAndModifySelection(_:)))
                text.input.insertText("\u{5D3}", replacementRange: NSRange(location: NSNotFound, length: 0))
                check("350 replacement", text.session?.edit.text == "abc \u{5D0}\u{5D3}\u{5D2} def", text.session?.edit.text ?? "-")
            }
        }
        await shot("350-bidi")
        await enterKey()
        await settle()

        // 351: IME marked text: cancel leaves nothing, commit is one node.
        if let point {
            text.beginExisting(doc, layer: point)
            await settle()
            let before = model(point)?.text
            let h351 = historyCount()
            text.input.setMarkedText("ka", selectedRange: NSRange(location: 2, length: 0), replacementRange: NSRange(location: NSNotFound, length: 0))
            text.input.setMarkedText("か", selectedRange: NSRange(location: 1, length: 0), replacementRange: NSRange(location: NSNotFound, length: 0))
            check("351 marked", text.input.hasMarkedText() && text.input.markedRange().length == 1)
            await shot("351-marked-text")
            text.cancel()   // Esc during composition
            check("351 cancel", text.session?.edit.text == before && !text.input.hasMarkedText(), text.session?.edit.text ?? "-")
            text.input.setMarkedText("かん", selectedRange: NSRange(location: 2, length: 0), replacementRange: NSRange(location: NSNotFound, length: 0))
            text.input.insertText("漢", replacementRange: NSRange(location: NSNotFound, length: 0))
            text.apply()
            await settle()
            check("351 one node", historyCount() == h351 + 1, "\(historyCount() - h351)")
            check("351 committed", model(point)?.text == (before ?? "") + "漢", model(point)?.text ?? "-")
            doc.undo()
            await settle()
            check("351 one undo", model(point)?.text == before, model(point)?.text ?? "-")
            doc.redo()
            await settle()
        }

        // 352: T V X D Q, digits, Space, ⌫ and ⌘A / ⌘C / ⌘V type or edit text only.
        if let point {
            text.beginExisting(doc, layer: point)
            await settle()
            let colors = DocumentTools.shared.colors
            let nLayers = doc.layers.count
            let opacity = doc.node(point)?.opacity
            await type(" tvxdq 1 2 ")
            key("\u{7f}", code: 51)
            await pause(0.5)
            for k in ["a", "c", "v", "v"] {
                if viewport?.window?.firstResponder !== text.input { log("352: the text lost the keyboard before ⌘\(k)") }
                key(k, flags: .command)
                await pause(0.8)
                await settle()
            }
            let s = text.session?.edit.text ?? ""
            check("352 letters typed", s.contains("tvxdq 1 2"), s)
            check("352 tool unchanged", doc.tool == .type, "\(doc.tool)")
            check("352 layers unchanged", doc.layers.count == nLayers, "\(doc.layers.count)")
            check("352 colours unchanged", DocumentTools.shared.colors == colors)
            check("352 opacity unchanged", doc.node(point)?.opacity == opacity)
            check("352 ⌘A ⌘C ⌘V pasted twice", s.components(separatedBy: "tvxdq").count == 3, s)
            key("z", flags: .command)
            await pause(0.4)
            check("352 ⌘Z reverts typing", text.session?.edit.isChanged == false, text.session?.edit.text ?? "-")
            await shot("352-keys")
            await escKey()
        }

        // 353: typing group, undo / redo exact.
        if let point {
            text.beginExisting(doc, layer: point)
            await settle()
            // B5-10c (3): after 352's Esc ("Type: cancelled") a new edit shows the editing hint.
            check("B5-10c hint after a cancel", statusMessage.hasPrefix("Point text"), statusMessage)
            let before = model(point)
            let h353 = historyCount()
            await type(" again")
            await enterKey()
            await settle()
            let after = model(point)
            check("353 one group", historyCount() == h353 + 1, "\(historyCount() - h353)")
            doc.undo()
            await settle()
            check("353 undo exact", model(point) == before)
            doc.redo()
            await settle()
            check("353 redo exact", model(point) == after)
        }

        // B5-10c (2): a click just right of the last glyph resumes the layer (no new text).
        if let point, let m = model(point), let layout = try? tb?.layoutText(m), let b = layout.bounds, let line = layout.lines.last,
           let t = try? tb?.textLayer(id: point).transform {
            text.apply()
            await settle()
            let n = doc.layers.count
            await click(t.apply(CGPoint(x: b.maxX + Double(line.ascent + line.descent) * 0.3, y: line.baseline - line.ascent * 0.4)))
            await settle()
            check("B5-10c click right of the last glyph resumes", text.session?.layer == point,
                  "\(String(describing: text.session?.layer))")
            check("B5-10c caret at the end", text.session?.edit.caret == m.utf8Count, "\(String(describing: text.session?.edit.caret))")
            await escKey()
            await settle()
            check("B5-10c no new layer", doc.layers.count == n, "\(doc.layers.count - n)")
        }

        // 356: styled and masked text → pixels, undo restores the text (before a document switch).
        if let point {
            doc.selection = [point]
            doc.addMask(.revealAll)
            await settle()
            let hBefore = historyCount()
            text.convertToPixels(doc)
            await settle()
            check("356 converted", doc.node(point)?.kind == .pixel && doc.node(point)?.hasMask == true)
            await shot("356-converted")
            check("356 one node", historyCount() == hBefore + 1)
            doc.undo()
            await settle()
            check("356 undo restores text", doc.node(point)?.kind == .text && model(point) != nil)
        }

        // 355: locks.
        if let point {
            doc.selection = [point]
            doc.toggleLock(.pixels)
            await settle()
            text.beginExisting(doc, layer: point)
            await settle()
            let before = model(point)
            await type("zz")
            await enterKey()
            await settle()
            check("355 pixel lock rejects content", model(point) == before)
            doc.selection = [point]
            doc.toggleLock(.pixels)
            doc.toggleLock(.position)
            await settle()
            text.beginExisting(doc, layer: point)
            await settle()
            let t0 = try? tb?.textLayer(id: point).transform
            let c = tapCenter()
            await drag(c, CGPoint(x: c.x + 40, y: c.y + 40), flags: .command)
            await settle()
            check("355 position lock rejects moves", (try? tb?.textLayer(id: point).transform) == t0)
            await type("!")
            await enterKey()
            await settle()
            check("355 position lock allows content", model(point)?.text.hasSuffix("!") == true, model(point)?.text ?? "-")
            doc.selection = [point]
            doc.toggleLock(.position)
            await settle()
        }

        // 357 / 358: native and PSD reopen; converted layers do not come back as text.
        let convertMe = textLayers().first { $0 != point && $0 != area }
        if let convertMe {
            doc.selection = [convertMe]
            text.convertToPixels(doc)
            await settle()
        }
        let expectText = textLayers().count
        for ext in ["tessera-doc", "psd"] {
            guard let current = workspace.current else { break }
            let url = dir.appendingPathComponent("TextSelfTest.\(ext)")
            try? FileManager.default.removeItem(at: url)
            do { try current.backend.saveAs(path: url.path) } catch { check("save \(ext)", false, error.localizedDescription); continue }
            workspace.discard(current)
            workspace.open(url)
            let ok = await wait(60) { workspace.current?.title == url.lastPathComponent }
            guard ok, let reopened = workspace.current else { check("reopen \(ext)", false); continue }
            await settle()
            let texts = reopened.layers.filter { $0.kind == .text }
            check("\(ext == "psd" ? "358" : "357") text layers survive", texts.count == expectText, "\(texts.count) vs \(expectText)")
            if let convertMe {
                check("358 converted stays pixels (\(ext))", reopened.layers.first { $0.name == doc.node(convertMe)?.name }.map { $0.kind != .text } ?? true)
            }
            if let first = texts.first {
                reopened.tool = .type
                text.beginExisting(reopened, layer: first.id)
                await settle()
                await type(" re")
                await enterKey()
                await settle()
                check("\(ext == "psd" ? "358" : "357") editable after reopen", model(first.id)?.text.hasSuffix(" re") == true)
            }
            await shot("\(ext == "psd" ? "358" : "357")-reopened-\(ext)")
        }

        // 354: a draft cancelled, then a document switch: no stale caret, preview or node.
        if let current = workspace.current {
            let h = historyCount(), n = current.layers.count
            current.tool = .type
            await click(CGPoint(x: W * 0.3, y: H * 0.5))
            await type("draft")
            await settle()
            check("354 draft shows", current.layers.count == n + 1)
            await escKey()
            await settle()
            check("354 cancel: no layer, no node", current.layers.count == n && historyCount() == h && !text.isEditing)
            workspace.newDocument(NewDocumentSettings())
            _ = await wait(10) { workspace.current !== current }
            await settle()
            workspace.select(current)
            await settle()
            check("354 no stale session", !text.isEditing && current.layers.count == n && historyCount() == h)
            await shot("354-cancel-switch")
        }

        // 359: explicit limitations (missing font, warp caret, colour) and the 1440 inspector again.
        if let current = workspace.current, let t = current.backend as? any DocumentTextBackend {
            var warped = TextSourceModel.point("Warped", family: "Helvetica", size: size)
            warped.warp.amount = 0.3
            let missing = TextSourceModel.point("Missing", family: "No Such Font Family", size: size)
            let a = try? t.addTextLayer(name: "Warped", parent: nil, index: nil, model: warped, transform: .translation(W * 0.5, H * 0.5), interactive: false)
            _ = try? t.addTextLayer(name: "Missing", parent: nil, index: nil, model: missing, transform: .translation(W * 0.5, H * 0.7), interactive: false)
            current.reloadModel()
            current.reloadHistory()
            if let id = a?.created.first {
                current.selection = [id]
                current.tool = .type
                let src = try? t.textLayer(id: id)
                check("359 warp caret disabled", src?.caretEditable == false && src?.limitations.contains { $0.hasPrefix("Warped") } == true)
                await shot("359-warp-limitation-1440")
            }
            if let m = current.layers.first(where: { $0.name == "Missing" }) {
                current.selection = [m.id]
                let src = try? t.textLayer(id: m.id)
                check("359 missing font explicit", src?.limitations.contains { $0.contains("Missing font") } == true)
                await shot("359-missing-font-1440")
            }
        }

        log("latency \(text.latencyReadout ?? "none")")
        log("done, \(failures) failure(s)")
    }

    // MARK: Helpers

    /// Keystroke → presented frame on this (20 MP) document: point text at 1/20 of the long edge,
    /// typed at 8 keys a second at Fit, then at 100 %.
    private func measureLatency(_ doc: DocumentController, W: Double, H: Double) async {
        doc.viewport?.window?.makeFirstResponder(doc.viewport)
        key("t")
        _ = await wait(3) { doc.tool == .type }
        doc.viewport?.zoomToFit()
        await click(CGPoint(x: W * 0.1, y: H * 0.3))
        let t0 = Date()
        await type("The quick brown fox jumps over the lazy dog", interval: 0.125)
        await settle()
        log(String(format: "latency fit: %@ (typing %.1f s)", text.latencyReadout ?? "none", Date().timeIntervalSince(t0)))
        await enterKey()
        await settle()
    }

    /// Evidence for the 1440-pt fix: the window width and the width SwiftUI gives the split view host
    /// (they must match; before the fix the host stayed ~1450 pt wide and overflowed the window).
    private func layoutReport() {
        guard let w = viewport?.window, let content = w.contentView else { return }
        var host: CGFloat = 0
        func walk(_ v: NSView) {
            if host == 0, v is NSSplitView, let s = v.superview { host = s.frame.width; return }
            v.subviews.forEach(walk)
        }
        walk(content)
        check(String(format: "340 split view fits the %.0f pt window", w.frame.width), host <= w.frame.width + 0.5,
              String(format: "host %.0f", host))
    }

    /// Canvas centre of the edited text's frame.
    private func tapCenter() -> CGPoint {
        guard let s = text.session else { return .zero }
        let b = text.index?.layout.bounds ?? .zero
        return s.transform.apply(CGPoint(x: b.midX, y: b.midY))
    }

    private func centre(on canvas: CGPoint) {
        guard let v = viewport else { return }
        let here = v.viewPoint(canvas: canvas)
        v.panBy(dx: v.bounds.midX - here.x, dy: v.bounds.midY - here.y)
    }

    /// The caret at each stop sits on the engine glyph edge mapped through the layer affine and the
    /// viewport (within half a view point).
    private func caretMatchesGlyph() -> Bool {
        guard let v = viewport, let s = text.session, let i = text.index else { return false }
        for g in i.layout.glyphs where !g.rtl {
            let c = i.caret(at: g.cluster)
            let fromCaret = v.viewPoint(canvas: s.transform.apply(CGPoint(x: c.top.x, y: g.y)))
            let fromGlyph = v.viewPoint(canvas: s.transform.apply(CGPoint(x: g.x, y: g.y)))
            if i.layout.glyphs.filter({ $0.cluster == g.cluster }).count == 1, hypot(fromCaret.x - fromGlyph.x, fromCaret.y - fromGlyph.y) > 0.5 {
                return false
            }
        }
        return true
    }
}
