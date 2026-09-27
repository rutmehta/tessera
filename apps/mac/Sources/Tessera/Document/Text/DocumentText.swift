import AppKit
import Observation
import TesseraCore

/// The Type tool (WP B5-10): creating point text (click) and area text (drag), resuming an existing
/// text layer (click on it), the caret, drag selection, the paragraph box handles, ⌘-drag move and
/// rotate of the layer's affine, IME composition (through `TextInputView`), the Character / Paragraph
/// edits of the inspector, and explicit Apply / Esc.
///
/// History: every keystroke updates a host draft (`TextEditSession`) and sends the COMPLETE draft
/// as a preview (`interactive`, recorded nowhere); Apply records one node for the typing group.
/// A completed Character / Paragraph / box / move edit first records the pending typing, then its
/// own node. Esc drops the uncommitted draft with no history change. Engine calls run in order on
/// one serial queue; previews coalesce so at most one is in flight.
@MainActor @Observable
final class DocumentText {
    static let shared = DocumentText()

    @ObservationIgnored weak var workspace: DocumentWorkspace?
    var document: DocumentController? { workspace?.current }

    // MARK: New-text defaults (options bar)

    var family = "Helvetica"
    var weight: UInt16 = 400
    var italic = false
    var size: Float = 48
    var alignment: TextParagraphAlignment = .left
    private(set) var fonts: [TextFontFamilyInfo] = []

    // MARK: Session

    struct Session {
        let docID: String
        /// nil while the draft adds a new layer (until its first commit).
        var layer: DocLayerID?
        var parent: DocLayerID?
        var insertIndex: UInt32?
        var transform: AffineTransform2D
        var edit: TextEditSession
        var caretEditable = true
        var limitations: [String] = []
        /// The engine holds a draft of this session (a preview was sent).
        var previewed = false
    }

    private(set) var session: Session?
    /// The engine layout of the draft (the caret oracle).
    @ObservationIgnored private(set) var index: TextLayoutIndex?
    private(set) var layoutError: String?
    /// "Type: median 9.8 ms · p95 14 ms (42 keys)" (keystroke → presented frame).
    private(set) var latencyReadout: String?
    @ObservationIgnored private(set) var latencies: [Double] = []

    enum Gesture {
        case create(start: CGPoint, current: CGPoint)
        case select(anchor: Int)
        case move(last: CGPoint)
        case rotate(center: CGPoint, startAngle: Double, base: AffineTransform2D)
        case resize(handle: Int, box: CGSize, base: AffineTransform2D, start: CGPoint)
    }
    @ObservationIgnored private(set) var gesture: Gesture?

    let input = TextInputView()
    @ObservationIgnored private let queue = DispatchQueue(label: "dev.tessera.document-text", qos: .userInteractive)
    /// Engine-side ids known only on the queue (a created layer, the committed revision).
    private final class QueueState: @unchecked Sendable {
        var layer: DocLayerID?
        var revision: UInt64?
    }
    @ObservationIgnored private var queueState = QueueState()
    @ObservationIgnored private var previewInFlight = false
    @ObservationIgnored private var previewDirty = false
    @ObservationIgnored private var busy = 0
    @ObservationIgnored private var keyAt: Date?
    @ObservationIgnored private var awaitingEpochs: [(epoch: UInt64, at: Date)] = []
    @ObservationIgnored private var caretOn = true
    @ObservationIgnored private var blink: Timer?
    @ObservationIgnored private var hitCache: [DocLayerID: (revision: UInt64, source: TextLayerSource, layout: TextLayoutInfo)] = [:]

    private init() {}

    func attach(_ workspace: DocumentWorkspace) {
        self.workspace = workspace
    }

    var isEditing: Bool { session != nil }
    var isComposing: Bool { session?.edit.isComposing ?? false }
    func isEditing(_ doc: DocumentController) -> Bool { session?.docID == doc.id }

    private func backend(_ doc: DocumentController) -> (any DocumentTextBackend)? { doc.backend as? any DocumentTextBackend }
    private func say(_ s: String) { document?.report?(s) }
    private func redraw() { document?.viewport?.toolOverlay.needsDisplay = true }

    func loadFonts() {
        guard fonts.isEmpty else { return }
        fonts = TextBridge.fonts()
        if !fonts.contains(where: { $0.family == family }), let f = fonts.first { family = f.family }
    }

    /// Waits until queued engine work has finished (self-test).
    func idle() async {
        while busy > 0 || previewInFlight { try? await Task.sleep(for: .milliseconds(10)) }
    }

    // MARK: Engine queue

    private func enqueue<T: Sendable>(_ what: String, _ body: @escaping @Sendable (QueueState) throws -> T,
                                      done: (@MainActor (T) -> Void)? = nil, failed: (@MainActor () -> Void)? = nil) {
        busy += 1
        let state = queueState
        queue.async {
            let r = Result { try body(state) }
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    let me = DocumentText.shared
                    me.busy -= 1
                    switch r {
                    case .success(let v): done?(v)
                    case .failure(let e):
                        me.say("\(what): \(e.localizedDescription)")
                        failed?()
                    }
                }
            }
        }
    }

    /// Sends the newest draft unless one is in flight (then it follows when that one lands).
    private func schedulePreview() {
        previewDirty = true
        pumpPreview()
    }

    private func pumpPreview() {
        guard previewDirty, !previewInFlight, let s = session, let doc = document, doc.id == s.docID,
              let t = backend(doc), s.caretEditable || s.layer != nil else { return }
        previewDirty = false
        previewInFlight = true
        let (model, transform, parent, index) = (s.edit.model, s.transform, s.parent, s.insertIndex)
        let state = queueState
        let started = keyAt
        queue.async {
            let r = Result { () -> DocumentChange in
                if let layer = state.layer {
                    return try t.setTextLayer(id: layer, model: model, transform: transform, interactive: true,
                                              expectedRevision: state.revision)
                }
                if model.utf8Count == 0 { return try t.cancelSourcePreview() }
                return try t.addTextLayer(name: "", parent: parent, index: index, model: model, transform: transform,
                                          interactive: true)
            }
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    let me = DocumentText.shared
                    me.previewInFlight = false
                    switch r {
                    case .success(let c):
                        if me.session?.docID == doc.id { me.session?.previewed = model.utf8Count > 0 || state.layer != nil }
                        if let started { me.awaitingEpochs.append((c.epoch, started)) }
                    case .failure(let e):
                        me.say("Type: \(e.localizedDescription)")
                    }
                    me.pumpPreview()
                }
            }
        }
    }

    /// Records the draft as one history node labelled `label` (none when unchanged). With
    /// `end`, the session ends; otherwise it continues from the committed model.
    private func commitDraft(_ label: String, end: Bool, selectCreated: Bool = true) {
        guard let s = session, let doc = document, doc.id == s.docID, let t = backend(doc) else { return }
        previewDirty = false
        let (model, transform, parent, index) = (s.edit.model, s.transform, s.parent, s.insertIndex)
        let changed = s.edit.isChanged || s.previewed
        if end { endSession() } else { session?.edit.rebase(to: model); session?.previewed = false }
        guard changed else { return }
        let backendRef = doc.backend
        enqueue(label) { state -> DocumentChange? in
            if let layer = state.layer {
                _ = try t.setTextLayer(id: layer, model: model, transform: transform, interactive: true, expectedRevision: state.revision)
                let c = try backendRef.commit(label: label)
                state.revision = try t.textLayer(id: layer).revision
                return c
            }
            if model.utf8Count == 0 { _ = try t.cancelSourcePreview(); return nil }
            _ = try t.addTextLayer(name: "", parent: parent, index: index, model: model, transform: transform, interactive: true)
            let c = try backendRef.commit(label: label)
            if let id = c.created.first {
                state.layer = id
                state.revision = try t.textLayer(id: id).revision
            }
            return c
        } done: { [weak doc] c in
            guard let doc else { return }
            let me = DocumentText.shared
            if let id = c?.created.first {
                if me.session?.docID == doc.id, me.session?.layer == nil { me.session?.layer = id }
                if selectCreated { doc.selection = [id] }
            }
            doc.reloadModel()
            doc.reloadHistory()
            me.redraw()
        }
    }

    // MARK: Session lifecycle

    /// Starts editing a new layer at canvas point `p` (point text, baseline at the click) or in the
    /// canvas rectangle `box` (area text).
    private func beginNew(_ doc: DocumentController, at p: CGPoint, box: CGRect?) {
        loadFonts()
        var style = TextRunModel(text: "", family: family, weight: weight, italic: italic, size: size)
        style.setColor(DocumentTools.shared.colors.foreground)
        var model = TextSourceModel(runs: [style])
        model.paragraph.alignment = alignment
        var transform = AffineTransform2D.translation(p.x, p.y)
        if let box {
            model.textBox = .paragraph(width: Float(max(box.width, 8)), height: Float(max(box.height, 8)))
            transform = .translation(box.minX, box.minY)
        } else {
            // Point text: the first baseline sits at the click (the engine's first baseline is the ascent below 0).
            var probe = model
            probe.runs[0].text = "Ag"
            let probed = try? backend(doc)?.layoutText(probe)
            let ascent = probed?.lines.first?.baseline ?? Double(size) * 0.9
            transform = .translation(p.x, p.y - ascent)
        }
        let primary = doc.primary
        queueState = QueueState()
        session = Session(docID: doc.id, layer: nil, parent: primary?.parent, insertIndex: primary.map { $0.index + 1 },
                          transform: transform, edit: TextEditSession(model: model))
        relayout()
        focus(doc)
        say(box == nil ? "Point text: type, then Enter (keypad) or ⌘Return applies, Esc cancels"
                       : "Area text: type to wrap inside the box; Enter (keypad) or ⌘Return applies, Esc cancels")
    }

    /// Resumes editing text layer `id`, the caret at local point `local` (nil: at the end).
    @discardableResult
    func beginExisting(_ doc: DocumentController, layer id: DocLayerID, local: CGPoint? = nil) -> Bool {
        guard let t = backend(doc) else { say("Type: this document cannot edit text"); return false }
        do {
            let src = try t.textLayer(id: id)
            queueState = QueueState()
            queueState.layer = id
            queueState.revision = src.revision
            var s = Session(docID: doc.id, layer: id, transform: src.transform, edit: TextEditSession(model: src.model))
            s.caretEditable = src.caretEditable
            s.limitations = src.limitations
            session = s
            relayout()
            if let local, let index, src.caretEditable {
                let o = index.hitTest(local)
                session?.edit.select(o..<o)
            }
            if doc.selection != [id] { doc.selection = [id] }
            if src.caretEditable { focus(doc) } else { say("Type: " + (src.limitations.first ?? "canvas editing is unavailable")) }
            return true
        } catch {
            say("Type: \(error.localizedDescription)")
            return false
        }
    }

    private func focus(_ doc: DocumentController) {
        guard let v = doc.viewport else { return }
        if input.superview !== v {
            input.removeFromSuperview()
            input.frame = v.bounds
            input.autoresizingMask = [.width, .height]
            v.addSubview(input)
        }
        input.viewport = v
        v.window?.makeFirstResponder(input)
        startBlink()
    }

    private func endSession() {
        session = nil
        index = nil
        gesture = nil
        layoutError = nil
        blink?.invalidate()
        blink = nil
        if input.window?.firstResponder === input {
            input.inputContext?.discardMarkedText()
            input.window?.makeFirstResponder(input.viewport)
        }
        redraw()
    }

    /// Apply (Enter / ⌘Return / ✓ / tool or document switch / click elsewhere): one history node.
    func apply() {
        guard session != nil else { return }
        if isComposing { commitComposition() }
        let label = session?.layer == nil ? "Add Text" : "Edit Text"
        commitDraft(label, end: true)
    }

    /// Esc: drops the uncommitted draft; no history change.
    func cancel() {
        guard let s = session else { return }
        if s.edit.isComposing {
            session?.edit.cancelComposition()
            input.inputContext?.discardMarkedText()
            edited()
            return
        }
        let doc = document
        let hadPreview = s.previewed
        previewDirty = false
        endSession()
        guard hadPreview, let doc, doc.id == s.docID, let t = backend(doc) else { return }
        enqueue("Type") { _ in try t.cancelSourcePreview() } done: { [weak doc] _ in
            doc?.reloadModel()
            doc?.reloadHistory()
        }
        say("Type: cancelled")
    }

    /// Another document is about to show, the tool changed, or the document closes: apply.
    func documentWillChange() { apply() }

    /// History moved underneath (checkout, snapshot): the engine flushed the draft already.
    func historyDidMove(_ doc: DocumentController) {
        guard isEditing(doc) else { return }
        previewDirty = false
        endSession()
    }

    /// The layer went away (deleted, converted).
    func layersDidReload(_ doc: DocumentController) {
        hitCache = hitCache.filter { doc.node($0.key)?.kind == .text }
        guard let s = session, s.docID == doc.id, let id = s.layer, busy == 0, !previewInFlight else { return }
        if doc.node(id)?.kind != .text { endSession() }
    }

    // MARK: Layout

    private func relayout() {
        guard let s = session, let doc = document, let t = backend(doc) else { index = nil; return }
        do {
            let layout = try t.layoutText(s.edit.model)
            index = TextLayoutIndex(model: s.edit.model, layout: layout)
            layoutError = nil
        } catch {
            index = TextLayoutIndex(model: s.edit.model, layout: .empty)
            layoutError = error.localizedDescription
        }
        session?.edit.stops = index?.caretStops
        redraw()
    }

    /// After every draft change: layout, preview, redraw.
    private func edited() {
        keyAt = Date()
        relayout()
        if let s = session, let i = index {
            let a = i.snap(s.edit.selection.lowerBound), b = i.snap(s.edit.selection.upperBound, forward: true)
            if a != s.edit.selection.lowerBound || b != s.edit.selection.upperBound { session?.edit.select(a..<max(a, b)) }
        }
        caretOn = true
        schedulePreview()
    }

    // MARK: Keyboard (TextInputView)

    var selectedText: String { session?.edit.selectedText ?? "" }

    private var map: TextIndexMap { index?.map ?? TextIndexMap(session?.edit.text ?? "") }

    var selectedRange16: NSRange { session.map { map.utf16Range($0.edit.selection) } ?? NSRange(location: 0, length: 0) }
    var markedRange16: NSRange? { session?.edit.marked.map { map.utf16Range($0) } }

    func substring16(_ r: NSRange) -> (String, NSRange)? {
        guard session != nil else { return nil }
        let r8 = map.utf8Range(r)
        return (map.substring(r8), map.utf16Range(r8))
    }

    func insert(_ s: String, replacing r16: NSRange? = nil) {
        guard session?.caretEditable == true else { return }
        let r8 = r16.map { map.utf8Range($0) }
        session?.edit.insert(s, replacing: r8)
        edited()
    }

    func setMarked(_ s: String, selected: NSRange, replacing r16: NSRange?) {
        guard session?.caretEditable == true else { return }
        let sel = TextIndexMap(s).utf8Range(selected)
        session?.edit.setMarked(s, selected: sel, replacing: r16.map { map.utf8Range($0) })
        edited()
    }

    func commitComposition() {
        session?.edit.commitComposition()
        redraw()
    }

    func selectAll() {
        guard let s = session else { return }
        session?.edit.select(0..<s.edit.length)
        redraw()
    }

    func undo() {
        guard let s = session, let doc = document else { return }
        if s.edit.isChanged {
            // Undo while typing: back to the last committed model (nothing recorded).
            session?.edit.setModel(s.edit.base)
            edited()
            say("Undo typing")
        } else {
            // Nothing uncommitted: leave the text and undo the document once queued work landed.
            previewDirty = false
            endSession()
            whenIdle { [weak doc] in doc?.undo() }
        }
    }

    func redo() {
        guard let doc = document, session?.edit.isChanged != true else { return }
        previewDirty = false
        endSession()
        whenIdle { [weak doc] in doc?.redo() }
    }

    /// Runs `body` after every queued engine call.
    private func whenIdle(_ body: @escaping @MainActor () -> Void) {
        enqueue("Type") { _ in } done: { _ in body() }
    }

    /// The document is closing: forget the session (the engine session goes with it).
    func documentClosing(_ doc: DocumentController) {
        guard isEditing(doc) else { return }
        previewDirty = false
        endSession()
    }

    /// `doCommand(by:)` selectors of the key bindings.
    func command(_ sel: Selector) {
        guard var e = session?.edit, let i = index else { return }
        let c = e.caret
        func move(_ o: Int, extend: Bool) {
            let anchor = e.selection.lowerBound == c ? e.selection.upperBound : e.selection.lowerBound
            e.select(extend ? min(anchor, o)..<max(anchor, o) : o..<o)
        }
        switch NSStringFromSelector(sel) {
        case "insertNewline:", "insertLineBreak:", "insertParagraphSeparator:": insert("\n"); return
        case "insertTab:": insert("\t"); return
        case "deleteBackward:", "deleteBackwardByDecomposingPreviousCharacter:":
            e.deleteBackward(); session?.edit = e; edited(); return
        case "deleteForward:": e.deleteForward(); session?.edit = e; edited(); return
        case "deleteWordBackward:":
            let w = i.wordRange(at: i.previousStop(c)).lowerBound
            e.insert("", replacing: e.selection.isEmpty ? w..<c : e.selection); session?.edit = e; edited(); return
        case "deleteToBeginningOfLine:":
            e.insert("", replacing: i.lineBounds(of: c).lowerBound..<c); session?.edit = e; edited(); return
        case "cancelOperation:": cancel(); return
        case "moveLeft:": move(e.selection.isEmpty ? i.previousStop(c) : e.selection.lowerBound, extend: false)
        case "moveRight:": move(e.selection.isEmpty ? i.nextStop(c) : e.selection.upperBound, extend: false)
        case "moveLeftAndModifySelection:": move(i.previousStop(c), extend: true)
        case "moveRightAndModifySelection:": move(i.nextStop(c), extend: true)
        case "moveUp:": move(i.verticalMove(from: c, down: false), extend: false)
        case "moveDown:": move(i.verticalMove(from: c, down: true), extend: false)
        case "moveUpAndModifySelection:": move(i.verticalMove(from: c, down: false), extend: true)
        case "moveDownAndModifySelection:": move(i.verticalMove(from: c, down: true), extend: true)
        case "moveWordLeft:": move(i.wordRange(at: i.previousStop(c)).lowerBound, extend: false)
        case "moveWordRight:": move(i.wordRange(at: c).upperBound, extend: false)
        case "moveWordLeftAndModifySelection:": move(i.wordRange(at: i.previousStop(c)).lowerBound, extend: true)
        case "moveWordRightAndModifySelection:": move(i.wordRange(at: c).upperBound, extend: true)
        case "moveToBeginningOfLine:", "moveToLeftEndOfLine:": move(i.lineBounds(of: c).lowerBound, extend: false)
        case "moveToEndOfLine:", "moveToRightEndOfLine:": move(i.lineBounds(of: c).upperBound, extend: false)
        case "moveToBeginningOfLineAndModifySelection:", "moveToLeftEndOfLineAndModifySelection:":
            move(i.lineBounds(of: c).lowerBound, extend: true)
        case "moveToEndOfLineAndModifySelection:", "moveToRightEndOfLineAndModifySelection:":
            move(i.lineBounds(of: c).upperBound, extend: true)
        case "moveToBeginningOfDocument:": move(0, extend: false)
        case "moveToEndOfDocument:": move(e.length, extend: false)
        case "selectAll:": e.select(0..<e.length)
        default: return
        }
        session?.edit = e
        caretOn = true
        redraw()
    }

    // MARK: Coordinates

    private func local(_ canvas: CGPoint) -> CGPoint? { session?.transform.inverse?.apply(canvas) }

    /// View points (y down) → canvas pixels.
    private func canvasPoint(view p: CGPoint, in v: DocumentViewportView) -> CGPoint {
        let o = v.viewPoint(canvas: .zero), x = v.viewPoint(canvas: CGPoint(x: 1, y: 0)), y = v.viewPoint(canvas: CGPoint(x: 0, y: 1))
        let sx = x.x - o.x, sy = y.y - o.y
        return CGPoint(x: (p.x - o.x) / (sx == 0 ? 1 : sx), y: (p.y - o.y) / (sy == 0 ? 1 : sy))
    }

    func characterIndex16(atView p: CGPoint, in v: DocumentViewportView) -> Int? {
        guard let i = index, let l = local(canvasPoint(view: p, in: v)) else { return nil }
        return map.utf16(fromUTF8: i.hitTest(l))
    }

    /// Canvas bounding rectangle of a UTF-16 range (IME candidate placement).
    func canvasRect(for16 r: NSRange) -> CGRect? {
        guard let s = session, let i = index else { return nil }
        let r8 = map.utf8Range(r)
        var rects = i.selectionRects(r8)
        if rects.isEmpty {
            let c = i.caret(at: r8.lowerBound)
            rects = [CGRect(x: c.top.x, y: c.top.y, width: 1, height: c.bottom.y - c.top.y)]
        }
        let pts = rects.flatMap { [CGPoint(x: $0.minX, y: $0.minY), CGPoint(x: $0.maxX, y: $0.maxY),
                                   CGPoint(x: $0.minX, y: $0.maxY), CGPoint(x: $0.maxX, y: $0.minY)] }.map(s.transform.apply)
        let xs = pts.map(\.x), ys = pts.map(\.y)
        return CGRect(x: xs.min()!, y: ys.min()!, width: xs.max()! - xs.min()!, height: ys.max()! - ys.min()!)
    }

    /// The local frame: the paragraph box, or the point text's layout bounds.
    private var localFrame: CGRect? {
        guard let s = session else { return nil }
        if let box = s.edit.model.textBox.size { return CGRect(origin: .zero, size: box) }
        let b = index?.layout.bounds ?? CGRect(x: 0, y: 0, width: 1, height: Double(s.edit.model.runs.first?.size ?? 24))
        return b.insetBy(dx: -2, dy: -2)
    }

    /// Paragraph box handles (local): 0…7 clockwise from the top-left corner.
    private func handles(_ box: CGSize) -> [CGPoint] {
        let (w, h) = (box.width, box.height)
        return [CGPoint(x: 0, y: 0), CGPoint(x: w / 2, y: 0), CGPoint(x: w, y: 0), CGPoint(x: w, y: h / 2),
                CGPoint(x: w, y: h), CGPoint(x: w / 2, y: h), CGPoint(x: 0, y: h), CGPoint(x: 0, y: h / 2)]
    }

    private func handleHit(_ p: CGPoint, in v: DocumentViewportView) -> Int? {
        guard let s = session, let box = s.edit.model.textBox.size else { return nil }
        let vp = v.viewPoint(canvas: p)
        return handles(box).firstIndex { h in
            let q = v.viewPoint(canvas: s.transform.apply(h))
            return hypot(q.x - vp.x, q.y - vp.y) <= 7
        }
    }

    /// The topmost visible, caret-editable text layer under canvas point `p`.
    private func textLayerHit(_ doc: DocumentController, at p: CGPoint) -> (DocLayerID, CGPoint)? {
        guard let t = backend(doc) else { return nil }
        for id in doc.outline.flattened {
            guard let n = doc.node(id), n.kind == .text, n.visible else { continue }
            let entry: (revision: UInt64, source: TextLayerSource, layout: TextLayoutInfo)
            if let c = hitCache[id], c.revision == n.revision {
                entry = c
            } else {
                guard let src = try? t.textLayer(id: id) else { continue }
                let layout = (try? t.layoutText(src.model)) ?? .empty
                entry = (n.revision, src, layout)
                hitCache[id] = entry
            }
            guard let inv = entry.source.transform.inverse else { continue }
            let l = inv.apply(p)
            let frame = entry.source.model.textBox.size.map { CGRect(origin: .zero, size: $0) } ?? entry.layout.bounds
            if let f = frame?.insetBy(dx: -4, dy: -4), f.contains(l) { return (id, l) }
        }
        return nil
    }

    // MARK: Mouse (DocumentTools forwards the Type tool's events)

    func mouseDown(_ e: NSEvent, in v: DocumentViewportView) {
        guard let doc = v.controller else { return }
        let p = v.canvasPoint(e)
        let cmd = e.modifierFlags.contains(.command)
        if let s = session, s.docID == doc.id, let l = local(p) {
            if handleHit(p, in: v) != nil || cmd {
                // Box and affine gestures are their own history nodes: record the typing first.
                if s.edit.isChanged || s.previewed { commitDraft("Typing", end: false, selectCreated: true) }
            }
            if let h = handleHit(p, in: v), let box = s.edit.model.textBox.size {
                gesture = .resize(handle: h, box: box, base: s.transform, start: l)
                return
            }
            let inside = localFrame?.insetBy(dx: -4, dy: -4).contains(l) ?? false
            if cmd {
                if inside {
                    gesture = .move(last: p)
                } else {
                    let c = s.transform.apply(CGPoint(x: localFrame?.midX ?? 0, y: localFrame?.midY ?? 0))
                    gesture = .rotate(center: c, startAngle: atan2(p.y - c.y, p.x - c.x), base: s.transform)
                }
                return
            }
            if inside, s.caretEditable, let i = index {
                let o = i.hitTest(l)
                switch e.clickCount {
                case 2: session?.edit.select(i.wordRange(at: o))
                case 3...: session?.edit.select(0..<i.textLength)
                default:
                    if e.modifierFlags.contains(.shift) {
                        let sel = s.edit.selection
                        let anchor = abs(o - sel.lowerBound) > abs(o - sel.upperBound) ? sel.lowerBound : sel.upperBound
                        session?.edit.select(min(anchor, o)..<max(anchor, o))
                        gesture = .select(anchor: anchor)
                    } else {
                        session?.edit.select(o..<o)
                        gesture = .select(anchor: o)
                    }
                }
                caretOn = true
                focus(doc)
                redraw()
                return
            }
            // A click elsewhere applies this text and starts the next.
            apply()
        }
        if let (id, l) = textLayerHit(doc, at: p) {
            if beginExisting(doc, layer: id, local: l), let o = session?.edit.caret { gesture = .select(anchor: o) }
            return
        }
        gesture = .create(start: p, current: p)
        redraw()
    }

    func mouseDragged(_ e: NSEvent, in v: DocumentViewportView) {
        let p = v.canvasPoint(e)
        switch gesture {
        case .create(let start, _):
            gesture = .create(start: start, current: p)
        case .select(let anchor):
            guard let l = local(p), let i = index else { break }
            let o = i.hitTest(l)
            session?.edit.select(min(anchor, o)..<max(anchor, o))
        case .move(let last):
            guard let s = session else { break }
            session?.transform = AffineTransform2D.translation(p.x - last.x, p.y - last.y).concatenating(after: s.transform)
            gesture = .move(last: p)
            schedulePreview()
        case .rotate(let c, let a0, let base):
            var a = atan2(p.y - c.y, p.x - c.x) - a0
            if e.modifierFlags.contains(.shift) { a = (a / (.pi / 12)).rounded() * (.pi / 12) }
            let r = AffineTransform2D.translation(c.x, c.y).concatenating(after: AffineTransform2D.rotation(degrees: a * 180 / .pi))
                .concatenating(after: .translation(-c.x, -c.y))
            session?.transform = r.concatenating(after: base)
            schedulePreview()
        case .resize(let h, let box, let base, let start):
            guard let inv = base.inverse else { break }
            let l = inv.apply(p)
            let dx = l.x - start.x, dy = l.y - start.y
            var x0 = 0.0, y0 = 0.0, x1 = box.width, y1 = box.height
            if [0, 6, 7].contains(h) { x0 += dx }
            if [2, 3, 4].contains(h) { x1 += dx }
            if [0, 1, 2].contains(h) { y0 += dy }
            if [4, 5, 6].contains(h) { y1 += dy }
            let w = max(x1 - x0, 8), hgt = max(y1 - y0, 8)
            if [0, 6, 7].contains(h) { x0 = x1 - w }
            if [0, 1, 2].contains(h) { y0 = y1 - hgt }
            session?.edit.setBox(.paragraph(width: Float(w), height: Float(hgt)))
            session?.transform = base.concatenating(after: .translation(x0, y0))
            relayout()
            schedulePreview()
        case nil: break
        }
        redraw()
    }

    func mouseUp(_ e: NSEvent, in v: DocumentViewportView) {
        guard let doc = v.controller else { gesture = nil; return }
        let g = gesture
        gesture = nil
        switch g {
        case .create(let start, let current):
            let a = v.viewPoint(canvas: start), b = v.viewPoint(canvas: current)
            if hypot(a.x - b.x, a.y - b.y) < 4 {
                beginNew(doc, at: start, box: nil)
            } else {
                beginNew(doc, at: start, box: CGRect(x: min(start.x, current.x), y: min(start.y, current.y),
                                                     width: abs(current.x - start.x), height: abs(current.y - start.y)))
            }
        case .move:
            if session?.layer != nil { commitDraft("Move Type", end: false) }
        case .rotate:
            if session?.layer != nil { commitDraft("Rotate Type", end: false) }
        case .resize:
            if session?.layer != nil { commitDraft("Resize Text Box", end: false) }
        default: break
        }
        redraw()
    }

    // MARK: Character / Paragraph (inspector)

    /// The model the inspector shows: the draft while editing, else the selected text layer's source.
    func inspected(_ doc: DocumentController) -> (model: TextSourceModel, summary: TextStyleSummary, editing: Bool)? {
        if let s = session, s.docID == doc.id, s.layer == nil || s.layer == doc.primary?.id {
            return (s.edit.model, s.edit.styleSummary, true)
        }
        guard let p = doc.primary, p.kind == .text, let t = backend(doc), let src = try? t.textLayer(id: p.id) else { return nil }
        return (src.model, TextStyleSummary(src.model.runs.filter { !$0.text.isEmpty }), false)
    }

    func source(_ doc: DocumentController) -> TextLayerSource? {
        guard let p = doc.primary, p.kind == .text, let t = backend(doc) else { return nil }
        return try? t.textLayer(id: p.id)
    }

    /// A Character edit: the selection (or the caret's next text) while editing, else every run of
    /// the selected text layer. `final`: one history node labelled `label`; otherwise a live draft.
    func character(_ doc: DocumentController, _ label: String, final: Bool = true, _ change: @escaping @Sendable (inout TextRunModel) -> Void) {
        if let s = session, s.docID == doc.id {
            if s.edit.isChanged, !pendingControlEdit { commitDraft("Typing", end: false) }
            pendingControlEdit = !final
            session?.edit.applyStyle(change)
            edited()
            // A caret style with text around waits for typed text: nothing to record yet.
            if final, session?.edit.selection.isEmpty == false || session?.layer == nil || session?.edit.length == 0 {
                commitDraft(label, end: false)
            }
            return
        }
        wholeLayer(doc, label, final: final) { m in
            m.runs = m.runs.map { var r = $0; change(&r); return r }
        }
    }

    /// A Paragraph (or box) edit of the whole model.
    func paragraph(_ doc: DocumentController, _ label: String, final: Bool = true, _ change: @escaping @Sendable (inout TextSourceModel) -> Void) {
        if let s = session, s.docID == doc.id {
            if s.edit.isChanged, !pendingControlEdit { commitDraft("Typing", end: false) }
            pendingControlEdit = !final
            var m = s.edit.model
            change(&m)
            session?.edit.setModel(m)
            edited()
            if final { commitDraft(label, end: false) }
            return
        }
        wholeLayer(doc, label, final: final, change)
    }

    @ObservationIgnored private var pendingControlEdit = false

    /// A control edit of a text layer that is not being edited on canvas: live while dragging,
    /// one node on release.
    private func wholeLayer(_ doc: DocumentController, _ label: String, final: Bool, _ change: @escaping @Sendable (inout TextSourceModel) -> Void) {
        guard let p = doc.primary, p.kind == .text, let t = backend(doc) else { return }
        let backendRef = doc.backend
        enqueue(label) { _ in
            let src = try t.textLayer(id: p.id)
            var m = src.model
            change(&m)
            _ = try t.setTextLayer(id: p.id, model: m, transform: src.transform, interactive: true,
                                   expectedRevision: src.draftPending ? nil : src.revision)
            if final { _ = try backendRef.commit(label: label) }
        } done: { [weak doc] _ in
            doc?.reloadModel()
            if final { doc?.reloadHistory() }
        }
    }

    /// Replaces the source text of a warped / path / vertical layer (the labelled source editor):
    /// the changed middle is replaced, so styles of the unchanged start and end survive.
    func replaceSourceText(_ doc: DocumentController, with new: String) {
        wholeLayer(doc, "Edit Text Source", final: true) { m in
            let old = m.text
            let a = old.commonPrefix(with: new).utf8.count
            let oldTail = String(old.utf8.dropFirst(a)) ?? "", newTail = String(new.utf8.dropFirst(a)) ?? ""
            let suffix = String(String(oldTail.reversed()).commonPrefix(with: String(newTail.reversed())).reversed()).utf8.count
            let insert = String(new.utf8.dropFirst(a).dropLast(suffix)) ?? ""
            m.runs = TextRuns.replace(m.runs, range: a..<(old.utf8.count - suffix), with: insert)
        }
    }

    /// Converts the selected (or edited) text layer to pixels: one node, undo restores the source.
    func convertToPixels(_ doc: DocumentController) {
        if isEditing(doc) { apply() }
        guard let p = doc.primary, p.kind == .text, let t = backend(doc) else { say("Convert to Pixels: select a text layer"); return }
        enqueue("Convert to Pixels") { _ in try t.convertToPixels(id: p.id) } done: { [weak doc] _ in
            doc?.reloadModel()
            doc?.reloadHistory()
        }
    }

    /// Point ⇄ area text for the selected / edited layer.
    func toggleBox(_ doc: DocumentController) {
        let model = inspected(doc)?.model
        let b = isEditing(doc) ? index?.layout.bounds : model.flatMap { try? backend(doc)?.layoutText($0) }?.bounds
        let box = TextBoxModel.paragraph(width: Float(max(b?.maxX ?? 200, 40) + 8), height: Float(max(b?.maxY ?? 60, 20) + 8))
        paragraph(doc, model?.textBox.isParagraph == true ? "Convert to Point Text" : "Convert to Paragraph Text") { m in
            m.textBox = m.textBox.isParagraph ? .point : box
        }
    }

    // MARK: Latency (keystroke → presented frame)

    func frameArrived(_ f: DocFrame, doc: DocumentController) {
        guard !awaitingEpochs.isEmpty else { return }
        let now = Date()
        let done = awaitingEpochs.filter { $0.epoch <= f.epoch }
        awaitingEpochs.removeAll { $0.epoch <= f.epoch }
        for d in done { latencies.append(now.timeIntervalSince(d.at) * 1000) }
        if latencies.count > 400 { latencies.removeFirst(latencies.count - 400) }
        let s = latencies.sorted()
        guard !s.isEmpty else { return }
        let median = s[s.count / 2], p95 = s[min(s.count - 1, Int(Double(s.count) * 0.95))]
        latencyReadout = String(format: "Type: median %.1f ms · p95 %.1f ms (%d keys)", median, p95, s.count)
        if ProcessInfo.processInfo.environment["TESSERA_TEXT_LATENCY_LOG"] != nil {
            FileHandle.standardError.write(Data(String(format: "text-latency: %.2f ms epoch %llu\n",
                                                       done.last.map { now.timeIntervalSince($0.at) * 1000 } ?? 0, f.epoch).utf8))
        }
    }

    // MARK: Drawing (ToolOverlayView)

    private func startBlink() {
        guard blink == nil else { return }
        blink = Timer.scheduledTimer(withTimeInterval: 0.53, repeats: true) { _ in
            MainActor.assumeIsolated {
                let me = DocumentText.shared
                me.caretOn.toggle()
                me.redraw()
            }
        }
    }

    private func quad(_ r: CGRect, _ t: AffineTransform2D, in v: DocumentViewportView) -> NSBezierPath {
        let pts = [CGPoint(x: r.minX, y: r.minY), CGPoint(x: r.maxX, y: r.minY), CGPoint(x: r.maxX, y: r.maxY),
                   CGPoint(x: r.minX, y: r.maxY)].map { v.viewPoint(canvas: t.apply($0)) }
        let p = NSBezierPath()
        p.move(to: pts[0])
        pts.dropFirst().forEach { p.line(to: $0) }
        p.close()
        return p
    }

    private func stroke(_ p: NSBezierPath, dashed: Bool = false) {
        p.lineWidth = 2
        Theme.Palette.OnImage.shadow.setStroke()
        p.stroke()
        p.lineWidth = 1
        if dashed { p.setLineDash([4, 3], count: 2, phase: 0) }
        Theme.Palette.OnImage.guide.setStroke()
        p.stroke()
        p.setLineDash([], count: 0, phase: 0)
    }

    func draw(in v: DocumentViewportView) {
        guard let doc = v.controller else { return }
        if case .create(let a, let b) = gesture {
            let r = CGRect(x: min(a.x, b.x), y: min(a.y, b.y), width: abs(b.x - a.x), height: abs(b.y - a.y))
            if r.width > 0 || r.height > 0 { stroke(quad(r, .identity, in: v), dashed: true) }
        }
        guard let s = session, s.docID == doc.id else { return }
        let t = s.transform
        if let frame = localFrame { stroke(quad(frame, t, in: v), dashed: !s.edit.model.textBox.isParagraph) }
        if let box = s.edit.model.textBox.size {
            for h in handles(box) {
                let q = v.viewPoint(canvas: t.apply(h))
                let k = NSBezierPath(rect: CGRect(x: q.x - 3.5, y: q.y - 3.5, width: 7, height: 7))
                Theme.Palette.OnImage.text.setFill()
                k.fill()
                k.lineWidth = 1
                Theme.Palette.OnImage.ink.setStroke()
                k.stroke()
            }
        }
        guard let i = index, s.caretEditable else { return }
        for r in i.selectionRects(s.edit.selection) {
            Theme.Palette.accent.withAlphaComponent(0.35).setFill()
            quad(r, t, in: v).fill()
        }
        if let m = s.edit.marked {
            for r in i.selectionRects(m) {
                let a = v.viewPoint(canvas: t.apply(CGPoint(x: r.minX, y: r.maxY - 1)))
                let b = v.viewPoint(canvas: t.apply(CGPoint(x: r.maxX, y: r.maxY - 1)))
                let u = NSBezierPath()
                u.move(to: a)
                u.line(to: b)
                u.lineWidth = 2
                Theme.Palette.accent.setStroke()
                u.stroke()
            }
        }
        if s.edit.selection.isEmpty, caretOn {
            let c = i.caret(at: s.edit.caret)
            let p = NSBezierPath()
            p.move(to: v.viewPoint(canvas: t.apply(c.top)))
            p.line(to: v.viewPoint(canvas: t.apply(c.bottom)))
            p.lineWidth = 3
            Theme.Palette.OnImage.shadow.setStroke()
            p.stroke()
            p.lineWidth = 1.5
            Theme.Palette.accent.setStroke()
            p.stroke()
        }
    }
}
