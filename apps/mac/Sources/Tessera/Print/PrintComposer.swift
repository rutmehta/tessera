import AppKit
import CoreGraphics
import TesseraCore

/// Draws print pages (docs/01 §2.26) into any flipped (y-down, points) CGContext: the print view
/// (printer and PDF), the JPEG page bitmaps and the sheet's preview all use this one routine.
struct PrintComposer {
    var layout: PrintLayout
    var pageSize: CGSize
    /// Picture for image index `i` (an engine render, or a thumbnail in the preview).
    var picture: (Int) -> CGImage?
    var caption: (Int) -> String
    /// Aspect to lay out with when no picture is available yet (preview placeholders).
    var fallbackAspect: (Int) -> Double = { _ in 1.5 }

    var pages: [[Int]] { layout.pages(images: count, page: pageSize) }
    var count: Int

    /// Draws page `page` (0-based). The context must be flipped with points as units.
    func draw(page: Int, in ctx: CGContext, placeholders: Bool = false) {
        let cells = layout.cells(page: pageSize)
        guard pages.indices.contains(page) else { return }
        ctx.saveGState()
        defer { ctx.restoreGState() }
        ctx.setFillColor(CGColor(gray: 1, alpha: 1))
        ctx.fill(CGRect(origin: .zero, size: pageSize))
        for (cell, index) in zip(cells, pages[page]) {
            let area = layout.imageArea(in: cell)
            let image = picture(index)
            let aspect = image.map { Double($0.width) / Double(max($0.height, 1)) } ?? fallbackAspect(index)
            let placement = layout.place(aspect: aspect, in: area)
            if let image {
                Self.draw(image, placement: placement, in: ctx)
            } else if placeholders {
                ctx.setFillColor(CGColor(gray: 0.85, alpha: 1))
                ctx.fill(placement.frame)
            }
            if let band = layout.captionArea(in: cell) {
                drawCaption(caption(index), in: band, ctx: ctx)
            }
        }
    }

    /// Aspect-fit picture, optionally turned 90° clockwise, in a flipped context.
    static func draw(_ image: CGImage, placement: PrintLayout.Placement, in ctx: CGContext) {
        ctx.saveGState()
        ctx.translateBy(x: placement.frame.midX, y: placement.frame.midY)
        if placement.rotated { ctx.rotate(by: .pi / 2) }
        ctx.scaleBy(x: 1, y: -1)   // CGImage rows run bottom-up in user space
        let s = placement.pictureSize
        ctx.interpolationQuality = .high
        ctx.draw(image, in: CGRect(x: -s.width / 2, y: -s.height / 2, width: s.width, height: s.height))
        ctx.restoreGState()
    }

    private func drawCaption(_ text: String, in rect: CGRect, ctx: CGContext) {
        let previous = NSGraphicsContext.current
        NSGraphicsContext.current = NSGraphicsContext(cgContext: ctx, flipped: true)
        defer { NSGraphicsContext.current = previous }
        let style = NSMutableParagraphStyle()
        style.alignment = .center
        style.lineBreakMode = .byTruncatingMiddle
        let attributes: [NSAttributedString.Key: Any] = [
            .font: NSFont.systemFont(ofSize: 7),
            .foregroundColor: NSColor(white: 0.25, alpha: 1),
            .paragraphStyle: style,
        ]
        (text as NSString).draw(in: rect.insetBy(dx: 2, dy: 2), withAttributes: attributes)
    }
}

/// The printable document: one page per `rectForPage`, stacked vertically. NSPrintOperation
/// drives it for the printer and for "Save as PDF".
final class PrintPageView: NSView {
    let composer: PrintComposer

    init(composer: PrintComposer) {
        self.composer = composer
        let pages = max(composer.pages.count, 1)
        super.init(frame: NSRect(x: 0, y: 0, width: composer.pageSize.width,
                                 height: composer.pageSize.height * Double(pages)))
    }

    required init?(coder: NSCoder) { fatalError() }

    override var isFlipped: Bool { true }

    override func knowsPageRange(_ range: NSRangePointer) -> Bool {
        range.pointee = NSRange(location: 1, length: max(composer.pages.count, 1))
        return true
    }

    override func rectForPage(_ page: Int) -> NSRect {
        NSRect(x: 0, y: composer.pageSize.height * Double(page - 1),
               width: composer.pageSize.width, height: composer.pageSize.height)
    }

    override func draw(_ dirtyRect: NSRect) {
        guard let ctx = NSGraphicsContext.current?.cgContext else { return }
        for page in composer.pages.indices {
            let rect = rectForPage(page + 1)
            guard rect.intersects(dirtyRect) else { continue }
            ctx.saveGState()
            ctx.translateBy(x: 0, y: rect.minY)
            composer.draw(page: page, in: ctx)
            ctx.restoreGState()
        }
    }
}
