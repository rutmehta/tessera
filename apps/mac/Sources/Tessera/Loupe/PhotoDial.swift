import AppKit
import Observation
import TesseraCore
import TesseraFFI

/// What a click on the photo targets (SPEC §3 "Pointing at the photo", WP UX-13): one of the three
/// AI masks or the whole photo.
enum DialTarget: String, CaseIterable, Identifiable {
    case subject, sky, background, whole
    var id: String { rawValue }

    var title: String {
        switch self {
        case .subject: "Subject"
        case .sky: "Sky"
        case .background: "Background"
        case .whole: "Whole photo"
        }
    }

    /// The dial's last line.
    var note: String { self == .whole ? "Applies to the whole photo." : "Applies to the \(title.lowercased()) only." }

    var request: AiMaskRequest? {
        switch self {
        case .subject: .subject
        case .sky: .sky
        case .background: .background
        case .whole: nil
        }
    }

    var componentType: MaskComponentType? {
        switch self {
        case .subject: .subject
        case .sky: .sky
        case .background: .background
        case .whole: nil
        }
    }

    /// The most relevant slider: Shadows on a subject, Highlights on sky, Exposure elsewhere.
    var defaultControl: DialControl {
        switch self {
        case .subject: .shadows
        case .sky: .highlights
        case .background, .whole: .exposure
        }
    }
}

/// The dial's controls: a local parameter on a mask, the matching panel slider on the whole photo.
enum DialControl: String, CaseIterable, Identifiable {
    case exposure, shadows, warmth, highlights, skin
    var id: String { rawValue }

    var title: String {
        switch self {
        case .exposure: "Exposure"
        case .shadows: "Shadows"
        case .warmth: "Warmth"
        case .highlights: "Highlights"
        case .skin: "Skin"
        }
    }

    /// The engine's local parameter name (`LocalParam.all`). Skin softens texture on the person.
    var localName: String {
        switch self {
        case .exposure: "exposure"
        case .shadows: "shadows"
        case .warmth: "temperature"
        case .highlights: "highlights"
        case .skin: "texture"
        }
    }

    var local: LocalParam { LocalParam.all.first { $0.name == localName }! }

    /// The panel slider the control drives on the whole photo (the panel highlights it).
    var global: DevelopControl {
        switch self {
        case .exposure: DevelopControl("Exposure", ["tone", "exposure"], -5...5, step: 0.01, format: "%+.2f")
        case .shadows: DevelopControl("Shadows", ["tone", "shadows"], -100...100)
        case .warmth: DevelopControl("Temperature", ["white_balance", "temperature"], 2000...25000, default: 5500, step: 50, format: "%.0f K")
        case .highlights: DevelopControl("Highlights", ["tone", "highlights"], -100...100)
        case .skin: DevelopControl("Texture", ["tone", "texture"], -100...100)
        }
    }

    /// The same slider as the Edit panel names it (UX-12 highlights `.basic` rows).
    var panelControl: DevelopPanelControl {
        switch self {
        case .exposure: .basic(.exposure)
        case .shadows: .basic(.shadows)
        case .warmth: .basic(.temperature)
        case .highlights: .basic(.highlights)
        case .skin: .basic(.texture)
        }
    }

    /// Value change per point of knob drag.
    func dragScale(whole: Bool) -> Double {
        switch self {
        case .exposure: 0.02
        case .warmth: whole ? 25 : 1
        default: 1
        }
    }
}

/// The dial: a click on the photo in Edit picks the mask under the cursor, draws its ring and opens
/// a HUD beside the cursor with one knob for the most relevant slider (SPEC §3). The value is a
/// local adjustment on that AI mask (a global slider on the whole photo), recorded through the
/// normal develop history. Also owns the Masks chips and click-to-heal of the tools strip.
///
/// The three AI masks are created on first use, as one history step, and found again by their
/// single Subject / Sky / Background component; a click while they segment re-targets when done.
@MainActor @Observable
final class PhotoDial: LibraryObserver {
    static let shared = PhotoDial(model: .shared)

    @ObservationIgnored let model: AppModel

    // MARK: Observed state

    private(set) var isOpen = false
    /// Where the HUD sits: the click, in loupe view points (y-down).
    private(set) var anchor: CGPoint = .zero
    private(set) var target: DialTarget = .whole
    private(set) var control: DialControl = .exposure
    private(set) var value: Double = 0
    /// A detected face is under the cursor: the Skin chip appears.
    private(set) var faceUnderCursor = false
    /// The AI masks are still segmenting: the click re-targets when they finish.
    private(set) var segmenting = false
    /// The Masks tool of the strip: the Subject / Sky / Background / Whole photo chips.
    private(set) var masksToolShown = false
    /// The Heal tool of the strip: the next click heals a spot.
    private(set) var healActive = false
    /// Bumped when the ring or a value changes outside a drag.
    private(set) var revision = 0

    // MARK: Hot state

    /// Loupe redraw (ring, heal spots, cursor).
    @ObservationIgnored var onLoupeChange: (() -> Void)?
    /// The ring: closed outlines of the target mask in mask space (the overlay maps them).
    @ObservationIgnored private(set) var ring: [[(x: Double, y: Double)]] = []
    @ObservationIgnored private var rasters: [UInt32: MaskRaster] = [:]
    /// A click waiting for the segmenter (mask space).
    @ObservationIgnored private var pendingClick: (x: Double, y: Double)?
    @ObservationIgnored private var dragStart: Double?

    init(model: AppModel) {
        self.model = model
        model.addObserver(self)
        MaskTools.shared.onAIMaskDone = { [weak self] in self?.aiMaskDidFinish() }
    }

    private var masks: MaskTools { .shared }
    private var tools: DevelopTools { .shared }
    var develop: DevelopController? { model.develop }

    /// A click on the photo opens the dial: Edit, the session ready, no other tool armed.
    var armed: Bool {
        model.isPhotoEditing && model.developStatus == .ready && develop != nil && model.viewMode == .loupe
            && !tools.cropActive && tools.hslPicker == nil && !tools.detailPicking && masks.tool == nil
            && !UprightGuideTool.shared.active && !model.showingBefore
    }

    // MARK: AI masks

    /// The group that is exactly this AI mask, if the photo has one.
    func groupID(_ t: DialTarget) -> UInt32? {
        guard let kind = t.componentType else { return nil }
        return masks.list.groups.first { g in
            g.components.count == 1 && g.components[0].kind == kind && !g.invert && !g.components[0].invert
        }?.id
    }

    private func isReady(_ t: DialTarget) -> Bool {
        guard let id = groupID(t), let g = masks.list.groups.first(where: { $0.id == id }) else { return false }
        if case .ready = g.components[0].ai { return true }
        return false
    }

    /// All three AI masks exist and have segmented.
    var masksReady: Bool { [DialTarget.subject, .sky, .background].allSatisfy(isReady) }

    /// Creates the AI masks the photo lacks as one step. Returns false while any still segments.
    @discardableResult
    func ensureAIMasks() -> Bool {
        guard let d = develop, model.developStatus == .ready else { return false }
        let missing = [DialTarget.subject, .sky, .background].filter { groupID($0) == nil }
        if !missing.isEmpty {
            for t in missing {
                guard let request = t.request else { continue }
                _ = d.addAIMask(group: nil, request, combine: .add)
            }
            masks.commit("Subject, Sky and Background masks")
        }
        rasters = [:]
        segmenting = !masksReady
        return !segmenting
    }

    private func raster(_ t: DialTarget) -> MaskRaster? {
        guard isReady(t), let id = groupID(t), let d = develop else { return nil }
        if let r = rasters[id] { return r }
        guard let thumb = d.maskThumbnail(id, maxPixels: 160),
              let r = MaskRaster(width: Int(thumb.width), height: Int(thumb.height), alpha: [UInt8](thumb.alpha)) else { return nil }
        rasters[id] = r
        return r
    }

    /// Subject if the point is inside the subject mask, Sky if inside sky, else Background; the
    /// whole photo when the segmenter found nothing (or the masks are not there yet).
    func target(at p: (x: Double, y: Double)) -> DialTarget {
        if let s = raster(.subject), s.contains(x: p.x, y: p.y) { return .subject }
        if let s = raster(.sky), s.contains(x: p.x, y: p.y) { return .sky }
        if let b = raster(.background), b.coverage() > 0.01 { return .background }
        return .whole
    }

    private func aiMaskDidFinish() {
        rasters = [:]
        guard masksReady else { return }
        segmenting = false
        if let p = pendingClick {
            pendingClick = nil
            retarget(target(at: p))
        } else if isOpen || masksToolShown {
            retarget(target)
        }
        TryLooks.shared.masksDidBecomeReady()
    }

    // MARK: Opening and targeting

    /// A click on the photo at `point` (view) over mask-space `p`.
    func click(at point: CGPoint, mask p: (x: Double, y: Double), face: Bool) {
        guard armed else { return }
        anchor = point
        faceUnderCursor = face
        isOpen = true
        if ensureAIMasks() {
            pendingClick = nil
            retarget(target(at: p))
        } else {
            pendingClick = p
            retarget(.whole)
        }
    }

    /// Targets `t`: its ring, its default control and the panel's selection follow.
    private func retarget(_ t: DialTarget) {
        target = t
        control = t.defaultControl
        if control == .skin, !faceUnderCursor { control = .shadows }
        ring = t == .whole ? [] : (raster(t)?.outlines() ?? [])
        if let id = groupID(t), masks.list.selectedID != id { masks.select(id) }
        value = currentValue()
        model.highlightedDevelopControl = isOpen ? control.panelControl : nil
        revision += 1
        onLoupeChange?()
    }

    /// The Masks chips: target this mask for the dial and the panel.
    func select(_ t: DialTarget) {
        if t != .whole, !ensureAIMasks() {
            model.statusMessage = "Finding the \(t.title.lowercased())…"
        }
        retarget(t)
    }

    func choose(_ c: DialControl) {
        control = c
        value = currentValue()
        model.highlightedDevelopControl = c.panelControl
        revision += 1
    }

    func close() {
        guard isOpen else { return }
        isOpen = false
        pendingClick = nil
        dragStart = nil
        if !masksToolShown { ring = [] }
        model.highlightedDevelopControl = nil
        revision += 1
        onLoupeChange?()
    }

    // MARK: Values

    private func currentValue() -> Double {
        if target == .whole { return tools.value(control.global) }
        guard let id = groupID(target), let g = masks.list.groups.first(where: { $0.id == id }) else { return 0 }
        return Double(g.params.first { $0.name == control.localName }?.value ?? 0)
    }

    /// Range of the knob for the current target.
    var range: ClosedRange<Double> { target == .whole ? control.global.range : control.local.range }

    /// The knob's readout.
    var formatted: String {
        target == .whole ? String(format: control.global.format, value) : String(format: control.local.format, value)
    }

    /// Knob drag: `delta` points up from the press (coalesced per frame; one undo step on release).
    func drag(delta: Double, final: Bool) {
        if dragStart == nil { dragStart = value }
        let start = dragStart ?? value
        let v = min(max(start + delta * control.dragScale(whole: target == .whole), range.lowerBound), range.upperBound)
        let stepped = target == .whole ? (v / control.global.step).rounded() * control.global.step
                                       : (v / control.local.step).rounded() * control.local.step
        value = stepped
        apply(stepped, final: final)
        if final { dragStart = nil; revision += 1 }
    }

    private func apply(_ v: Double, final: Bool) {
        if target == .whole {
            tools.set(control.global, v, final: final)
            return
        }
        guard let id = groupID(target), let d = develop else { return }
        d.setMaskParam(id, control.localName, v, interactive: !final)
        NotificationCenter.default.post(name: DevelopTools.valuesChanged, object: nil)
        if final { masks.commit("\(target.title): \(control.local.historyLabel(v))") }
    }

    // MARK: Masks tool (strip)

    func toggleMasksTool() {
        masksToolShown.toggle()
        if masksToolShown {
            if healActive { healActive = false }
            if target == .whole { select(.subject) } else { select(target) }
        } else if !isOpen {
            ring = []
            onLoupeChange?()
        }
        revision += 1
    }

    // MARK: Heal tool (strip)

    func toggleHeal() {
        healActive.toggle()
        if healActive {
            if masksToolShown { masksToolShown = false }
            close()
            if tools.cropActive { tools.commitCrop() }
            model.statusMessage = "Click a spot on the photo to heal it · Esc leaves healing"
        }
        revision += 1
        onLoupeChange?()
    }

    /// The heal spots of the live settings (drawn while the tool is active).
    var healSpots: [RetouchSpots.Spot] { develop.map { RetouchSpots.spots(in: $0.settingsObject) } ?? [] }

    /// Heals a round spot at mask-space `p`, as one step.
    func heal(at p: (x: Double, y: Double)) {
        guard healActive, let d = develop, model.developStatus == .ready else { return }
        let settings = d.freshSettings()
        let op = RetouchSpots.healOperation(id: RetouchSpots.nextID(in: settings), center: p)
        d.apply(patch: RetouchSpots.appending(op, to: settings), interactive: false)
        model.commitDevelop(label: "Heal spot")
        tools.refreshHistory()
        revision += 1
        onLoupeChange?()
    }

    // MARK: Keys

    /// Esc closes the dial, then leaves healing, then hides the mask chips. Returns true when handled.
    func handleKey(_ event: NSEvent) -> Bool {
        guard model.isPhotoEditing, event.keyCode == 53 else { return false }
        if isOpen { close(); return true }
        if healActive { toggleHeal(); return true }
        if masksToolShown { toggleMasksTool(); return true }
        return false
    }

    // MARK: LibraryObserver

    private func reset() {
        isOpen = false
        pendingClick = nil
        dragStart = nil
        segmenting = false
        masksToolShown = false
        healActive = false
        ring = []
        rasters = [:]
        target = .whole
        model.highlightedDevelopControl = nil
        revision += 1
        onLoupeChange?()
    }

    func workspaceWillLeavePhotoEdit() { reset() }
    func libraryDidReload() { reset() }
    func itemsDidChange(_ positions: IndexSet) {}
    func selectionDidChange(scrollToFocus: Bool) { if isOpen || healActive || masksToolShown { reset() } }
    func developDidChange() { reset() }
}
