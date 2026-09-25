import AppKit
import PhotoEditorCore

enum CellStyle {
    case grid, filmstrip

    var captionHeight: CGFloat { self == .grid ? 22 : 0 }
    var imageInset: CGFloat { self == .grid ? 8 : 4 }
}

/// Recycled grid / filmstrip cell. Image in a plain CALayer (GPU-composited, no redraw on scroll);
/// badges and caption in a small overlay view that redraws only when cull state changes.
@MainActor
final class ThumbnailCell: NSCollectionViewItem {
    static let identifier = NSUserInterfaceItemIdentifier("ThumbnailCell")

    private var cellView: ThumbnailCellView { view as! ThumbnailCellView }
    private var request: PreviewRequest?
    private(set) var itemID: Int = -1

    override func loadView() {
        view = ThumbnailCellView(frame: NSRect(x: 0, y: 0, width: 100, height: 100))
    }

    override var isSelected: Bool {
        didSet { if isSelected != oldValue { cellView.isSelectedCell = isSelected } }
    }

    override func prepareForReuse() {
        super.prepareForReuse()
        request?.cancel()
        request = nil
        itemID = -1
        cellView.setImage(nil)
    }

    func configure(item: PhotoItem, state: CullState, groupIndex: Int, groupSize: Int,
                   focused: Bool, style: CellStyle, loader: ThumbnailLoader) {
        let v = cellView
        v.style = style
        v.isFocusedCell = focused
        v.altGroup = item.groupID % 2 == 1
        v.overlay.set(item: item, state: state, groupIndex: groupIndex, groupSize: groupSize, style: style)
        v.imageLayer.opacity = state.decision == .reject ? 0.32 : 1
        v.setAccessibilityLabel("\(item.name), \(state.decision.label)"
            + (state.grade > 0 ? ", grade \(state.grade)" : "")
            + (state.mark > 0 ? ", mark \(state.mark)" : "")
            + (state.inBasket ? ", in basket" : ""))

        guard item.id != itemID else { return }
        itemID = item.id
        request?.cancel()
        v.setImage(nil)
        let id = item.id
        request = loader.request(item, tier: .thumbnail, priority: .high) { [weak self] image in
            guard let self, self.itemID == id else { return }
            self.cellView.setImage(image)
        }
    }

    func update(state: CullState) {
        cellView.imageLayer.opacity = state.decision == .reject ? 0.32 : 1
        cellView.overlay.set(state: state)
    }

    func setFocused(_ f: Bool) { cellView.isFocusedCell = f }
}

@MainActor
final class ThumbnailCellView: NSView {
    let imageLayer = CALayer()
    let overlay = BadgeOverlayView()
    var style: CellStyle = .grid { didSet { if style != oldValue { needsLayout = true } } }
    var isSelectedCell = false { didSet { if isSelectedCell != oldValue { needsDisplay = true } } }
    var isFocusedCell = false { didSet { if isFocusedCell != oldValue { needsDisplay = true } } }
    var altGroup = false { didSet { if altGroup != oldValue { needsDisplay = true } } }

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layerContentsRedrawPolicy = .onSetNeedsDisplay
        layer?.cornerRadius = 3
        imageLayer.contentsGravity = .resizeAspect
        imageLayer.minificationFilter = .trilinear
        imageLayer.actions = ["contents": NSNull(), "opacity": NSNull(), "bounds": NSNull(), "position": NSNull()]
        layer?.addSublayer(imageLayer)
        addSubview(overlay)
        setAccessibilityElement(true)
        setAccessibilityRole(.image)
    }

    required init?(coder: NSCoder) { fatalError() }

    override var wantsUpdateLayer: Bool { true }
    override var isFlipped: Bool { true }

    override func updateLayer() {
        guard let layer else { return }
        let bg: NSColor = isSelectedCell ? Theme.cellSelected : (altGroup ? Theme.cellBackgroundAlt : Theme.cellBackground)
        layer.backgroundColor = bg.cgColor
        layer.borderWidth = isFocusedCell ? 2 : 0
        layer.borderColor = Theme.accent.cgColor
    }

    func setImage(_ image: CGImage?) {
        imageLayer.contents = image
    }

    var imageRect: NSRect {
        let i = style.imageInset
        return NSRect(x: i, y: i, width: bounds.width - 2 * i, height: bounds.height - 2 * i - style.captionHeight)
    }

    override func layout() {
        super.layout()
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        imageLayer.frame = imageRect
        CATransaction.commit()
        overlay.frame = bounds
        overlay.imageRect = imageRect
    }
}

/// Draws decision / grade pill, mark chip, basket chip and the caption. Text only, no symbol fonts.
@MainActor
final class BadgeOverlayView: NSView {
    private var name = ""
    private var groupText = ""
    private var state = CullState()
    private var style: CellStyle = .grid
    var imageRect: NSRect = .zero { didSet { if imageRect != oldValue { needsDisplay = true } } }

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layerContentsRedrawPolicy = .onSetNeedsDisplay
    }

    required init?(coder: NSCoder) { fatalError() }

    override var isFlipped: Bool { true }
    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    func set(item: PhotoItem, state: CullState, groupIndex: Int, groupSize: Int, style: CellStyle) {
        let g = "G\(item.groupID + 1)" + (groupSize > 1 ? " · \(groupIndex + 1)/\(groupSize)" : "")
        if name != item.name || groupText != g || self.state != state || self.style != style {
            name = item.name
            groupText = g
            self.state = state
            self.style = style
            needsDisplay = true
        }
    }

    func set(state: CullState) {
        if self.state != state { self.state = state; needsDisplay = true }
    }

    private static let pillFont = NSFont.systemFont(ofSize: 9.5, weight: .bold)
    private static let smallPillFont = NSFont.systemFont(ofSize: 8, weight: .bold)
    private static let captionFont = NSFont.systemFont(ofSize: 10.5, weight: .regular)
    private static let captionFontMono = NSFont.monospacedDigitSystemFont(ofSize: 10, weight: .regular)

    override func draw(_ dirtyRect: NSRect) {
        let small = style == .filmstrip
        let font = small ? Self.smallPillFont : Self.pillFont
        let pad: CGFloat = small ? 3 : 5
        let r = imageRect

        // Decision / grade (top-left)
        if let text = state.badgeText {
            let tw = small && state.decision == .keep ? (state.grade > 0 ? "\(state.grade)" : "K") : (small ? "X" : text)
            drawPill(tw, color: state.decision.color, font: font, at: NSPoint(x: r.minX + pad, y: r.minY + pad), textColor: .black)
        }
        // Mark (top-right): coloured chip with the key number
        if state.mark != 0 {
            let s = "\(state.mark)"
            let size = pillSize(s, font: font)
            drawPill(s, color: MarkStyle.color(state.mark), font: font,
                     at: NSPoint(x: r.maxX - pad - size.width, y: r.minY + pad), textColor: .black)
        }
        // Basket (bottom-left)
        if state.inBasket {
            let s = small ? "B" : "BASKET"
            let size = pillSize(s, font: font)
            drawPill(s, color: Theme.basket, font: font, at: NSPoint(x: r.minX + pad, y: r.maxY - pad - size.height), textColor: .black)
        }
        // Caption
        if style == .grid {
            let y = bounds.height - style.captionHeight + 2
            let para = NSMutableParagraphStyle()
            para.lineBreakMode = .byTruncatingMiddle
            let groupAttrs: [NSAttributedString.Key: Any] = [.font: Self.captionFontMono, .foregroundColor: Theme.textSecondary]
            let gw = (groupText as NSString).size(withAttributes: groupAttrs).width
            (groupText as NSString).draw(at: NSPoint(x: bounds.width - 8 - gw, y: y + 1), withAttributes: groupAttrs)
            let nameAttrs: [NSAttributedString.Key: Any] = [.font: Self.captionFont, .foregroundColor: Theme.textPrimary, .paragraphStyle: para]
            (name as NSString).draw(in: NSRect(x: 8, y: y, width: max(bounds.width - 24 - gw, 10), height: 16), withAttributes: nameAttrs)
        }
    }

    private func pillSize(_ text: String, font: NSFont) -> NSSize {
        let s = (text as NSString).size(withAttributes: [.font: font])
        return NSSize(width: ceil(s.width) + (style == .filmstrip ? 6 : 10), height: ceil(s.height) + 2)
    }

    private func drawPill(_ text: String, color: NSColor, font: NSFont, at origin: NSPoint, textColor: NSColor) {
        let size = pillSize(text, font: font)
        let rect = NSRect(origin: origin, size: size)
        color.setFill()
        NSBezierPath(roundedRect: rect, xRadius: 3, yRadius: 3).fill()
        let attrs: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: textColor.withAlphaComponent(0.85)]
        let ts = (text as NSString).size(withAttributes: attrs)
        (text as NSString).draw(at: NSPoint(x: rect.midX - ts.width / 2, y: rect.midY - ts.height / 2), withAttributes: attrs)
    }
}
