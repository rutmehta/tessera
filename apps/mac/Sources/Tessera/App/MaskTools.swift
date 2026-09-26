import AppKit
import Observation
import TesseraCore
import TesseraFFI

/// Masking mode (M2-14): the Masks panel, the loupe mask toolbar and the loupe mask tools. Every
/// change goes through the open `DevelopController` (coalesced per display frame while dragging) and
/// is committed as one undo step at the end of a gesture.
///
/// Keys (loupe): M masks on/off · O overlay · ⇧O overlay colour · [ ] brush size (⇧: feather) ·
/// X invert the selected mask · ⌫ delete it · Esc disarms the tool, then leaves masking.
/// ⌥ while painting erases; ⌥ with the other tools subtracts from the selected mask.
@MainActor @Observable
final class MaskTools: LibraryObserver {
    static let shared = MaskTools(model: .shared)

    @ObservationIgnored let model: AppModel

    // MARK: Observed state

    /// Masking mode: toolbar shown, overlay and tools available.
    private(set) var active = false
    /// Armed loupe tool.
    var tool: MaskTool? { didSet { if tool != oldValue { onLoupeChange?() } } }
    /// The next tool use adds to this group with this mode (from the panel's Add/Subtract/Intersect).
    var target: (group: UInt32, combine: MaskCombineMode)?
    private(set) var list = MaskListState()
    var overlayOn = true { didSet { updateOverlay() } }
    var overlayColor: MaskOverlayColor = .red { didSet { onLoupeChange?() } }
    var overlayOpacity = 0.5 { didSet { onLoupeChange?() } }
    /// Brush radius in view points; feather and flow 0…100.
    var brushSize = 40.0 { didSet { onLoupeChange?() } }
    var brushFeather = 50.0
    var brushFlow = 100.0
    /// Bumped when the list or values change outside a drag (SwiftUI refresh).
    private(set) var revision = 0
    private(set) var thumbnails: [UInt32: CGImage] = [:]

    // MARK: Hot state

    /// Loupe redraw (tool change, overlay colour, brush cursor).
    @ObservationIgnored var onLoupeChange: (() -> Void)?
    /// A new overlay plane for the loupe.
    @ObservationIgnored var onOverlayFrame: ((MaskOverlayFrame?) -> Void)?
    @ObservationIgnored private var valuesToken: NSObjectProtocol?
    @ObservationIgnored private var thumbnailsDirty = true

    init(model: AppModel) {
        self.model = model
        model.addObserver(self)
        valuesToken = NotificationCenter.default.addObserver(forName: DevelopTools.valuesChanged, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.valuesChangedElsewhere() }
        }
        if !model.library.items.isEmpty { libraryDidReload() }
    }

    var develop: DevelopController? { model.develop }
    var ready: Bool { model.developStatus == .ready && develop != nil && model.viewMode == .loupe }
    var selected: MaskGroupInfo? { list.selected }

    // MARK: Mode

    func toggle() { setActive(!active) }

    func setActive(_ on: Bool) {
        guard on != active else { return }
        if on {
            guard ready else {
                model.statusMessage = "Masking works on a RAW in the loupe (E)"
                return
            }
            if DevelopTools.shared.cropActive { DevelopTools.shared.commitCrop() }
            UserDefaults.standard.set(true, forKey: "InspectorPanel.Masks")
            model.showInspector = true
        } else {
            tool = nil
            target = nil
        }
        active = on
        refresh()
        updateOverlay()
        onLoupeChange?()
    }

    // MARK: List

    /// Reloads the groups from the session.
    func refresh() {
        guard let d = develop else {
            list = MaskListState()
            thumbnails = [:]
            revision += 1
            return
        }
        let before = list.selectedID
        list.update(d.maskGroups())
        revision += 1
        thumbnailsDirty = true
        if list.selectedID != before { updateOverlay() }
    }

    func select(_ id: UInt32?) {
        list.select(id)
        revision += 1
        updateOverlay()
        onLoupeChange?()
    }

    private func valuesChangedElsewhere() {
        // Undo, redo, presets, history: the groups may have changed under us.
        guard develop != nil else { return }
        refresh()
        onLoupeChange?()
    }

    /// Commits the live change as one undo step and refreshes the panel.
    func commit(_ label: String) {
        model.commitDevelop(label: label)
        DevelopTools.shared.refreshHistory()
        refresh()
    }

    // MARK: Group edits

    func setEnabled(_ id: UInt32, _ on: Bool) {
        develop?.updateMaskGroup(id, MaskGroupPatch(name: nil, enabled: on, amount: nil, invert: nil), interactive: false)
        commit(on ? "Show Mask" : "Hide Mask")
    }

    func setAmount(_ v: Double, final: Bool) {
        guard let id = list.selectedID else { return }
        list.setAmount(v)
        develop?.updateMaskGroup(id, MaskGroupPatch(name: nil, enabled: nil, amount: Float(v), invert: nil), interactive: !final)
        if final { commit(String(format: "Mask Amount %.0f%%", v)) }
    }

    func invertSelected() {
        guard let g = selected else { return }
        develop?.updateMaskGroup(g.id, MaskGroupPatch(name: nil, enabled: nil, amount: nil, invert: !g.invert), interactive: false)
        commit(g.invert ? "Uninvert Mask" : "Invert Mask")
    }

    func rename(_ id: UInt32, to name: String) {
        let name = name.trimmingCharacters(in: .whitespaces)
        guard !name.isEmpty else { return }
        develop?.updateMaskGroup(id, MaskGroupPatch(name: name, enabled: nil, amount: nil, invert: nil), interactive: false)
        commit("Rename Mask")
    }

    func duplicate(_ id: UInt32) {
        if develop?.duplicateMask(id) != nil { commit("Duplicate Mask") }
    }

    func delete(_ id: UInt32) {
        develop?.deleteMask(id)
        commit("Delete Mask")
    }

    func setParam(_ p: LocalParam, _ v: Double, final: Bool) {
        guard let id = list.selectedID else { return }
        list.setParam(p.name, v)
        develop?.setMaskParam(id, p.name, v, interactive: !final)
        if final { commit("\(selected?.name ?? "Mask"): \(p.historyLabel(v))") }
    }

    func resetParams() {
        guard let id = list.selectedID else { return }
        develop?.resetMaskParams(id)
        commit("Reset Mask Adjustments")
    }

    func setComponentMode(_ index: Int, combine: MaskCombineMode, invert: Bool) {
        guard let id = list.selectedID else { return }
        develop?.setMaskComponentMode(id, index, combine: combine, invert: invert)
        commit("Mask Component \(combine.title)\(invert ? " (Inverted)" : "")")
    }

    func removeComponent(_ index: Int) {
        guard let id = list.selectedID else { return }
        develop?.removeMaskComponent(id, index)
        commit("Remove Mask Component")
    }

    func retry(_ key: String) { develop?.retryAIMask(key: key) }

    /// Arms `tool` to add to the selected mask with `combine` (the panel's Add/Subtract/Intersect).
    func arm(_ tool: MaskTool, combine: MaskCombineMode) {
        if !active { setActive(true) }
        target = list.selectedID.map { ($0, combine) }
        self.tool = tool
    }

    /// Where the next tool use goes: an explicit target, ⌥ (subtract from the selection) or a new group.
    func destination(option: Bool) -> (group: UInt32?, combine: MaskCombineMode) {
        if let t = target { return (t.group, t.combine) }
        if option, let id = list.selectedID { return (id, .subtract) }
        return (nil, .add)
    }

    private func used(_ group: UInt32?) {
        target = nil
        if let group { list.select(group) }
    }

    // MARK: AI

    func runAI(_ request: AiMaskRequest, title: String, option: Bool = false) {
        guard ready, let d = develop else { return }
        if !active { setActive(true) }
        let dest = destination(option: option)
        guard let id = d.addAIMask(group: dest.group, request, combine: dest.combine) else { return }
        commit(dest.group == nil ? "New \(title) Mask" : "\(dest.combine.title) \(title)")
        used(id)
        refresh()
        updateOverlay()
        model.statusMessage = "\(title): segmenting…"
    }

    func aiProgress(_ u: MaskJobUpdate) {
        list.progressUpdate(u)
        revision += 1
        if u.done {
            refresh()
            // The final render can precede the job callback. In that ordering
            // the dirty thumbnail otherwise waits for an unrelated edit.
            refreshThumbnails()
            if let e = u.error {
                model.statusMessage = "\(u.title) mask failed: \(e)"
            } else {
                model.statusMessage = "\(u.title) mask ready"
            }
        }
    }

    // MARK: Brush

    private var strokeGroup: UInt32?

    /// Begins a stroke at mask-space `p`. ⌥ erases from the selected mask's brush.
    func beginStroke(at p: (x: Double, y: Double), pressure: Double, erase: Bool, radius: Double) {
        guard let d = develop else { return }
        var group: UInt32? = target?.group
        if group == nil, list.selectedBrushIndex != nil { group = list.selectedID }
        if erase {
            guard let g = list.selectedID, list.selectedBrushIndex != nil else {
                model.statusMessage = "Select a brushed mask to erase from"
                return
            }
            group = g
        }
        guard let id = d.beginBrushStroke(group: group, radius: radius, feather: brushFeather, flow: brushFlow, erase: erase)
        else { return }
        strokeGroup = id
        if group == nil { refresh() }
        list.select(id)
        updateOverlay()
        d.addBrushSample(x: p.x, y: p.y, pressure: pressure)
    }

    func continueStroke(at p: (x: Double, y: Double), pressure: Double) {
        guard strokeGroup != nil else { return }
        develop?.addBrushSample(x: p.x, y: p.y, pressure: pressure)
    }

    func endStroke(erase: Bool) {
        guard strokeGroup != nil else { return }
        strokeGroup = nil
        develop?.endBrushStroke()
        target = nil
        commit(erase ? "Brush Erase" : "Brush Stroke")
    }

    // MARK: Gradients

    /// A gradient drag in progress: the group and component being shaped.
    private var gradient: (group: UInt32, index: Int, isNew: Bool, title: String)?

    /// Creates a gradient component at mouse-down (it is shaped by `shapeGradient` until mouse-up).
    func beginGradient(_ json: String, title: String, option: Bool) -> Bool {
        guard let d = develop else { return false }
        let dest = destination(option: option)
        if let g = dest.group {
            guard let index = d.addMaskComponent(g, json, combine: dest.combine, interactive: true) else { return false }
            gradient = (g, Int(index), false, "\(dest.combine.title) \(title)")
        } else {
            guard let id = d.addMask(json, interactive: true) else { return false }
            gradient = (id, 0, true, "New \(title) Mask")
        }
        used(gradient?.group)
        refresh()
        updateOverlay()
        return true
    }

    /// Edits an existing gradient component (handle drag).
    func editGradient(group: UInt32, index: Int, title: String) {
        gradient = (group, index, false, "Edit \(title)")
    }

    func shapeGradient(_ json: String, final: Bool) {
        guard let g = gradient else { return }
        develop?.setMaskComponent(g.group, g.index, json: json, interactive: !final)
        if final {
            gradient = nil
            commit(g.title)
        }
    }

    // MARK: Range and prompts

    /// Samples a colour or luminance range at a mask-space point (off the main actor).
    func pickRange(_ kind: RangeKind, at p: (x: Double, y: Double), option: Bool, addSample: Bool) {
        guard let d = develop else { return }
        let session = d.session
        if addSample, let g = selected,
           let index = g.components.lastIndex(where: { $0.kind == .colorRange }) {
            let id = g.id
            Task.detached(priority: .userInitiated) {
                let result = Result { try session.addColorRangeSample(groupId: id, index: UInt32(index), x: Float(p.x), y: Float(p.y)) }
                await MainActor.run { self.rangeDone(result.map { id }, label: "Add Color Sample") }
            }
            return
        }
        d.flushPending()
        let dest = destination(option: option)
        let title = kind == .color ? "Color Range" : "Luminance Range"
        Task.detached(priority: .userInitiated) {
            let result = Result { try session.addRangeMask(groupId: dest.group, kind: kind, x: Float(p.x), y: Float(p.y), combine: dest.combine) }
            await MainActor.run {
                self.rangeDone(result, label: dest.group == nil ? "New \(title) Mask" : "\(dest.combine.title) \(title)")
            }
        }
    }

    private func rangeDone(_ result: Result<UInt32, Error>, label: String) {
        switch result {
        case .success(let id):
            used(id)
            commit(label)
            updateOverlay()
        case .failure(let e):
            model.statusMessage = "Range mask: \(e.localizedDescription)"
        }
    }

    // MARK: Overlay

    /// Asks the engine for the selected mask's overlay (or turns it off).
    func updateOverlay() {
        let group = active && overlayOn && model.viewMode == .loupe ? list.selectedID : nil
        develop?.setMaskOverlay(group)
        if group == nil { onOverlayFrame?(nil) }
        onLoupeChange?()
    }

    func overlayFrame(_ f: MaskOverlayFrame) {
        guard active, overlayOn, f.groupId == list.selectedID else { return }
        onOverlayFrame?(f)
    }

    // MARK: Thumbnails

    private func refreshThumbnails() {
        guard thumbnailsDirty, let d = develop else { return }
        thumbnailsDirty = false
        var next: [UInt32: CGImage] = [:]
        for g in list.groups {
            if let t = d.maskThumbnail(g.id, maxPixels: 64), let image = Self.image(t) { next[g.id] = image }
        }
        thumbnails = next
    }

    private static func image(_ t: MaskThumbnail) -> CGImage? {
        guard t.width > 0, t.height > 0, t.alpha.count == Int(t.width * t.height),
              let provider = CGDataProvider(data: Data(t.alpha) as CFData) else { return nil }
        return CGImage(width: Int(t.width), height: Int(t.height), bitsPerComponent: 8, bitsPerPixel: 8,
                       bytesPerRow: Int(t.width), space: CGColorSpaceCreateDeviceGray(),
                       bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.none.rawValue),
                       provider: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent)
    }

    // MARK: Keys

    /// Masking keys (before the culling map). Returns true when handled.
    func handleKey(_ event: NSEvent) -> Bool {
        guard model.viewMode == .loupe, model.developStatus == .ready else { return false }
        let mods = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        let ch = event.charactersIgnoringModifiers?.lowercased() ?? ""
        let plain = mods.subtracting([.shift, .option, .capsLock, .numericPad, .function]).isEmpty
        guard plain else { return false }
        if ch == "m", mods.isEmpty, !DevelopTools.shared.cropActive {
            toggle()
            return true
        }
        guard active else { return false }
        switch event.keyCode {
        case 53:                                                     // Esc
            if tool != nil { tool = nil; target = nil } else { setActive(false) }
            return true
        case 51, 117:                                                // ⌫ ⌦
            if let id = list.selectedID { delete(id) }
            return true
        default: break
        }
        switch ch {
        case "o":
            if mods.contains(.shift) {
                let all = MaskOverlayColor.allCases
                overlayColor = all[(all.firstIndex(of: overlayColor)! + 1) % all.count]
                if !overlayOn { overlayOn = true }
            } else {
                overlayOn.toggle()
            }
            return true
        case "x":
            invertSelected()
            return true
        case "[", "]", "{", "}":
            let up = ch == "]" || ch == "}"
            if mods.contains(.shift) {
                brushFeather = min(max(brushFeather + (up ? 10 : -10), 0), 100)
                model.statusMessage = String(format: "Brush feather %.0f", brushFeather)
            } else {
                brushSize = min(max(brushSize * (up ? 1.2 : 1 / 1.2), 2), 600)
            }
            onLoupeChange?()
            return true
        default:
            return false
        }
    }

    // MARK: LibraryObserver

    func libraryDidReload() {
        developDidChange()
        if ProcessInfo.processInfo.arguments.contains("--masks-selftest"), !selfTestRan { openLoupeForSelfTest() }
    }

    /// Self-test: the loupe on the first photo once the folder has loaded.
    private func openLoupeForSelfTest(polls: Int = 0) {
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
            MainActor.assumeIsolated {
                if self.model.isLoading || self.model.library.items.isEmpty {
                    if polls < 60 { self.openLoupeForSelfTest(polls: polls + 1) }
                } else if self.model.viewMode != .loupe {
                    self.model.viewMode = .loupe
                }
            }
        }
    }

    func itemsDidChange(_ positions: IndexSet) {}
    func selectionDidChange(scrollToFocus: Bool) {
        if active, model.viewMode != .loupe { setActive(false) }
    }

    func developDidChange() {
        strokeGroup = nil
        gradient = nil
        target = nil
        list = MaskListState()
        thumbnails = [:]
        if let d = develop {
            d.onMaskOverlay = { [weak self] f in self?.overlayFrame(f) }
            d.onMaskJob = { [weak self] u in self?.aiProgress(u) }
        } else if active {
            active = false
            tool = nil
        }
        refresh()
        updateOverlay()
        if let d = develop, ProcessInfo.processInfo.arguments.contains("--masks-selftest"), !selfTestRan {
            selfTestRan = true
            runSelfTest(d)
        }
    }

    func developDidRender(_ frame: DevelopFrame, controller: DevelopController) {
        if selfTestFrames != nil { selfTestFrames?.append(frame) }
        guard controller === develop, frame.isFinal, !frame.isOverlay, !list.groups.isEmpty else { return }
        refreshThumbnails()
    }

    // MARK: Self-test (`--masks-selftest`)

    @ObservationIgnored private var selfTestRan = false
    @ObservationIgnored private var selfTestFrames: [DevelopFrame]?

    /// Steps a drag once per display frame and summarises the engine's frame times.
    private func measure(_ d: DevelopController, _ name: String, _ steps: Int, _ step: (Int) -> Void) async -> String {
        selfTestFrames = []
        for i in 0...steps {
            step(i)
            d.flushPending()   // the display-link tick
            try? await Task.sleep(for: .milliseconds(16))
        }
        try? await Task.sleep(for: .milliseconds(700))
        let frames = selfTestFrames ?? []
        selfTestFrames = nil
        let drag = (frames.count > 1 ? Array(frames.dropLast()) : frames).map(\.renderMs).sorted()
        let levels = Set(frames.map(\.level)).sorted().map { "L\($0)" }.joined(separator: "/")
        let q = { (p: Double) in drag.isEmpty ? 0 : drag[Int(Double(drag.count - 1) * p)] }
        return String(format: "%@ %d frames %@ median %.1f ms p90 %.1f ms", name, drag.count, levels, q(0.5), q(0.9))
    }

    /// Drives the mask paths the UI uses — a linear gradient drag, a local Exposure drag and a brush
    /// stroke, each coalesced per display frame and committed on "mouse-up" — and reports the engine's
    /// frame times. Leaves the masks on the photo for inspection.
    private func runSelfTest(_ d: DevelopController) {
        Task { @MainActor [weak self] in
            try? await Task.sleep(for: .seconds(1.5))
            guard let self, self.develop === d else { return }
            self.setActive(true)
            var lines: [String] = []
            lines.append(await self.measure(d, "linear gradient", 30) { i in
                if i == 0 {
                    _ = self.beginGradient(LinearGradientShape(start: (0.5, 0), end: (0.5, 0.05)).json, title: "Linear Gradient", option: false)
                } else {
                    self.shapeGradient(LinearGradientShape(start: (0.5, 0), end: (0.5, 0.05 + 0.5 * Double(i) / 30)).json, final: i == 30)
                }
            })
            if let p = LocalParam.all.first(where: { $0.name == "exposure" }) {
                lines.append(await self.measure(d, "local exposure", 40) { i in self.setParam(p, -1.5 * Double(i) / 40, final: i == 40) })
            }
            lines.append(await self.measure(d, "brush", 40) { i in
                let x = 0.3 + 0.4 * Double(i) / 40, y = 0.6 + 0.1 * sin(Double(i) / 6)
                if i == 0 {
                    self.target = nil
                    self.list.select(nil)
                    self.beginStroke(at: (x, y), pressure: 1, erase: false, radius: 0.04)
                } else {
                    self.continueStroke(at: (x, y), pressure: 1)
                    if i == 40 { self.endStroke(erase: false) }
                }
            })
            if let p = LocalParam.all.first(where: { $0.name == "saturation" }) {
                lines.append(await self.measure(d, "brush saturation", 20) { i in self.setParam(p, -100 * Double(i) / 20, final: i == 20) })
            }
            let line = "masks-selftest: " + lines.joined(separator: "; ") + "; masks \(self.list.groups.count); backend \(d.info.backend)"
            FileHandle.standardError.write(Data((line + "\n").utf8))
            self.model.statusMessage = line
        }
    }
}
