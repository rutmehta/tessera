import AppKit
import Observation
import SwiftUI
import TesseraCore
import TesseraFFI

/// One Try card (SPEC §3): a look for this photo, previewed on the card by a real render of the
/// photo with the look, applied as one history step when clicked, removed when clicked again.
enum TryLook: String, CaseIterable, Identifiable {
    case relight, sky, film, crowd, bw, style
    var id: String { rawValue }

    var title: String {
        switch self {
        case .relight: "Relight face"
        case .sky: "Deeper sky"
        case .film: "Film look"
        case .crowd: "Clear the crowd"
        case .bw: "Black and white"
        case .style: "More from how you usually edit"
        }
    }

    var help: String {
        switch self {
        case .relight: "Open the shadows and lift the subject"
        case .sky: "Darker, richer sky"
        case .film: "Softer contrast, lifted blacks, warm highlights and grain"
        case .crowd: "Quieten the background: darker, softer, less colour (the engine removes objects in Layers only)"
        case .bw: "Black and white conversion"
        case .style: "Run the style profile on this photo (an agent edit, reviewed like any other)"
        }
    }

    /// The AI mask the look adjusts, if local.
    var maskTarget: DialTarget? {
        switch self {
        case .relight: .subject
        case .sky: .sky
        case .crowd: .background
        default: nil
        }
    }

    /// Local parameters of a mask look (engine names, `LocalParam.all`).
    private var localParams: [String: Double] {
        switch self {
        case .relight: ["shadows": 45, "exposure": 0.25, "highlights": -10]
        case .sky: ["highlights": -45, "exposure": -0.3, "saturation": 22, "dehaze": 10]
        case .crowd: ["exposure": -0.45, "clarity": -30, "saturation": -30, "sharpness": -40]
        default: [:]
        }
    }

    /// The settings patch of the look over `settings`; nil when it needs a mask the photo lacks,
    /// or is not a patch (the style profile is an agent run).
    func patch(over settings: [String: Any], group: UInt32?) -> [String: Any]? {
        switch self {
        case .film:
            return ["tone": ["contrast": -12, "blacks": 14, "highlights": -10],
                    "color": ["saturation": -14,
                              "grading": ["shadows": ["hue": 215, "saturation": 14], "highlights": ["hue": 48, "saturation": 12], "blending": 50]],
                    "effects": ["grain": ["amount": 22, "size": 30, "roughness": 50]]]
        case .bw:
            return ["color": ["monochrome": ["enabled": true]]]
        case .style:
            return nil
        case .relight, .sky, .crowd:
            guard let group,
                  var adjustments = (settings["locals"] as? [String: Any])?["adjustments"] as? [[String: Any]],
                  let i = adjustments.firstIndex(where: { ($0["id"] as? NSNumber)?.uint32Value == group }) else { return nil }
            var params = adjustments[i]["params"] as? [String: Any] ?? [:]
            for (k, v) in localParams { params[k] = v }
            adjustments[i]["params"] = params
            return ["locals": ["adjustments": adjustments]]
        }
    }
}

/// When a Try card may render (ST-2). Cards are a background nicety: they never compete with the
/// photo's own render, a slider drag or an export, and they go one at a time.
struct TryCardPolicy {
    /// How long the loupe must have been quiet (no frame of its own) before a card renders.
    static let idle: TimeInterval = 1
    /// The coarsest pyramid level a card asks for (`DevelopController.renderLooks(minLevel:)`).
    static let minLevel = 5

    var firstFrameSeen = false
    /// The last frame the loupe showed (the cards' own frames do not count).
    var lastFrameAt: Date?
    var interacting = false
    var exporting = false
    var busy = false

    /// Whether a card may start now.
    func mayRender(at now: Date) -> Bool {
        guard firstFrameSeen, !interacting, !exporting, !busy, let lastFrameAt else { return false }
        return now.timeIntervalSince(lastFrameAt) >= Self.idle
    }

    /// How long to wait before asking again (nil: nothing to wait for, ask on the next event).
    func delay(at now: Date) -> TimeInterval? {
        guard firstFrameSeen, let lastFrameAt else { return nil }
        return max(Self.idle - now.timeIntervalSince(lastFrameAt), 0)
    }
}

/// The Try cards' state: real renders of this photo with each look, and which looks are on. Looks
/// are recorded through the normal develop history so the Edits stack shows them.
///
/// Renders are lazy (ST-2): nothing starts on open. After the photo's own first frame, once the
/// loupe has been idle for `TryCardPolicy.idle`, one card renders at a time at a coarse level;
/// the pass is cancelled when the photo changes and never starts during a slider drag or an
/// export. Only the current photo's renders are kept.
@MainActor @Observable
final class TryLooks: LibraryObserver {
    static let shared = TryLooks(model: .shared)

    @ObservationIgnored let model: AppModel

    private(set) var thumbnails: [TryLook: CGImage] = [:]
    /// The photo without any look, while the renders are pending.
    private(set) var plain: CGImage?
    /// Looks that are on, with the history step each recorded.
    private(set) var active: [TryLook: UInt64] = [:]
    /// The card rendering now (its spinner).
    private(set) var renderingLook: TryLook?
    var rendering: Bool { renderingLook != nil }

    /// The current photo's renders, kept across a develop reopen of the same photo.
    @ObservationIgnored private var cached: (imageID: String, thumbnails: [TryLook: CGImage])?
    @ObservationIgnored private var renderTask: Task<Void, Never>?
    @ObservationIgnored private var idleTask: Task<Void, Never>?
    @ObservationIgnored private var policy = TryCardPolicy()

    init(model: AppModel) {
        self.model = model
        model.addObserver(self)
    }

    private var dial: PhotoDial { .shared }
    private var tools: DevelopTools { .shared }
    var develop: DevelopController? { model.develop }
    var ready: Bool { model.isPhotoEditing && model.developStatus == .ready && develop != nil }

    func isOn(_ look: TryLook) -> Bool {
        look == .style ? model.agent.isRunning : active[look] != nil
    }

    /// The card's picture: the render with the look, else the photo as it is (the loader's
    /// thumbnail, fetched again when it was not cached at reset).
    func image(_ look: TryLook) -> CGImage? {
        if let image = thumbnails[look] { return image }
        if plain == nil, let item = model.editTarget { plain = model.loader.cached(item, tier: .thumbnail) }
        return plain
    }

    // MARK: Renders

    /// The policy's view of now: the loupe's frames, the sliders and the exporter.
    private func currentPolicy() -> TryCardPolicy {
        var p = policy
        p.interacting = develop?.isInteracting ?? false
        p.exporting = model.exporter.isRunning
        p.busy = renderTask != nil
        return p
    }

    /// Waits for the loupe to be idle, then renders the next card. Re-armed by every loupe frame
    /// (a drag keeps producing frames, so it keeps pushing the cards back).
    private func scheduleRender() {
        idleTask?.cancel()
        idleTask = nil
        guard ready, let d = develop, renderTask == nil else { return }
        guard let delay = currentPolicy().delay(at: Date()) else { return }
        idleTask = Task { [weak self] in
            try? await Task.sleep(for: .seconds(delay))
            guard let self, !Task.isCancelled, self.develop === d else { return }
            self.idleTask = nil
            if self.currentPolicy().mayRender(at: Date()) { self.renderNext(d) } else { self.scheduleRender() }
        }
    }

    /// One card: the first look without a render that has what it needs (mask looks wait for the
    /// AI masks), through the session's own pipeline at a coarse level. The next card follows
    /// once the loupe is idle again.
    private func renderNext(_ d: DevelopController) {
        guard ready, renderTask == nil else { return }
        let settings = d.freshSettings()
        let next: (TryLook, [String: Any])? = TryLook.allCases.lazy
            .filter { self.thumbnails[$0] == nil && $0 != .style }
            .compactMap { look in look.patch(over: settings, group: look.maskTarget.flatMap { self.dial.groupID($0) }).map { (look, $0) } }
            .first
        Self.log("next: ready=\(ready) look=\(next?.0.rawValue ?? "none")")
        guard let (look, patch) = next else { return }
        renderingLook = look
        let key = d.imageID
        renderTask = Task { [weak self] in
            let scale = NSScreen.main?.backingScaleFactor ?? 2
            let images = await d.renderLooks([patch], maxPixels: Int(Theme.Width.tryCard * 2 * scale),
                                             minLevel: TryCardPolicy.minLevel)
            Self.log("rendered \(look.rawValue): \(images.map { $0.map { "\($0.width)x\($0.height)" } ?? "nil" })")
            guard let self else { return }
            self.renderTask = nil
            self.renderingLook = nil
            guard !Task.isCancelled, self.develop === d, self.ready else { return }
            if let image = images.first ?? nil { self.thumbnails[look] = image }
            self.cached = (key, self.thumbnails)
            self.scheduleRender()
        }
    }

    private func cancelRenders() {
        idleTask?.cancel()
        idleTask = nil
        renderTask?.cancel()
        renderTask = nil
        renderingLook = nil
    }

    /// `TESSERA_LOOKS_LOG=1`: the render pass on stderr.
    static func log(_ line: String) {
        guard ProcessInfo.processInfo.environment["TESSERA_LOOKS_LOG"] == "1" else { return }
        FileHandle.standardError.write(Data(("looks: " + line + "\n").utf8))
    }

    /// The AI masks segmented (the dial created them): the mask looks can render now.
    func masksDidBecomeReady() { scheduleRender() }

    // MARK: Apply

    func toggle(_ look: TryLook) {
        guard ready, let d = develop, let item = model.editTarget else { return }
        if look == .style {
            guard !model.agent.isRunning else { model.statusMessage = "An agent edit is already running"; return }
            model.agent.start(itemIDs: [item.id], provider: .styleProfile)
            model.statusMessage = model.agent.isRunning ? "Editing \(item.name) from your style profile" : (model.agent.error ?? "Style profile: could not start")
            return
        }
        if let step = active[look] {
            active.removeValue(forKey: look)
            if d.historyItems().first(where: { $0.isHead })?.id == step {
                model.developHistoryMove("Remove", label: look.title) { try d.undo() }
            } else {
                // Later steps exist: record the removal as a step of its own.
                let settings = d.freshSettings()
                if let patch = look.patch(over: settings, group: look.maskTarget.flatMap { dial.groupID($0) }) {
                    tools.apply(DevelopController.inverse(of: patch, in: settings), final: true, label: "Remove \(look.title)")
                }
            }
            tools.refreshHistory()
            return
        }
        if let target = look.maskTarget, !dial.ensureAIMasks() {
            model.statusMessage = "Finding the \(target.title.lowercased())… try again in a moment"
            return
        }
        guard let patch = look.patch(over: d.freshSettings(), group: look.maskTarget.flatMap { dial.groupID($0) }) else { return }
        tools.apply(patch, final: true, label: "Try · \(look.title)")
        active[look] = d.historyItems().first { $0.isHead }?.id
        tools.refreshHistory()
    }

    // MARK: LibraryObserver

    private func reset() {
        cancelRenders()
        active = [:]
        thumbnails = [:]
        plain = nil
        policy = TryCardPolicy()
        if let item = model.editTarget { plain = model.loader.cached(item, tier: .thumbnail) }
    }

    func workspaceWillEnterPhotoEdit() { reset() }
    func workspaceWillLeavePhotoEdit() { reset(); cached = nil }
    func libraryDidReload() { reset(); cached = nil }
    func itemsDidChange(_ positions: IndexSet) {}
    func selectionDidChange(scrollToFocus: Bool) { if model.isPhotoEditing { reset() } }
    func developDidChange() {
        reset()
        guard let d = develop else { return }
        if let cached, cached.imageID == d.imageID { thumbnails = cached.thumbnails } else { cached = nil }
    }

    /// Every frame of the loupe's own render re-arms the idle wait; the first final one lets
    /// the cards start. Frames of a card pass (the tiny surface) are not the loupe's.
    func developDidRender(_ frame: DevelopFrame, controller: DevelopController) {
        guard controller === develop, !frame.isOverlay, renderTask == nil else { return }
        policy.lastFrameAt = Date()
        if frame.isFinal { policy.firstFrameSeen = true }
        scheduleRender()
    }
}

/// The row of Try cards above the agent field in Edit.
struct TryCardsRow: View {
    let model: AppModel
    private var looks: TryLooks { .shared }

    var body: some View {
        HStack(spacing: Theme.Space.s + Theme.Space.xxs) {
            ForEach(TryLook.allCases) { look in
                TryCard(look: look, looks: looks)
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Try")
    }
}

/// 132 × (64 + 28): the render with the look, then the caption and "on" when the look is applied.
private struct TryCard: View {
    let look: TryLook
    let looks: TryLooks
    @State private var hovering = false

    var body: some View {
        let on = looks.isOn(look)
        Button { looks.toggle(look) } label: {
            VStack(spacing: 0) {
                ZStack {
                    Rectangle().fill(Theme.raised)
                    if let image = looks.image(look) {
                        Image(decorative: image, scale: 1)
                            .resizable()
                            .aspectRatio(contentMode: .fill)
                            .frame(width: Theme.Width.tryCard, height: Theme.Height.tryThumbnail)
                            .clipped()
                    }
                    if looks.renderingLook == look {
                        ProgressView().controlSize(.small)
                    }
                }
                .frame(width: Theme.Width.tryCard, height: Theme.Height.tryThumbnail)
                HStack {
                    Text(look.title).lineLimit(1).truncationMode(.tail)
                    Spacer(minLength: Theme.Space.xs)
                    Text(on ? "on" : "").foregroundStyle(Theme.accent)
                }
                .font(Theme.Fonts.captionSemibold)
                .foregroundStyle(Theme.textPrimary)
                .padding(.horizontal, Theme.Space.s + Theme.Space.hairline)
                .frame(width: Theme.Width.tryCard, height: Theme.Height.tryCaption)
            }
            .background(Theme.panel)
            .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.card + Theme.Space.hairline))
            .overlay(RoundedRectangle(cornerRadius: Theme.Radius.card + Theme.Space.hairline)
                .strokeBorder(on ? Theme.accent.opacity(0.7) : hovering ? Theme.hairlineStrong : Theme.hairline,
                              lineWidth: Theme.Space.hairline))
        }
        .buttonStyle(.plain)
        .onHover { hovering = $0 }
        .help(look.help)
        .accessibilityLabel(look.title)
        .accessibilityValue(on ? "on" : "off")
        .accessibilityAddTraits(on ? .isSelected : [])
        .accessibilityIdentifier("develop.try.\(look.rawValue)")
    }
}
