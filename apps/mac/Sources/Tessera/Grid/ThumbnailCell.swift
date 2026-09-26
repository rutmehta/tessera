import AppKit
import TesseraCore

enum CellStyle {
    case grid, filmstrip

    var captionHeight: CGFloat { self == .grid ? Theme.Height.small : 0 }
    var imageInset: CGFloat { self == .grid ? Theme.Space.s - Theme.Space.xxs : Theme.Space.xxs }
}

/// Recycled grid / filmstrip cell. Image in a plain CALayer (GPU-composited, no redraw on scroll);
/// badges and caption in a small overlay view that redraws only when cull state changes.
@MainActor
final class ThumbnailCell: NSCollectionViewItem {
    static let identifier = NSUserInterfaceItemIdentifier("ThumbnailCell")

    private var cellView: ThumbnailCellView { view as! ThumbnailCellView }
    private var request: PreviewRequest?
    private var representedItem: PhotoItem?
    private(set) var itemID: Int = -1

    deinit { request?.cancel() }

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
        representedItem = nil
        cellView.setImage(nil)
    }

    func configure(item: PhotoItem, state: CullState, status: ItemStatus, basketTarget: String,
                   suggestedBest: Bool, groupIndex: Int, groupSize: Int,
                   focused: Bool, style: CellStyle, loader: ThumbnailLoader, suggestion: Decision? = nil) {
        let v = cellView
        v.style = style
        v.isFocusedCell = focused
        v.altGroup = item.groupID % 2 == 1
        v.overlay.set(item: item, state: state, status: status, basketTarget: basketTarget,
                      suggestedBest: suggestedBest, groupIndex: groupIndex, groupSize: groupSize, style: style)
        v.overlay.set(suggestion: suggestion)
        v.imageLayer.opacity = state.decision == .reject ? Theme.Opacity.rejectedImage : 1
        name = item.name
        self.suggestedBest = suggestedBest
        self.suggestion = suggestion
        updateAccessibility(state: state, status: status, basketTarget: basketTarget)

        guard item != representedItem else { return }
        representedItem = item
        itemID = item.id
        request?.cancel()
        v.setImage(nil)
        request = loader.request(item, tier: .thumbnail, priority: .high) { [weak self] image in
            guard let self, self.representedItem == item else { return }
            self.cellView.setImage(image)
        }
    }

    /// The item's preview changed (a saved develop edit): fetch it again, keeping the old image
    /// on screen until the new one arrives.
    func refreshThumbnail(loader: ThumbnailLoader) {
        guard let item = representedItem else { return }
        request?.cancel()
        request = loader.request(item, tier: .thumbnail, priority: .high) { [weak self] image in
            guard let self, self.representedItem == item else { return }
            self.cellView.setImage(image)
        }
    }

    func update(state: CullState, status: ItemStatus, basketTarget: String, suggestion: Decision? = nil) {
        cellView.imageLayer.opacity = state.decision == .reject ? Theme.Opacity.rejectedImage : 1
        cellView.overlay.set(state: state, status: status, basketTarget: basketTarget)
        cellView.overlay.set(suggestion: suggestion)
        self.suggestion = suggestion
        updateAccessibility(state: state, status: status, basketTarget: basketTarget)
    }

    private var name = ""
    private var suggestedBest = false
    private var suggestion: Decision?

    private func updateAccessibility(state: CullState, status: ItemStatus, basketTarget: String) {
        cellView.setAccessibilityLabel("\(name), \(state.decision.label)"
            + (state.grade > 0 ? ", grade \(state.grade)" : "")
            + (state.mark > 0 ? ", mark \(state.mark)" : "")
            + (suggestedBest ? ", suggested best" : "")
            + (suggestion.map { ", suggested \($0.label.lowercased())" } ?? "")
            + ", \(status.phase.rawValue)"
            + (status.albums.isEmpty ? "" : ", in " + status.albums.joined(separator: ", ")))
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
        layer?.cornerRadius = Theme.Radius.control
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

    /// No tile: the photo sits on the canvas. Selection is a subtle accent fill, focus a 2 px
    /// accent ring; alternate burst groups get a faint fill so group boundaries read.
    override func updateLayer() {
        guard let layer else { return }
        let bg: NSColor = isSelectedCell ? Theme.Palette.accentSubtle : (altGroup ? Theme.Palette.groupAlt : .clear)
        layer.backgroundColor = bg.cgColor(for: self)
        layer.borderWidth = isFocusedCell ? Theme.Space.xxs : 0
        layer.borderColor = Theme.Palette.accent.cgColor(for: self)
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        needsDisplay = true
        overlay.needsDisplay = true
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
    private var status = ItemStatus()
    private var basketTarget = ""
    private var suggestedBest = false
    /// Assist's pre-filled decision (a translucent, outlined pill until confirmed).
    private var suggestion: Decision?
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

    func set(item: PhotoItem, state: CullState, status: ItemStatus, basketTarget: String,
             suggestedBest: Bool, groupIndex: Int, groupSize: Int, style: CellStyle) {
        let g = "G\(item.groupID + 1)" + (groupSize > 1 ? " · \(groupIndex + 1)/\(groupSize)" : "")
        if name != item.name || groupText != g || self.style != style || self.suggestedBest != suggestedBest {
            name = item.name
            groupText = g
            self.style = style
            self.suggestedBest = suggestedBest
            needsDisplay = true
        }
        set(state: state, status: status, basketTarget: basketTarget)
    }

    func set(suggestion: Decision?) {
        if self.suggestion != suggestion {
            self.suggestion = suggestion
            needsDisplay = true
        }
    }

    func set(state: CullState, status: ItemStatus, basketTarget: String) {
        if self.state != state || self.status != status || self.basketTarget != basketTarget {
            self.state = state
            self.status = status
            self.basketTarget = basketTarget
            needsDisplay = true
        }
    }

    /// Bottom-right status text: derived phase (unedited is implicit) and albums other than the
    /// basket target, whose membership already shows as the basket chip.
    private var statusText: String? {
        var parts: [String] = []
        if status.phase != .unedited { parts.append(status.phase.rawValue.capitalized) }
        let others = status.albums.filter { $0 != basketTarget }
        if others.count == 1 { parts.append("In " + others[0]) }
        else if others.count > 1 { parts.append("In \(others.count) albums") }
        return parts.isEmpty ? nil : parts.joined(separator: " · ")
    }

    private typealias OnImage = Theme.Palette.OnImage

    /// Chips (DESIGN.md §4.4): one family, 16 pt tall, radius 4, 11 pt medium, sentence case.
    /// Filled = a fact about the photo (decision, mark, basket); outlined over a scrim = a hint
    /// (suggested best) or derived status. The filmstrip shows single-glyph chips only.
    override func draw(_ dirtyRect: NSRect) {
        let small = style == .filmstrip
        let pad = small ? Theme.Space.xxs : Theme.Space.xs
        let gap = Theme.Space.xs
        let r = imageRect

        // Decision / grade (top-left), then the group's suggested best.
        var x = r.minX + pad
        if let text = state.badgeText {
            let t = small ? (state.decision == .keep ? (state.grade > 0 ? "\(state.grade)" : "K") : "X") : text
            x += drawChip(t, fill: state.decision.chipColor, text: OnImage.ink,
                          at: NSPoint(x: x, y: r.minY + pad), maxWidth: r.width / 2).width + gap
        }
        // Assist's pre-filled decision: an outlined hint over the scrim until Y confirms it.
        if let suggestion, state.decision == .undecided {
            let color = suggestion == .keep ? OnImage.keep : OnImage.reject
            let text = small ? (suggestion == .keep ? "K?" : "X?") : (suggestion == .keep ? "Keep?" : "Reject?")
            x += drawChip(text, fill: OnImage.scrim, text: color, outline: color,
                          at: NSPoint(x: x, y: r.minY + pad), maxWidth: r.width / 2).width + gap
        }
        if suggestedBest, !small {
            drawChip("Suggested", fill: OnImage.scrim, text: OnImage.keep, outline: OnImage.keep,
                     at: NSPoint(x: x, y: r.minY + pad), maxWidth: r.maxX - x - pad - Theme.Height.chip - gap)
        }
        // Mark (top-right): a square chip in the mark colour with its key.
        if state.mark != 0 {
            let size = chipSize("\(state.mark)", square: true)
            drawChip("\(state.mark)", fill: MarkStyle.color(state.mark), text: OnImage.ink,
                     at: NSPoint(x: r.maxX - pad - size.width, y: r.minY + pad), square: true)
        }
        // Basket target membership (bottom-left): the album's name.
        let bottom = r.maxY - pad - Theme.Height.chip
        var basketWidth: CGFloat = 0
        if state.inBasket {
            basketWidth = drawChip(small ? "B" : basketTarget, fill: OnImage.basket, text: OnImage.ink,
                                   at: NSPoint(x: r.minX + pad, y: bottom), maxWidth: r.width * 0.55).width
        }
        // Derived status (bottom-right).
        if !small, let s = statusText {
            let maxW = r.width - 2 * pad - basketWidth - gap
            let size = chipSize(s, maxWidth: maxW)
            drawChip(s, fill: OnImage.scrim, text: OnImage.text, at: NSPoint(x: r.maxX - pad - size.width, y: bottom), maxWidth: maxW)
        }
        // Caption: name (secondary) and group (tertiary, tabular) on one baseline.
        if style == .grid {
            let para = NSMutableParagraphStyle()
            para.lineBreakMode = .byTruncatingMiddle
            let groupAttrs: [NSAttributedString.Key: Any] = [.font: Theme.NSFonts.captionNumeric,
                                                             .foregroundColor: Theme.Palette.textTertiary]
            let nameAttrs: [NSAttributedString.Key: Any] = [.font: Theme.NSFonts.caption,
                                                            .foregroundColor: Theme.Palette.textSecondary, .paragraphStyle: para]
            let font = Theme.NSFonts.caption
            let lineHeight = ceil(font.ascender - font.descender)
            let y = r.maxY + floor((style.captionHeight - lineHeight) / 2)
            let gw = ceil((groupText as NSString).size(withAttributes: groupAttrs).width)
            (groupText as NSString).draw(at: NSPoint(x: r.maxX - gw, y: y), withAttributes: groupAttrs)
            (name as NSString).draw(in: NSRect(x: r.minX, y: y, width: max(r.width - gw - Theme.Space.s, 10), height: lineHeight),
                                    withAttributes: nameAttrs)
        }
    }

    private static let chipFont = Theme.NSFonts.captionMedium

    private func chipSize(_ text: String, square: Bool = false, maxWidth: CGFloat = .greatestFiniteMagnitude) -> NSSize {
        let h = Theme.Height.chip
        if square || text.count == 1 { return NSSize(width: h, height: h) }
        let w = ceil((text as NSString).size(withAttributes: [.font: Self.chipFont]).width) + 2 * (Theme.Space.s - Theme.Space.xxs)
        return NSSize(width: min(w, maxWidth), height: h)
    }

    @discardableResult
    private func drawChip(_ text: String, fill: NSColor, text color: NSColor, outline: NSColor? = nil,
                          at origin: NSPoint, square: Bool = false, maxWidth: CGFloat = .greatestFiniteMagnitude) -> NSSize {
        let size = chipSize(text, square: square, maxWidth: maxWidth)
        guard size.width >= Theme.Height.chip else { return .zero }
        let rect = NSRect(origin: origin, size: size)
        let path = NSBezierPath(roundedRect: rect.insetBy(dx: 0.5, dy: 0.5), xRadius: Theme.Radius.chip, yRadius: Theme.Radius.chip)
        fill.setFill()
        path.fill()
        if let outline {
            outline.withAlphaComponent(0.7).setStroke()
            path.lineWidth = Theme.Space.hairline
            path.stroke()
        }
        let para = NSMutableParagraphStyle()
        para.alignment = .center
        para.lineBreakMode = .byTruncatingTail
        let attrs: [NSAttributedString.Key: Any] = [.font: Self.chipFont, .foregroundColor: color, .paragraphStyle: para]
        let font = Self.chipFont
        let lineHeight = ceil(font.ascender - font.descender)
        let inset = square || text.count == 1 ? 0 : Theme.Space.xs
        (text as NSString).draw(in: NSRect(x: rect.minX + inset, y: rect.midY - lineHeight / 2,
                                           width: rect.width - 2 * inset, height: lineHeight), withAttributes: attrs)
        return size
    }
}
