import AppKit
import SwiftUI
import TesseraCore

/// Cull's canvas (redesign SPEC §4, WP UX-14): the triage column, the header (burst, frames,
/// time, exposure · the pick and why · Burst / Grid) and, in the burst layout, one burst at a
/// time: the focused frame large with its signals as HUD pills, the burst as a strip of cells
/// with the pick badged and decisions marked, and the actions row. In the grid layout the
/// Library grid (grouped by burst) shows through the centre.
struct CullWorkspace: View {
    let model: AppModel

    var body: some View {
        HStack(spacing: 0) {
            if model.showInspector {
                TriageColumn(model: model)
                    .transition(Theme.Motion.transition(from: .leading))
            }
            VStack(spacing: Theme.Space.m) {
                BurstHeader(model: model)
                if model.cullLayout == .burst {
                    BurstReviewView(model: model)
                } else {
                    // The grouped grid is the Library grid underneath, inset to this area.
                    Color.clear.allowsHitTesting(false)
                }
            }
            .padding(.top, Theme.Space.m)
            .padding(.horizontal, Theme.Space.xl)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .animation(Theme.Motion.appear, value: model.showInspector)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("library.cull.workspace")
    }

    /// Where the Library grid shows through in the grid layout: below the header, right of the
    /// column, above the quick actions and the agent field.
    static let gridTopInset: CGFloat = Theme.Space.m + Theme.Height.large + Theme.Space.m
    static let gridBottomInset: CGFloat = Theme.Height.agentField + Theme.Height.large + Theme.Space.l + Theme.Space.s + Theme.Space.xxs
        + Theme.Height.toastBand
}

/// "Burst n · k frames · time · exposure" · "Pick: name · sharpest of k, eyes open" · Burst / Grid.
private struct BurstHeader: View {
    let model: AppModel
    @State private var exposure: (id: Int, text: String)?

    var body: some View {
        let _ = model.decisionRevision
        let burst = model.focusedBurst
        HStack(alignment: .firstTextBaseline, spacing: Theme.Space.m) {
            HStack(alignment: .firstTextBaseline, spacing: Theme.Space.xs) {
                Text(title(burst)).font(Theme.Fonts.title).foregroundStyle(Theme.textPrimary)
                    .accessibilityIdentifier("library.cull.burst.title")
                Text(detail(burst)).font(Theme.Fonts.labelNumeric).foregroundStyle(Theme.textTertiary)
                    .lineLimit(1).truncationMode(.tail)
                    .accessibilityIdentifier("library.cull.burst.position")
            }
            // UX-16: the person filter (Show me <name>) is cleared here; the status bar is gone.
            if let person = model.assist.personFilterTitle {
                Button { model.assist.clearPersonFilter() } label: {
                    HStack(spacing: Theme.Space.xs) {
                        Text(person).lineLimit(1).truncationMode(.middle)
                        Image(systemName: "xmark.circle.fill").font(Theme.Fonts.iconSmall)
                    }
                }
                .buttonStyle(.theme(.borderless, height: Theme.Height.regular))
                .foregroundStyle(Theme.accent)
                .help("Showing frames with \(person) only. Click to show all.")
                .accessibilityIdentifier("status-person-filter")
                .frame(maxWidth: Theme.Width.labelWide * 2)
            }
            Spacer(minLength: Theme.Space.m)
            Text(pickReason(burst)).font(Theme.Fonts.label).foregroundStyle(Theme.textTertiary)
                .lineLimit(1).truncationMode(.middle)
                .accessibilityIdentifier("library.cull.burst.pick")
            HStack(spacing: Theme.Space.xs) {
                SegmentedPicker(selection: Binding(get: { model.cullLayout }, set: { model.cullLayout = $0 }),
                                segments: CullLayout.allCases.map { .init(value: $0, title: $0.title, help: "\($0.title) view (G)") },
                                height: Theme.Height.regular, fill: false, accessibilityPrefix: "library.cull.layout")
                .fixedSize()
                KeyCap("G")
            }
        }
        .frame(height: Theme.Height.large)
        .onChange(of: burst?.pick, initial: true) { _, pick in loadExposure(pick) }
    }

    private func title(_ burst: (group: Int, frames: Range<Int>, pick: Int)?) -> String {
        guard let burst else { return model.library.items.isEmpty ? "No photos" : "No frame" }
        if burst.frames.count == 1 { return "Single frame" }
        return "Burst \(model.focusedBurstOrdinal ?? burst.group + 1) · \(burst.frames.count) frames"
    }

    /// "· 18:06:48 · 1/640 f/4 · 3 of 226": the pick's time and exposure, then the focused
    /// frame's position in the view (UX-16: formerly the status bar's).
    private func detail(_ burst: (group: Int, frames: Range<Int>, pick: Int)?) -> String {
        guard let burst else { return "" }
        let pick = model.item(id: burst.pick)
        var parts = ["· " + Self.time.string(from: pick.captureDate)]
        if let exposure, exposure.id == burst.pick, !exposure.text.isEmpty { parts.append("· " + exposure.text) }
        if let p = model.focusedPosition { parts.append("· \((p + 1).formatted()) of \(model.visibleCount.formatted())") }
        return parts.joined(separator: " ")
    }

    private func pickReason(_ burst: (group: Int, frames: Range<Int>, pick: Int)?) -> String {
        guard let burst else { return "" }
        guard burst.frames.count > 1 else { return "Single frame · no burst to compare" }
        let name = model.item(id: burst.pick).name
        if let why = CullTriage.pickReason(pick: burst.pick, group: burst.frames, signals: model.assist.signals) {
            return "Pick: \(name) · \(why)"
        }
        return "Pick: \(name) · " + (model.assist.isRunning ? "measuring…" : "by file size until measured")
    }

    /// Shutter and aperture of the pick, from the catalog's EXIF (one read per burst).
    private func loadExposure(_ pick: Int?) {
        guard let pick, exposure?.id != pick else { return }
        guard let fields = try? model.collections.catalog?.metadata(of: pick)?.fields else { exposure = (pick, ""); return }
        var parts: [String] = []
        if let t = fields.first(where: { $0.name == "ExposureTime" })?.value, !t.isEmpty { parts.append(t.contains("/") || t.contains("s") ? t : t + " s") }
        if let f = fields.first(where: { $0.name == "FNumber" })?.value, !f.isEmpty { parts.append(f.hasPrefix("f") ? f : "f/" + f) }
        exposure = (pick, parts.joined(separator: " "))
    }

    private static let time: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "HH:mm:ss"
        return f
    }()
}

/// One burst: the focused frame large with its signals, the strip and the actions.
struct BurstReviewView: View {
    let model: AppModel

    var body: some View {
        let _ = model.decisionRevision
        let burst = model.focusedBurst
        VStack(spacing: Theme.Space.m) {
            if let item = model.focusedItem, let burst {
                let inBurst = burst.frames.count > 1
                BigFrame(model: model, item: item, isPick: inBurst && item.id == burst.pick)
                    .frame(maxWidth: Theme.Width.burstPhotoMax, maxHeight: .infinity)
                    .frame(maxWidth: .infinity)
                // The strip is centred until it is wider than the view, then it scrolls.
                GeometryReader { geo in
                    ScrollView(.horizontal) {
                        HStack(spacing: Theme.Space.s + Theme.Space.xxs) {
                            ForEach(Array(burst.frames), id: \.self) { id in
                                if model.positionOfID.indices.contains(id), model.positionOfID[id] >= 0 {
                                    BurstStripCell(model: model, item: model.item(id: id), focused: id == item.id,
                                                   isPick: inBurst && id == burst.pick)
                                }
                            }
                        }
                        .frame(minWidth: geo.size.width)
                    }
                    .scrollIndicators(.never)
                }
                .frame(height: Theme.Height.burstCell + Theme.Space.xs)
                .accessibilityIdentifier("library.cull.burst.strip")
                BurstActions(model: model, single: burst.frames.count == 1)
                    .shellProbe("burstActions")
            } else {
                EmptyStateContent(symbol: "square.stack", title: "Nothing in view",
                                  message: "No frame matches this filter.") {
                    Button("Show the whole shoot") { model.setCullFilter(nil) }
                        .buttonStyle(.theme(.bordered, height: Theme.Height.large))
                        .disabled(model.cullFilter == nil)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        // Clear of the agent field, the chips and the toast band above them (UX-15 item 20).
        .padding(.bottom, Theme.Height.agentField + Theme.Height.large * 2 + Theme.Space.xl + Theme.Height.toastBand)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("library.cull.burst")
    }
}

/// The frame large, outlined by its state (the accent for the pick, keep / reject once decided),
/// with the pick badge, the name and the signals as HUD pills.
private struct BigFrame: View {
    let model: AppModel
    let item: PhotoItem
    let isPick: Bool

    var body: some View {
        let state = model.state(id: item.id)
        let outline: Color = state.decision == .keep ? Theme.keep : state.decision == .reject ? Theme.reject : isPick ? Theme.accent : Theme.clear
        GeometryReader { geo in
            let aspect = CGFloat(item.aspectRatio)
            let fit = fitted(geo.size, aspect: aspect)
            ZStack(alignment: .topLeading) {
                PreviewImageView(model: model, item: item, fill: false)
                if state.decision == .reject { Theme.canvas.opacity(Theme.Opacity.hidden) }
                HStack(alignment: .top) {
                    if isPick {
                        Text("Agent's pick").font(Theme.Fonts.labelSemibold).foregroundStyle(Theme.textOnAccent)
                            .padding(.horizontal, Theme.Space.s + Theme.Space.xxs).frame(height: Theme.Height.regular)
                            .background(Capsule().fill(Theme.accent))
                            .accessibilityIdentifier("library.cull.burst.pickBadge")
                    }
                    Spacer()
                    Text(item.name + (state.decision == .keep ? " · kept" : state.decision == .reject ? " · rejected" : ""))
                        .font(Theme.Fonts.label).foregroundStyle(Theme.textSecondary)
                        .padding(.horizontal, Theme.Space.s + Theme.Space.xxs).frame(height: Theme.Height.regular)
                        .background(Capsule().fill(Theme.hud))
                        .accessibilityIdentifier("library.cull.burst.name")
                }
                .padding(Theme.Space.m)
                VStack { Spacer(); SignalPills(signals: model.assist.signals[item.id], score: model.assist.scores[item.id]?.technical) }
                    .padding(Theme.Space.m)
            }
            .frame(width: fit.width, height: fit.height)
            .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.control))
            .overlay(RoundedRectangle(cornerRadius: Theme.Radius.control).strokeBorder(outline, lineWidth: Theme.Space.xxs))
            .position(x: geo.size.width / 2, y: geo.size.height / 2)
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("\(item.name), \(state.decision.label)" + (isPick ? ", the pick" : ""))
        .accessibilityIdentifier("library.cull.burst.photo")
    }

    private func fitted(_ size: CGSize, aspect: CGFloat) -> CGSize {
        guard size.width > 0, size.height > 0 else { return .zero }
        let w = min(size.width, size.height * aspect)
        return CGSize(width: w, height: w / aspect)
    }
}

/// Sharpness (a bar) · Eyes · Face · Score, over the photo. Missing signals say so.
private struct SignalPills: View {
    let signals: FrameSignals?
    let score: Double?

    var body: some View {
        HStack(spacing: Theme.Space.xs + Theme.Space.xxs) {
            pill {
                Text("Sharpness").foregroundStyle(Theme.textTertiary)
                ZStack(alignment: .leading) {
                    RoundedRectangle(cornerRadius: Theme.Space.xxs).fill(Theme.track)
                    RoundedRectangle(cornerRadius: Theme.Space.xxs).fill(Theme.textPrimary)
                        .frame(width: Theme.Width.signalBar * CGFloat(min(max(signals?.sharpness ?? 0, 0), 1)))
                }
                .frame(width: Theme.Width.signalBar, height: Theme.Space.xs)
                .accessibilityLabel("Sharpness \(signals?.sharpness.map { String(format: "%.2f", $0) } ?? "unmeasured")")
            }
            pill {
                Text("Eyes").foregroundStyle(Theme.textTertiary)
                if let e = signals?.eyesOpen {
                    Text(e >= 0.5 ? "open" : e >= DefectRule.closedEyes.threshold ? "unsure" : "closed")
                        .foregroundStyle(e >= 0.5 ? Theme.textSecondary : Theme.reject)
                } else {
                    Text("—").foregroundStyle(Theme.textQuaternary)
                }
            }
            pill {
                Text("Face").foregroundStyle(Theme.textTertiary)
                Text(signals?.faceSharpness.map { "\(Int(($0 * 100).rounded())) %" } ?? "—")
                    .foregroundStyle(signals?.faceSharpness == nil ? Theme.textQuaternary : Theme.textSecondary)
            }
            pill {
                Text("Score").foregroundStyle(Theme.textTertiary)
                Text(score.map { String(format: "%.2f", $0) } ?? "—")
                    .fontWeight(.semibold).foregroundStyle(score == nil ? Theme.textQuaternary : Theme.accent)
            }
        }
        .font(Theme.Fonts.labelNumeric)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("library.cull.burst.signals")
    }

    private func pill<Content: View>(@ViewBuilder _ content: () -> Content) -> some View {
        HStack(spacing: Theme.Space.xs + Theme.Space.xxs) { content() }
            .padding(.horizontal, Theme.Space.s + Theme.Space.xxs)
            .frame(height: Theme.Height.large)
            .background(Capsule().fill(Theme.hud))
            .overlay(Capsule().strokeBorder(Theme.hairlineStrong, lineWidth: Theme.Space.hairline))
    }
}

/// One 172 × 116 cell of the strip: the thumbnail, the focus ring, the pick badge, the decision
/// mark, the name and the score. Click focuses; double-click opens the loupe.
private struct BurstStripCell: View {
    let model: AppModel
    let item: PhotoItem
    let focused: Bool
    let isPick: Bool

    var body: some View {
        let state = model.state(id: item.id)
        let score = model.assist.scores[item.id]?.technical
        Button { model.select(id: item.id) } label: {
            ZStack(alignment: .topLeading) {
                PreviewImageView(model: model, item: item, fill: true)
                    .opacity(state.decision == .reject ? Double(Theme.Opacity.rejectedImage) : 1)
                if state.decision != .undecided {
                    Text(state.decision == .keep ? "P" : "X")
                        .font(Theme.Fonts.captionSemibold).foregroundStyle(Theme.textOnAccent)
                        .frame(width: Theme.Height.chip + Theme.Space.xxs, height: Theme.Height.chip + Theme.Space.xxs)
                        .background(Circle().fill(state.decision == .keep ? Theme.keep : Theme.reject))
                        .padding(Theme.Space.s)
                }
                if isPick {
                    VStack { HStack { Spacer(); Chip(text: "Pick", color: Theme.accent, style: .filled) }; Spacer() }
                        .padding(Theme.Space.s - Theme.Space.xxs)
                }
                VStack {
                    Spacer()
                    HStack {
                        Text(item.name).lineLimit(1).truncationMode(.middle)
                        Spacer()
                        Text(score.map { String(format: "%.2f", $0) } ?? "")
                    }
                    .font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textSecondary)
                    .padding(.horizontal, Theme.Space.s).padding(.vertical, Theme.Space.xs)
                    .background(LinearGradient(colors: [Theme.clear, Theme.shadow], startPoint: .top, endPoint: .bottom))
                }
            }
            .frame(width: Theme.Width.burstCell, height: Theme.Height.burstCell)
            .background(Theme.well)
            .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.chip))
            .overlay(RoundedRectangle(cornerRadius: Theme.Radius.chip)
                .strokeBorder(focused ? Theme.accent : Theme.clear, lineWidth: Theme.Space.xxs))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .simultaneousGesture(TapGesture(count: 2).onEnded { model.showInLoupe(item.id) })
        .help("\(item.name): click to focus, double-click for the loupe (E)")
        .accessibilityLabel("\(item.name), \(state.decision.label)" + (isPick ? ", the pick" : ""))
        .accessibilityAddTraits(focused ? .isSelected : [])
        .accessibilityIdentifier("library.cull.burst.cell.\(item.id)")
    }
}

/// Keep the pick · Keep this one · Reject this one · ⌥← · Next burst ⌥→, key caps on the buttons.
private struct BurstActions: View {
    let model: AppModel
    let single: Bool

    var body: some View {
        HStack(spacing: Theme.Space.s) {
            Button { model.keepBestRejectRest() } label: {
                HStack(spacing: Theme.Space.s) { KeyCap("K"); Text("Keep the pick, reject the rest") }
            }
            .buttonStyle(.theme(.primary, height: Theme.Height.burstAction))
            .disabled(single)
            .help(single ? "A single frame has no burst to decide" : "Keep the agent's pick and reject the other frames of this burst (K), one undo step")
            .accessibilityIdentifier("library.cull.burst.keepPick")
            action("P", "Keep this one", help: "Keep the focused frame (P)") { model.perform(.keep) }
                .accessibilityIdentifier("library.cull.burst.keep")
            action("X", "Reject this one", help: "Reject the focused frame (X)") { model.perform(.reject) }
                .accessibilityIdentifier("library.cull.burst.reject")
            Spacer()
            action("⌥←", nil, help: "Previous burst (⌥←)") { model.moveBurst(-1) }
                .accessibilityLabel("Previous burst")
                .accessibilityIdentifier("library.cull.burst.previous")
            action("⌥→", "Next burst", help: "Next burst (⌥→)", capFirst: false) { model.moveBurst(1) }
                .accessibilityIdentifier("library.cull.burst.next")
        }
        .frame(height: Theme.Height.burstAction)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("library.cull.burst.actions")
    }

    private func action(_ cap: String, _ title: String?, help: String, capFirst: Bool = true, _ act: @escaping () -> Void) -> some View {
        Button(action: act) {
            HStack(spacing: Theme.Space.s) {
                if capFirst { KeyCap(cap) }
                if let title { Text(title) }
                if !capFirst { KeyCap(cap) }
            }
        }
        .buttonStyle(.theme(.bordered, height: Theme.Height.burstAction))
        .help(help)
    }
}

/// A photo in a CALayer (no SwiftUI image conversion): the cached thumbnail at once, then the
/// preview tier when `fill` is off (the big frame), the thumbnail tier for strip cells.
struct PreviewImageView: NSViewRepresentable {
    let model: AppModel
    let item: PhotoItem
    let fill: Bool

    final class Host: NSView {
        private(set) var itemID = -1
        var request: PreviewRequest?
        override init(frame: NSRect) {
            super.init(frame: frame)
            wantsLayer = true
            layer?.contentsGravity = .resizeAspect
            layer?.backgroundColor = Theme.Palette.well.cgColor
        }
        required init?(coder: NSCoder) { fatalError() }
        override func hitTest(_ point: NSPoint) -> NSView? { nil }
        func show(_ item: PhotoItem, model: AppModel, fill: Bool) {
            layer?.contentsGravity = fill ? .resizeAspectFill : .resizeAspect
            guard itemID != item.id else { return }
            itemID = item.id
            request?.cancel()
            let cached = model.loader.cached(item, tier: fill ? .thumbnail : .preview) ?? model.loader.cached(item, tier: .thumbnail)
            layer?.contents = cached
            request = model.loader.request(item, tier: fill ? .thumbnail : .preview, priority: fill ? .high : .veryHigh) { [weak self] image in
                guard let self, self.itemID == item.id else { return }
                self.layer?.contents = image
            }
        }
    }

    func makeNSView(context: Context) -> Host { Host(frame: .zero) }
    func updateNSView(_ view: Host, context: Context) { view.show(item, model: model, fill: fill) }
    static func dismantleNSView(_ view: Host, coordinator: ()) { view.request?.cancel() }
}
