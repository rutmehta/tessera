import AppKit
import SwiftUI
import TesseraCore

/// 2-up compare (docs/06 §3) hosted in SwiftUI. Both panes share one viewport, so zoom and pan
/// stay in sync. Images are the embedded previews as CGImages in CALayers; the Metal loupe keeps
/// the EDR path and will take over once the engine renders tiles.
struct CompareView: NSViewRepresentable {
    let model: AppModel

    func makeNSView(context: Context) -> CompareContainerView {
        let view = CompareContainerView()
        view.onChooseSide = { [weak model] side in model?.setCompareActive(side) }
        return view
    }

    func updateNSView(_ view: CompareContainerView, context: Context) {
        guard let pair = model.compare else { return }
        view.update(pair: pair, model: model)
    }
}

/// Zoom is relative to fit (1 = whole image visible). `center` is the viewport centre in
/// normalised image coordinates, shared by both panes.
@MainActor
final class CompareContainerView: NSView {
    var onChooseSide: ((Int) -> Void)?
    private let panes = [ComparePaneView(), ComparePaneView()]
    private var zoom: CGFloat = 1
    private var center = CGPoint(x: 0.5, y: 0.5)
    private var zoomToggles = 0
    private var shownIDs: [Int] = []
    private var requests: [PreviewRequest?] = [nil, nil]

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layer?.backgroundColor = Theme.gridBackground.cgColor
        panes.forEach(addSubview)
        setAccessibilityElement(true)
        setAccessibilityRole(.group)
        setAccessibilityLabel("Compare")
    }

    required init?(coder: NSCoder) { fatalError() }

    override var isFlipped: Bool { true }

    func update(pair: ComparePair, model: AppModel) {
        for side in 0..<2 {
            let id = pair.ids[side]
            let item = model.item(id: id)
            let pane = panes[side]
            pane.configure(item: item, state: model.state(id: id), suggestedBest: model.isSuggestedBest(item),
                           active: pair.active == side, key: side == 0 ? "←" : "→")
            if shownIDs.count != 2 || shownIDs[side] != id {
                requests[side]?.cancel()
                pane.setImage(model.loader.cached(item, tier: .preview) ?? model.loader.cached(item, tier: .thumbnail))
                requests[side] = model.loader.request(item, tier: .preview, priority: .veryHigh) { [weak pane] image in
                    guard let pane, pane.itemID == id else { return }
                    pane.setImage(image)
                }
            }
        }
        shownIDs = pair.ids
        if pair.zoomToggles != zoomToggles {
            zoomToggles = pair.zoomToggles
            toggleActualSize()
        }
        applyViewport()
    }

    override func layout() {
        super.layout()
        let gap: CGFloat = 6
        let w = (bounds.width - gap * 3) / 2
        for (side, pane) in panes.enumerated() {
            pane.frame = NSRect(x: gap + CGFloat(side) * (w + gap), y: gap, width: w, height: bounds.height - gap * 2)
        }
        applyViewport()
    }

    // MARK: Viewport (synced)

    private func applyViewport() {
        for pane in panes { pane.apply(zoom: zoom, center: center) }
    }

    /// Z: fit ↔ 1:1 device pixels of the left image's preview.
    private func toggleActualSize() {
        if zoom > 1.01 {
            zoom = 1
            center = CGPoint(x: 0.5, y: 0.5)
        } else {
            zoom = max(panes[0].actualSizeZoom(), 2)
        }
    }

    override func scrollWheel(with event: NSEvent) {
        let factor = exp(-event.scrollingDeltaY * (event.hasPreciseScrollingDeltas ? 0.01 : 0.1))
        setZoom(zoom * factor)
    }

    override func magnify(with event: NSEvent) {
        setZoom(zoom * (1 + event.magnification))
    }

    private func setZoom(_ z: CGFloat) {
        zoom = min(max(z, 1), 16)
        if zoom == 1 { center = CGPoint(x: 0.5, y: 0.5) }
        applyViewport()
    }

    override func mouseDown(with event: NSEvent) {
        let p = convert(event.locationInWindow, from: nil)
        if let side = panes.firstIndex(where: { $0.frame.contains(p) }) { onChooseSide?(side) }
    }

    override func mouseDragged(with event: NSEvent) {
        guard zoom > 1, let size = panes.first?.displayedSize, size.width > 0, size.height > 0 else { return }
        center.x = min(max(center.x - event.deltaX / size.width, 0), 1)
        center.y = min(max(center.y - event.deltaY / size.height, 0), 1)
        applyViewport()
    }
}

/// One side: the image layer (clipped), a caption with name and decision, active outline.
@MainActor
final class ComparePaneView: NSView {
    private let imageLayer = CALayer()
    private let clip = CALayer()
    private let caption = NSTextField(labelWithString: "")
    private let badge = NSTextField(labelWithString: "")
    private(set) var itemID = -1
    private var imageSize = CGSize.zero
    private var active = false
    private static let captionHeight: CGFloat = 26

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layer?.cornerRadius = 4
        layer?.backgroundColor = Theme.cellBackground.cgColor
        clip.masksToBounds = true
        imageLayer.contentsGravity = .resize
        imageLayer.minificationFilter = .trilinear
        imageLayer.actions = ["contents": NSNull(), "bounds": NSNull(), "position": NSNull()]
        clip.actions = ["bounds": NSNull(), "position": NSNull()]
        clip.addSublayer(imageLayer)
        layer?.addSublayer(clip)
        caption.font = .systemFont(ofSize: 12, weight: .medium)
        caption.textColor = Theme.textPrimary
        caption.lineBreakMode = .byTruncatingMiddle
        badge.font = .systemFont(ofSize: 10, weight: .bold)
        badge.alignment = .right
        addSubview(caption)
        addSubview(badge)
        setAccessibilityElement(true)
        setAccessibilityRole(.image)
    }

    required init?(coder: NSCoder) { fatalError() }

    override var isFlipped: Bool { true }
    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    func configure(item: PhotoItem, state: CullState, suggestedBest: Bool, active: Bool, key: String) {
        itemID = item.id
        caption.stringValue = "\(key)  \(item.name)"
        var parts: [String] = []
        if let b = state.badgeText { parts.append(b) }
        if suggestedBest { parts.append("SUGGESTED") }
        badge.stringValue = parts.joined(separator: " · ")
        badge.textColor = state.decision == .undecided ? Theme.keep : state.decision.color
        self.active = active
        layer?.borderWidth = active ? 2 : 0
        layer?.borderColor = Theme.accent.cgColor
        imageLayer.opacity = state.decision == .reject ? 0.45 : 1
        setAccessibilityLabel("\(active ? "Active: " : "")\(item.name)\(parts.isEmpty ? "" : ", " + parts.joined(separator: ", "))")
    }

    func setImage(_ image: CGImage?) {
        imageLayer.contents = image
        imageSize = image.map { CGSize(width: $0.width, height: $0.height) } ?? .zero
        relayout()
    }

    private var imageArea: NSRect {
        NSRect(x: 4, y: 4, width: bounds.width - 8, height: bounds.height - 8 - Self.captionHeight)
    }

    private var fitted: CGSize {
        let area = imageArea
        guard imageSize.width > 0, imageSize.height > 0, area.width > 0, area.height > 0 else { return .zero }
        let s = min(area.width / imageSize.width, area.height / imageSize.height)
        return CGSize(width: imageSize.width * s, height: imageSize.height * s)
    }

    private(set) var displayedSize: CGSize = .zero

    /// Zoom (relative to fit) at which one preview pixel covers one device pixel.
    func actualSizeZoom() -> CGFloat {
        let f = fitted
        guard f.width > 0 else { return 1 }
        let scale = window?.backingScaleFactor ?? 2
        return imageSize.width / (f.width * scale)
    }

    private var zoom: CGFloat = 1
    private var center = CGPoint(x: 0.5, y: 0.5)

    func apply(zoom: CGFloat, center: CGPoint) {
        self.zoom = zoom
        self.center = center
        relayout()
    }

    private func relayout() {
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        let area = imageArea
        clip.frame = area
        let f = fitted
        displayedSize = CGSize(width: f.width * zoom, height: f.height * zoom)
        let origin = CGPoint(x: area.width / 2 - center.x * displayedSize.width,
                             y: area.height / 2 - center.y * displayedSize.height)
        // CALayer geometry is not flipped; mirror y so centre.y grows downward like the view.
        imageLayer.frame = CGRect(x: origin.x, y: area.height - origin.y - displayedSize.height,
                                  width: displayedSize.width, height: displayedSize.height)
        CATransaction.commit()
        caption.frame = NSRect(x: 10, y: bounds.height - Self.captionHeight + 4, width: bounds.width * 0.6, height: 18)
        badge.frame = NSRect(x: bounds.width * 0.6, y: bounds.height - Self.captionHeight + 5, width: bounds.width * 0.4 - 10, height: 16)
    }

    override func layout() {
        super.layout()
        relayout()
    }
}
