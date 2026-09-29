import AppKit
import Observation
import TesseraCore

/// The Content-Aware Move tool (WP B5-13). It shares the Healing Brush's slot like Remove (its own palette button);
/// while it is on, `DocumentTools` forwards the viewport's mouse and keys here.
///
/// Make a selection, then drag inside it: the ghost of the selection follows the pointer by whole document pixels
/// (whatever the zoom). On release the engine computes the move at full resolution from a snapshot frozen when the
/// first drag began (`begin_content_aware_move`) and shows it in the viewport; dragging again adjusts the same move,
/// and the options (Move / Extend, Structure, Color, Seed) re-run it. Return applies it as one history step, Esc
/// cancels (layer, selection and history unchanged). Previews are throttled (one in flight, the newest wins); while
/// one computes, Cancel stops the move at once and discards its late result.
@MainActor @Observable
final class DocumentContentAware {
    static let shared = DocumentContentAware()

    var document: DocumentController? { DocumentTools.shared.document }

    private var on = false
    /// The tool is on (Remove turning on ends it).
    var active: Bool { on && !DocumentRetouch.shared.removeActive }
    var options = ContentAwareOptions() { didSet { optionsChanged(oldValue) } }
    /// The frozen move, once the first drag began.
    private(set) var session: ContentAwareMoveSession?
    /// The offset shown (document pixels).
    private(set) var offset: (dx: Int32, dy: Int32) = (0, 0)
    /// The last finished preview.
    private(set) var preview: ContentAwarePreviewInfo?
    private(set) var computing = false
    private(set) var error: String?
    /// Apply in progress (`RetouchJobs`: Cancel never waits).
    let jobs = RetouchJobs()
    var busy: String? { jobs.running?.title ?? (computing ? "Computing the move…" : nil) }

    @ObservationIgnored private var dragStart: CGPoint?
    @ObservationIgnored private var dragBase: (dx: Int32, dy: Int32) = (0, 0)
    @ObservationIgnored private var throttle = PreviewThrottle()
    @ObservationIgnored private var generation = 0
    @ObservationIgnored private var beginning = false
    @ObservationIgnored private let queue = DispatchQueue(label: "dev.tessera.content-aware-move", qos: .userInitiated)
    /// Called after each finished preview / apply (self-test).
    @ObservationIgnored var onPreview: ((Result<ContentAwarePreviewInfo, Error>) -> Void)?
    @ObservationIgnored var onApplied: ((Result<DocumentChange, Error>) -> Void)?

    private init() {}

    static func backend(_ doc: DocumentController?) -> (any DocumentContentAwareBackend)? {
        doc?.backend as? any DocumentContentAwareBackend
    }

    private func say(_ s: String) { document?.report?(s) }

    // MARK: Tool

    /// Palette slot: the Content-Aware Move tool on.
    func activate() {
        guard let doc = document else { return }
        DocumentRetouch.shared.deactivate()
        if doc.tool != .heal { DocumentTools.shared.select(.heal) }
        on = true
        error = nil
        redraw()
        say(ContentAwareMenuState.canStart(layerKind: doc.primary?.kind, hasSelection: doc.marquee != nil)
            ?? "Content-Aware Move: drag the selection to where it should go; Return applies, Esc cancels")
    }

    func deactivate() {
        guard on else { return }
        if session != nil { cancel() }
        on = false
        redraw()
    }

    /// `DocumentTools.select` hook: any tool choice ends this one.
    func toolSelected(_ tool: DocumentTool) {
        if on { deactivate() }
    }

    // MARK: Mouse

    func mouseDown(_ e: NSEvent, in v: DocumentViewportView) -> Bool {
        guard active, let doc = document, doc === v.controller else { return false }
        let p = v.canvasPoint(e)
        if jobs.isBusy { say("Content-Aware Move: applying… (Esc cancels)"); return true }
        if let why = ContentAwareMenuState.canStart(layerKind: doc.primary?.kind, hasSelection: doc.marquee != nil || session != nil) {
            if session == nil { say("Content-Aware Move: \(why)"); return true }
        }
        let bounds = session?.selectionBounds ?? doc.marquee
        guard let b = bounds else { return true }
        // Start inside the selection (moved or not), like Photoshop.
        let moved = ContentAwareDrag.moved(b, dx: offset.dx, dy: offset.dy)
        let inside = Self.contains(b, p) || Self.contains(moved, p)
        guard inside else { say("Content-Aware Move: drag from inside the selection"); return true }
        dragStart = p
        dragBase = offset
        if session == nil { begin(doc) }
        redraw()
        return true
    }

    func mouseDragged(_ e: NSEvent, in v: DocumentViewportView) -> Bool {
        guard active, let start = dragStart, let doc = document else { return false }
        guard let b = session?.selectionBounds ?? doc.marquee else { return true }
        let p = v.canvasPoint(e)
        let d = ContentAwareDrag.offset(from: start, to: p, bounds: ContentAwareDrag.moved(b, dx: dragBase.dx, dy: dragBase.dy),
                                        canvasWidth: doc.info.width, canvasHeight: doc.info.height)
        offset = (dragBase.dx + d.dx, dragBase.dy + d.dy)
        redraw()
        return true
    }

    func mouseUp(_ e: NSEvent, in v: DocumentViewportView) -> Bool {
        guard active, dragStart != nil else { return false }
        _ = mouseDragged(e, in: v)
        dragStart = nil
        requestPreview()
        return true
    }

    private static func contains(_ r: CanvasRect, _ p: CGPoint) -> Bool {
        let (x, y) = (Double(p.x), Double(p.y))
        let (x0, y0) = (Double(r.x), Double(r.y))
        return x >= x0 && x < x0 + Double(r.width) && y >= y0 && y < y0 + Double(r.height)
    }

    // MARK: Session

    private func begin(_ doc: DocumentController) {
        guard let b = Self.backend(doc), let l = doc.primary, !beginning else { return }
        beginning = true
        error = nil
        let (layer, mode) = (l.id, options.mode)
        do {
            // Fast (reads the selection and snapshots the layer): the next preview needs it.
            session = try b.beginContentAwareMove(layer: layer, mode: mode)
            offset = (0, 0)
            preview = nil
        } catch {
            self.error = error.localizedDescription
            say("Content-Aware Move: \(error.localizedDescription)")
            dragStart = nil
        }
        beginning = false
    }

    /// Latest-wins: at most one preview computes; a newer request runs once it returns.
    func requestPreview() {
        guard session != nil else { return }
        guard throttle.request() else { return }
        runPreview()
    }

    private func runPreview() {
        guard let s = session, let b = Self.backend(document) else { throttle.reset(); return }
        computing = true
        error = nil
        generation += 1
        let g = generation
        let (dx, dy, fill, seam) = (offset.dx, offset.dy, options.fillJson, options.seam)
        queue.async {
            let r = Result { try b.previewContentAwareMove(token: s.token, dx: dx, dy: dy, fillJson: fill, seam: seam) }
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    let m = DocumentContentAware.shared
                    guard m.session?.token == s.token, g == m.generation else { return }
                    switch r {
                    case .success(let p):
                        m.preview = p
                        m.say(String(format: "%@ by %d, %d px: %.1f s. Return applies, Esc cancels", s.mode.historyLabel, p.dx, p.dy,
                                     p.millis / 1000))
                    case .failure(let e):
                        if !e.localizedDescription.contains("cancelled") { m.error = e.localizedDescription }
                    }
                    m.onPreview?(r)
                    if m.throttle.finished() { m.runPreview() } else { m.computing = false }
                    m.redraw()
                }
            }
        }
    }

    private func optionsChanged(_ old: ContentAwareOptions) {
        guard let s = session, let doc = document else { return }
        if old.mode != options.mode {
            // The mode is frozen with the move: start again from the same snapshot point and offset.
            let keep = offset
            Self.backend(doc)?.cancelContentAwareMove(token: s.token)
            session = nil
            begin(doc)
            offset = keep
        }
        if old != options, offset != (0, 0) { requestPreview() }
    }

    // MARK: Apply / Cancel

    func apply() {
        guard let s = session, let doc = document, let b = Self.backend(doc) else { return }
        guard preview != nil, !computing else { say("Content-Aware Move: wait for the preview, or Esc to cancel"); return }
        let q = queue
        let refused = jobs.start("Applying \(s.mode.historyLabel)…", operation: s.mode.historyLabel, {
            try q.sync { try b.commitContentAwareMove(token: s.token) }
        }) { [weak self] end in
            self?.applyEnded(end, label: s.mode.historyLabel, doc: doc)
        }
        if let refused { error = refused }
    }

    /// How an Apply ended. A discarded (cancelled) apply keeps nothing: the engine refuses to write once it sees
    /// the cancel, and when the cancel arrived after its last check the committed step is undone (as B5-09 Remove
    /// does), so engine history and the panels agree.
    func applyEnded(_ end: RetouchJobs.End<DocumentChange>, label: String, doc: DocumentController) {
        switch end {
        case .finished(.success(let c)):
            doc.run(label) { c }
            reset()
            say("\(label) applied")
            onApplied?(.success(c))
        case .finished(.failure(let e)):
            error = e.localizedDescription
            onApplied?(.failure(e))
        case .discarded(let r):
            if case .success = r { _ = doc.run("Undo cancelled \(label)") { try doc.backend.undo() } }
            onApplied?(.failure(DocumentError.invalid("discarded")))
        }
        redraw()
    }

    /// Esc / Cancel: stops a computing preview or apply at once, ends the preview; nothing changes.
    func cancel() {
        guard let s = session else { return }
        let b = Self.backend(document)
        if jobs.isBusy {
            jobs.cancel { b?.cancelContentAwareMove(token: s.token) }
        } else {
            // Not on `queue`: a computing preview must not delay the cancel.
            DispatchQueue.global(qos: .userInitiated).async { b?.cancelContentAwareMove(token: s.token) }
        }
        reset()
        say("Content-Aware Move cancelled; the layer and selection are unchanged")
        redraw()
    }

    private func reset() {
        session = nil
        preview = nil
        computing = false
        offset = (0, 0)
        dragStart = nil
        throttle.reset()
        generation += 1
    }

    // MARK: Keys

    func handleKey(_ event: NSEvent) -> Bool {
        guard active else { return false }
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        guard flags.subtracting(.shift).isEmpty else { return false }
        switch event.keyCode {
        case 53:
            if session != nil { cancel(); return true }
            return false
        case 36, 76:
            if session != nil { apply(); return true }
            return false
        default:
            return false
        }
    }

    /// Waits until the running preview has returned (self-test).
    func idle() async {
        while computing || jobs.isBusy { try? await Task.sleep(for: .milliseconds(20)) }
    }

    // MARK: Overlay

    func redraw() {
        guard let v = document?.viewport else { return }
        overlay(v).needsDisplay = true
    }

    private func overlay(_ v: DocumentViewportView) -> ContentAwareOverlayView {
        if let o = v.subviews.compactMap({ $0 as? ContentAwareOverlayView }).first { return o }
        let o = ContentAwareOverlayView()
        o.frame = v.bounds
        o.autoresizingMask = [.width, .height]
        o.viewport = v
        v.addSubview(o, positioned: .above, relativeTo: v.toolOverlay)
        return o
    }

    var dragging: Bool { dragStart != nil }
}
