import AppKit
import IOSurface
import Observation
import TesseraCore
import TesseraFFI

/// State and actions of the develop panels and loupe tools (M2-13): generic slider access by
/// JSON path, the crop & straighten tool, the HSL targeted adjustment, the 1:1 detail preview,
/// presets and the history list. Every change goes through the open `DevelopController`, i.e. the
/// same coalesced session path as the Basic sliders.
@MainActor @Observable
final class DevelopTools: LibraryObserver {
    static let shared = DevelopTools(model: .shared)

    @ObservationIgnored let model: AppModel

    // MARK: Observed tool state

    enum CurveMode: String, CaseIterable { case parametric = "Parametric", point = "Point" }
    var curveMode: CurveMode = .parametric
    var curveChannel: CurveChannel = .rgb
    /// Targeted adjustment in the loupe for this HSL property, if armed.
    var hslPicker: HSLProperty?
    /// Next loupe click sets the 1:1 detail preview position.
    var detailPicking = false

    private(set) var cropActive = false
    var cropAspect: CropAspect = .free
    var cropPortrait = false
    var cropOverlay: CropOverlay = .thirds
    var constrainCrop = true
    var straightening = false
    /// Mirrors the working crop angle for the panel (updated on commit points, not every drag frame).
    private(set) var cropAngle: Double = 0

    private(set) var presets: [DevelopPreset] = []
    private(set) var historyItems: [HistoryItem] = []
    /// Agent groups on the current lineage (History panel "Agent base edit" sections).
    private(set) var historyGroups: [HistoryGroupState] = []
    /// Bumped when panel values change outside a slider drag.
    private(set) var revision = 0
    /// The loupe screen's EDR presentation (HDR panel: slider range, status).
    var edr: EDRPresentation = .sdr

    // MARK: Unobserved hot state

    /// Working crop while the tool is active (displayed orientation).
    @ObservationIgnored private(set) var crop: CropGeometry?
    @ObservationIgnored private var cropAtStart: CropGeometry?
    /// Called on crop/tool changes that the loupe must redraw (hot path, no SwiftUI).
    @ObservationIgnored var onLoupeToolChange: (() -> Void)?
    @ObservationIgnored let presetStore = PresetStore.standard

    /// 1:1 detail preview: centre in sensor-normalised coordinates and the surface it renders into.
    @ObservationIgnored var detailCenter = (x: 0.5, y: 0.5)
    @ObservationIgnored private(set) var detailSurface: IOSurfaceRef?
    @ObservationIgnored var onDetailPreview: ((IOSurfaceRef?, DetailPreview?) -> Void)?
    @ObservationIgnored var detailPreviewVisible = false
    @ObservationIgnored private var detailBusy = false
    @ObservationIgnored private var detailDirty = false
    @ObservationIgnored private let detailQueue = DispatchQueue(label: "develop.detail-preview", qos: .userInitiated)

    init(model: AppModel) {
        self.model = model
        presets = presetStore.list()
        model.addObserver(self)
        if !model.library.items.isEmpty { libraryDidReload() }
    }

    var develop: DevelopController? { model.develop }
    var ready: Bool { model.developStatus == .ready && develop != nil }

    // MARK: Generic controls

    func value(_ c: DevelopControl) -> Double { develop?.number(at: c.path) ?? c.defaultValue }

    /// Posted (on the main actor) after any panel value change, so linked controls (curve ↔
    /// region sliders, wheel ↔ its sliders) follow a drag without SwiftUI.
    static let valuesChanged = Notification.Name("TesseraDevelopValuesChanged")

    /// Slider hot path: coalesced while dragging, one undo step on release.
    func set(_ c: DevelopControl, _ v: Double, final: Bool, label: String? = nil) {
        apply(c.patch(v), final: final, label: label ?? c.historyLabel(v))
    }

    func apply(_ patch: [String: Any], final: Bool, label: String) {
        guard let d = develop else { return }
        d.apply(patch: patch, interactive: !final)
        NotificationCenter.default.post(name: Self.valuesChanged, object: nil)
        if final {
            model.commitDevelop(label: label)
            refreshHistory()
        }
    }

    func string(_ path: [String]) -> String? { develop?.value(at: path) as? String }

    // MARK: Tone curve

    func pointCurve(_ channel: CurveChannel) -> PointCurve { PointCurve(json: develop?.value(at: channel.path)) }

    func parametric() -> ParametricCurveModel {
        ParametricCurveModel(amounts: ParametricRegion.allCases.reversed().map { value($0.control) },
                             splits: SplitPoint.allCases.map { develop?.number(at: $0.path) ?? $0.defaultValue })
    }

    func setPointCurve(_ curve: PointCurve, channel: CurveChannel, final: Bool) {
        apply(DevelopController.patch(channel.path, curve.json), final: final, label: "Point Curve (\(channel.title))")
    }

    func setSplits(_ p: ParametricCurveModel, final: Bool) {
        var obj: [String: Any] = [:]
        for (i, s) in SplitPoint.allCases.enumerated() { obj[s.rawValue] = p.splits[i] }
        apply(["tone": ["curves": ["parametric": obj]]], final: final, label: "Tone Curve Split Points")
    }

    // MARK: Crop & straighten

    /// Displayed (oriented) full-resolution size of the open image.
    var displayedSize: (width: Double, height: Double)? {
        guard let info = develop?.info else { return nil }
        let (w, h) = (Double(info.width), Double(info.height))
        return info.orientation >= 5 ? (h, w) : (w, h)
    }

    /// The committed crop from the settings (displayed orientation).
    func storedCrop() -> CropGeometry? {
        guard let d = develop, let size = displayedSize else { return nil }
        let rect = d.value(at: CropControls.rectPath) as? [String: Any]
        let n = { (k: String, def: Double) in (rect?[k] as? NSNumber)?.doubleValue ?? def }
        return CropGeometry(engineRect: (n("left", 0), n("top", 0), n("right", 1), n("bottom", 1)),
                            angle: d.number(at: CropControls.anglePath) ?? 0,
                            width: size.width, height: size.height, orientation: Int(d.info.orientation))
    }

    func toggleCrop() { cropActive ? commitCrop() : beginCrop() }

    func beginCrop() {
        guard !cropActive, let d = develop, model.viewMode == .loupe, var g = storedCrop() else {
            if model.viewMode != .loupe { model.statusMessage = "Crop works in the loupe (E)" }
            return
        }
        if g.cropWidth < 1 || g.cropHeight < 1 { g = CropGeometry(width: g.width, height: g.height) }
        let hint = (d.value(at: CropControls.aspectPath) as? [NSNumber])?.map(\.intValue)
        cropAspect = CropAspect(hint: hint, imageWidth: Int(g.width), imageHeight: Int(g.height)) ?? .free
        cropPortrait = g.cropHeight > g.cropWidth
        crop = g
        cropAtStart = g
        cropAngle = g.angle
        cropActive = true
        hslPicker = nil
        detailPicking = false
        d.setCropEditing(true)
        onLoupeToolChange?()
    }

    /// Done (Return): writes the crop as one history step and shows the cropped result.
    func commitCrop() {
        guard cropActive else { return }
        if let d = develop, let g = crop, g != cropAtStart {
            let size = displayedSize ?? (g.width, g.height)
            let hint = cropAspect.hint(imageWidth: Int(size.width), imageHeight: Int(size.height), portrait: cropPortrait)
            d.apply(patch: g.patch(orientation: Int(d.info.orientation), aspectHint: hint), interactive: false)
            model.commitDevelop(label: g.isIdentity ? "Reset Crop" : cropLabel(g))
        }
        endCrop()
    }

    /// Cancel (Esc): leaves the stored crop untouched.
    func cancelCrop() {
        guard cropActive else { return }
        endCrop()
    }

    private func endCrop() {
        cropActive = false
        straightening = false
        crop = nil
        cropAtStart = nil
        develop?.setCropEditing(false)
        revision += 1
        onLoupeToolChange?()
    }

    private func cropLabel(_ g: CropGeometry) -> String {
        g.angle == 0 ? "Crop" : String(format: "Crop & Straighten %+.2f°", g.angle)
    }

    /// Working-geometry update from the loupe overlay or the panel.
    func updateCrop(_ g: CropGeometry, settled: Bool) {
        crop = g
        if settled { cropAngle = g.angle }
        onLoupeToolChange?()
    }

    func setCropAspect(_ a: CropAspect) {
        cropAspect = a
        guard var g = crop ?? (cropActive ? nil : storedCrop()) else { return }
        if !cropActive { beginCrop(); g = crop ?? g }
        if let v = a.value(imageAspect: g.width / g.height, portrait: cropPortrait) {
            g.fitLargest(aspect: v)
        }
        updateCrop(g, settled: true)
    }

    /// X: swaps the crop between landscape and portrait (keeping a locked ratio).
    func flipCropOrientation() {
        guard cropActive, var g = crop else { return }
        cropPortrait.toggle()
        if let v = cropAspect.value(imageAspect: g.width / g.height, portrait: cropPortrait) {
            g.fitLargest(aspect: v)
        } else {
            (g.cropWidth, g.cropHeight) = (g.cropHeight, g.cropWidth)
            if constrainCrop { g.constrainToImage() }
        }
        updateCrop(g, settled: true)
    }

    func setCropAngle(_ degrees: Double, settled: Bool) {
        if !cropActive { beginCrop() }
        guard var g = crop else { return }
        g.rotate(to: degrees, constrain: constrainCrop)
        updateCrop(g, settled: settled)
    }

    func resetCrop() {
        if !cropActive {
            guard let size = displayedSize, let d = develop else { return }
            let g = CropGeometry(width: size.width, height: size.height)
            d.apply(patch: g.patch(orientation: Int(d.info.orientation), aspectHint: nil), interactive: false)
            model.commitDevelop(label: "Reset Crop")
            revision += 1
            return
        }
        guard let g0 = crop else { return }
        cropAspect = .free
        updateCrop(CropGeometry(width: g0.width, height: g0.height), settled: true)
    }

    // MARK: HSL targeted adjustment

    func toggleHSLPicker(_ p: HSLProperty) {
        hslPicker = hslPicker == p ? nil : p
        if hslPicker != nil { detailPicking = false; if cropActive { cancelCrop() } }
        onLoupeToolChange?()
    }

    func beginTargetedHSL(sample: (r: UInt8, g: UInt8, b: UInt8)) -> TargetedHSLAdjustment? {
        guard let p = hslPicker else { return nil }
        let t = TargetedHSLAdjustment(property: p, sample: sample) { self.value(p.control($0)) }
        return t.isEmpty ? nil : t
    }

    func dragTargetedHSL(_ t: TargetedHSLAdjustment, delta: Double, final: Bool) {
        let label = t.dominant.map { "\($0.title) \(t.property.title) " + String(format: "%+.0f", delta) } ?? "HSL"
        apply(t.patch(delta: delta), final: final, label: label)
        if final { revision += 1 }
    }

    // MARK: Detail preview

    /// Surface of `width × height` device pixels for the preview view.
    func ensureDetailSurface(width: Int, height: Int) {
        guard width > 8, height > 8 else { return }
        if let s = detailSurface, IOSurfaceGetWidth(s) == width, IOSurfaceGetHeight(s) == height { return }
        detailSurface = DevelopController.makeDetailSurface(width: width, height: height)
        requestDetailPreview()
    }

    /// Renders the 1:1 crop off the main actor; coalesces requests while one is running.
    func requestDetailPreview() {
        guard detailPreviewVisible, let d = develop, let surface = detailSurface else { return }
        if detailBusy { detailDirty = true; return }
        detailBusy = true
        detailDirty = false
        let session = d.session
        let center = detailCenter
        nonisolated(unsafe) let target = surface
        detailQueue.async { [weak self] in
            let result = try? DevelopController.renderDetail(session: session, into: target,
                                                             centerX: center.x, centerY: center.y)
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    guard let self else { return }
                    self.detailBusy = false
                    if self.develop?.session === session { self.onDetailPreview?(target, result) }
                    if self.detailDirty { self.requestDetailPreview() }
                }
            }
        }
    }

    func toggleDetailPicking() {
        detailPicking.toggle()
        if detailPicking { hslPicker = nil }
        onLoupeToolChange?()
    }

    /// Loupe click with the detail target armed: `u, v` are normalised coordinates of the
    /// displayed (cropped) picture.
    func pickDetail(u: Double, v: Double) {
        detailPicking = false
        guard let d = develop else { return }
        let g = storedCrop()
        let p: (u: Double, v: Double) = g.map { $0.imageUV(fromCropUV: u, v) } ?? (u, v)
        let s = CropGeometry.orient(p.u, p.v, Int(d.info.orientation))
        detailCenter = (min(max(s.0, 0), 1), min(max(s.1, 0), 1))
        onLoupeToolChange?()
        requestDetailPreview()
    }

    /// Pans the preview by device pixels of the 1:1 crop (displayed orientation ignored: the
    /// crop is shown in sensor orientation like the loupe's stored surfaces are).
    func panDetail(dx: Double, dy: Double) {
        guard let info = develop?.info else { return }
        detailCenter.x = min(max(detailCenter.x - dx / Double(info.width), 0), 1)
        detailCenter.y = min(max(detailCenter.y - dy / Double(info.height), 0), 1)
        requestDetailPreview()
    }

    // MARK: Masking preview (⌥ on Masking)

    func setMaskingPreview(_ on: Bool) { develop?.setMaskingPreview(on) }

    // MARK: Presets

    func reloadPresets() { presets = presetStore.list() }

    func savePreset(name: String, groups: Set<PresetGroup>) {
        guard let d = develop else { return }
        d.flushPending()
        let preset = DevelopPreset(name: name, groups: Array(groups), from: d.settingsObject)
        do {
            try presetStore.save(preset)
            reloadPresets()
            model.statusMessage = "Preset “\(name)” saved"
        } catch {
            model.statusMessage = "Preset not saved: \(error.localizedDescription)"
        }
    }

    func applyPreset(_ p: DevelopPreset) {
        guard let d = develop else { return }
        if cropActive { cancelCrop() }
        d.applyPreset(p.settings, label: "Preset: \(p.name)")
        model.developHistoryMove("Preset", label: p.name) { true }
        revision += 1
        refreshHistory()
    }

    func deletePreset(_ p: DevelopPreset) {
        try? presetStore.delete(p.name)
        reloadPresets()
    }

    // MARK: History

    func refreshHistory() {
        historyItems = develop?.historyItems() ?? []
        historyGroups = develop?.historyGroups() ?? []
    }

    /// The group's amount slider: previews while dragging, one undo step on release.
    func setGroupAmount(_ group: HistoryGroupState, _ amount: Double, final: Bool) {
        guard let d = develop else { return }
        if !final {
            d.previewGroupAmount(group, amount)
            return
        }
        model.developHistoryMove(group.name, label: AgentFade.percent(amount)) {
            try d.commitGroupAmount(group.groupId, amount)
        }
        revision += 1
        NotificationCenter.default.post(name: Self.valuesChanged, object: nil)
        refreshHistory()
    }

    func checkout(_ id: UInt64?) {
        guard let d = develop else { return }
        if cropActive { cancelCrop() }
        let label = id.flatMap { i in historyItems.first { $0.id == i }?.label } ?? "Original"
        model.developHistoryMove("History", label: label) { try d.checkoutHistory(id) }
        revision += 1
        refreshHistory()
    }

    func setStep(_ item: HistoryItem, enabled: Bool) {
        guard let d = develop else { return }
        model.developHistoryMove(enabled ? "Turn On" : "Turn Off", label: item.label) {
            try d.setHistoryStep(item.id, enabled: enabled)
        }
        revision += 1
        refreshHistory()
    }

    // MARK: LibraryObserver

    func libraryDidReload() {
        developDidChange()
        // Self-test: open the first photo in the loupe once the folder has loaded.
        if ProcessInfo.processInfo.arguments.contains("--develop-panels-selftest"), !selfTestRan,
           !model.library.items.isEmpty, model.viewMode != .loupe {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
                MainActor.assumeIsolated { self.model.viewMode = .loupe }
            }
        }
    }
    func itemsDidChange(_ positions: IndexSet) {}
    func selectionDidChange(scrollToFocus: Bool) {
        if cropActive, model.viewMode != .loupe || develop == nil { cancelCrop() }
    }

    @ObservationIgnored private var toolSession: ObjectIdentifier?

    func developDidChange() {
        // Tools belong to one image's session.
        let current = develop.map(ObjectIdentifier.init)
        if cropActive, current != toolSession || crop == nil { cancelCrop() }
        toolSession = current
        hslPicker = nil
        detailPicking = false
        detailCenter = (0.5, 0.5)
        revision += 1
        refreshHistory()
        requestDetailPreview()
        develop?.onSettingsReloaded = { [weak self] in self?.settingsReloaded() }
        if let d = develop, ProcessInfo.processInfo.arguments.contains("--develop-panels-selftest"), !selfTestRan {
            selfTestRan = true
            runPanelsSelfTest(d)
        }
    }

    func developDidRender(_ frame: DevelopFrame, controller: DevelopController) {
        if selfTestFrames != nil {
            selfTestFrames?.append(frame)
            if ProcessInfo.processInfo.environment["TESSERA_SELFTEST_VERBOSE"] == "1" {
                FileHandle.standardError.write(Data("frame g\(frame.generation) L\(frame.level) \(frame.renderMs) final=\(frame.isFinal)\n".utf8))
            }
        }
        guard frame.isFinal, !frame.isOverlay, controller === develop else { return }
        requestDetailPreview()
    }

    // MARK: Self-test (`--develop-panels-selftest`)

    @ObservationIgnored private var selfTestRan = false
    @ObservationIgnored private var selfTestFrames: [DevelopFrame]?

    /// Drags one control of every panel through the slider path (coalesced per display frame,
    /// committed on "mouse-up") and reports the engine's frame times per panel. Leaves a visible
    /// look on the image (curve, HSL, grading, vignette, grain) for inspection.
    private func runPanelsSelfTest(_ d: DevelopController) {
        let drags: [(String, DevelopControl, Double, Double)] = [
            ("tone curve", ParametricRegion.lights.control, 0, 35),
            ("hsl", HSLProperty.saturation.control(.blue), 0, -45),
            ("grading", GradeRange.shadows.saturation, 0, 25),
            ("detail", DetailControls.amount, 40, 90),
            ("vignette", EffectsControls.amount, 0, -40),
            ("grain", EffectsControls.grainAmount, 0, 20),
        ]
        Task { @MainActor [weak self] in
            try? await Task.sleep(for: .seconds(1.5))
            guard let self, self.develop === d else { return }
            self.apply(GradeRange.shadows.wheelPatch(hue: 230, saturation: 0), final: true, label: "Shadows Grade")
            var lines: [String] = []
            for (name, control, from, to) in drags {
                self.selfTestFrames = []
                for i in 0...40 {
                    self.set(control, from + (to - from) * Double(i) / 40, final: i == 40)
                    d.flushPending()   // the display-link tick (paused while the window is occluded)
                    try? await Task.sleep(for: .milliseconds(16))
                }
                try? await Task.sleep(for: .milliseconds(600))
                let frames = self.selfTestFrames ?? []
                // Every frame but the refinement after the final commit.
                let drag = (frames.count > 1 ? Array(frames.dropLast()) : frames).map(\.renderMs).sorted()
                let levels = Set(frames.map(\.level)).sorted().map { "L\($0)" }.joined(separator: "/")
                let q = { (p: Double) in drag.isEmpty ? 0 : drag[Int(Double(drag.count - 1) * p)] }
                lines.append(String(format: "%@ %d frames %@ median %.1f ms p90 %.1f ms", name, drag.count, levels, q(0.5), q(0.9)))
            }
            self.selfTestFrames = nil
            self.revision += 1
            let line = "develop-panels-selftest: " + lines.joined(separator: "; ") + "; backend \(d.info.backend)"
            FileHandle.standardError.write(Data((line + "\n").utf8))
            self.model.statusMessage = line
            // TESSERA_SELFTEST_CROP=1 leaves the crop tool open on a straightened 3:2 crop;
            // =commit applies it (the loupe then shows the cropped render).
            let crop = ProcessInfo.processInfo.environment["TESSERA_SELFTEST_CROP"]
            if crop == "1" || crop == "commit" {
                self.beginCrop()
                self.setCropAspect(.ratio(1, 1))
                self.setCropAngle(3.5, settled: true)
                if crop == "commit" { self.commitCrop() }
            }
        }
    }

    private func settingsReloaded() {
        revision += 1
        refreshHistory()
        NotificationCenter.default.post(name: Self.valuesChanged, object: nil)
    }
}
