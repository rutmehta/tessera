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

/// The Try cards' state: real renders of this photo with each look (cached per photo), and which
/// looks are on. Looks are recorded through the normal develop history so the Edits stack shows them.
@MainActor @Observable
final class TryLooks: LibraryObserver {
    static let shared = TryLooks(model: .shared)

    @ObservationIgnored let model: AppModel

    private(set) var thumbnails: [TryLook: CGImage] = [:]
    /// The photo without any look, while the renders are pending.
    private(set) var plain: CGImage?
    /// Looks that are on, with the history step each recorded.
    private(set) var active: [TryLook: UInt64] = [:]
    private(set) var rendering = false

    @ObservationIgnored private var cache: [String: [TryLook: CGImage]] = [:]
    @ObservationIgnored private var renderTask: Task<Void, Never>?
    @ObservationIgnored private var renderedFor: String?
    @ObservationIgnored private var firstFrameSeen = false

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

    /// Renders the looks that have what they need (mask looks wait for the AI masks) once the loupe
    /// shows the photo, through the session's own pipeline at card size, and caches them per photo.
    private func scheduleRender(delay: Double = 0.3) {
        guard ready, let d = develop, firstFrameSeen else { return }
        let key = d.imageID
        if let cached = cache[key] { thumbnails = cached }
        let wanted = TryLook.allCases.filter { thumbnails[$0] == nil && $0 != .style }
        let settings = d.freshSettings()
        let looks: [(TryLook, [String: Any])] = wanted.compactMap { look in
            look.patch(over: settings, group: look.maskTarget.flatMap { dial.groupID($0) }).map { (look, $0) }
        }
        Self.log("schedule: ready=\(ready) wanted=\(wanted.map(\.rawValue)) looks=\(looks.map(\.0.rawValue)) busy=\(renderTask != nil)")
        guard !looks.isEmpty, renderTask == nil else { return }
        rendering = true
        renderTask = Task { [weak self] in
            try? await Task.sleep(for: .seconds(delay))
            guard let self, !Task.isCancelled, self.develop === d, self.ready else { self?.finishRender(); return }
            let scale = NSScreen.main?.backingScaleFactor ?? 2
            let images = await d.renderLooks(looks.map(\.1), maxPixels: Int(Theme.Width.tryCard * 2 * scale))
            Self.log("rendered: \(images.map { $0.map { "\($0.width)x\($0.height)" } ?? "nil" })")
            guard !Task.isCancelled, self.develop === d else { self.finishRender(); return }
            for (i, (look, _)) in looks.enumerated() {
                if let image = images[i] { self.thumbnails[look] = image }
            }
            self.cache[key] = self.thumbnails
            if self.cache.count > 12, let old = self.cache.keys.first(where: { $0 != key }) { self.cache.removeValue(forKey: old) }
            self.finishRender()
        }
    }

    private func finishRender() {
        renderTask = nil
        rendering = false
    }

    /// `TESSERA_LOOKS_LOG=1`: the render pass on stderr.
    static func log(_ line: String) {
        guard ProcessInfo.processInfo.environment["TESSERA_LOOKS_LOG"] == "1" else { return }
        FileHandle.standardError.write(Data(("looks: " + line + "\n").utf8))
    }

    /// The AI masks segmented (the dial created them): the mask looks can render now.
    func masksDidBecomeReady() { scheduleRender(delay: 0.1) }

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
        renderTask?.cancel()
        renderTask = nil
        rendering = false
        active = [:]
        thumbnails = [:]
        plain = nil
        firstFrameSeen = false
        if let item = model.editTarget { plain = model.loader.cached(item, tier: .thumbnail) }
    }

    func workspaceWillEnterPhotoEdit() { reset() }
    func workspaceWillLeavePhotoEdit() { reset() }
    func libraryDidReload() { reset() }
    func itemsDidChange(_ positions: IndexSet) {}
    func selectionDidChange(scrollToFocus: Bool) { if model.isPhotoEditing { reset() } }
    func developDidChange() {
        reset()
        if let d = develop, let cached = cache[d.imageID] { thumbnails = cached }
    }

    func developDidRender(_ frame: DevelopFrame, controller: DevelopController) {
        guard controller === develop, frame.isFinal, !frame.isOverlay, !firstFrameSeen else { return }
        firstFrameSeen = true
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
                    if looks.rendering, looks.thumbnails[look] == nil, look != .style {
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
