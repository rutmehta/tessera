import AppKit
import IOSurface
import QuartzCore
import SwiftUI
import TesseraCore
import TesseraFFI

/// 1:1 detail preview for the Detail panel: the engine renders a level-0 crop straight into an
/// IOSurface shown as layer contents (one surface pixel per device pixel). Drag to pan; the
/// target button picks the spot in the loupe.
@MainActor
final class DetailPreviewView: NSView {
    private let imageLayer = CALayer()
    private let label = CATextLayer()
    private var dragLast: CGPoint?
    private var tools: DevelopTools { .shared }

    override var isFlipped: Bool { true }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layer?.backgroundColor = NSColor(calibratedWhite: 0.08, alpha: 1).cgColor
        layer?.cornerRadius = 4
        layer?.masksToBounds = true
        imageLayer.contentsGravity = .topLeft
        imageLayer.magnificationFilter = .nearest
        layer?.addSublayer(imageLayer)
        label.fontSize = 9
        label.foregroundColor = NSColor(calibratedWhite: 0.85, alpha: 1).cgColor
        label.backgroundColor = NSColor(calibratedWhite: 0.05, alpha: 0.7).cgColor
        label.cornerRadius = 3
        label.alignmentMode = .center
        layer?.addSublayer(label)
        setAccessibilityElement(true)
        setAccessibilityRole(.image)
        setAccessibilityLabel("1:1 detail preview")
    }
    required init?(coder: NSCoder) { fatalError() }

    override func layout() {
        super.layout()
        imageLayer.frame = bounds
        label.frame = CGRect(x: 4, y: 4, width: 70, height: 13)
        let scale = window?.backingScaleFactor ?? 2
        imageLayer.contentsScale = scale
        label.contentsScale = scale
        tools.ensureDetailSurface(width: Int(bounds.width * scale), height: Int(bounds.height * scale))
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        tools.detailPreviewVisible = window != nil
        guard window != nil else { return }
        tools.onDetailPreview = { [weak self] surface, info in self?.show(surface, info) }
        needsLayout = true
        tools.requestDetailPreview()
    }

    func show(_ surface: IOSurfaceRef?, _ info: DetailPreview?) {
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        imageLayer.contents = info.flatMap { i in surface.flatMap { Self.image($0, width: Int(i.width), height: Int(i.height)) } }
        label.string = info.map { "1:1 · \($0.x), \($0.y)" } ?? ""
        label.isHidden = info == nil
        CATransaction.commit()
    }

    /// Copies the written region into a CGImage (a few hundred KB; the surface is reused).
    private static func image(_ s: IOSurfaceRef, width: Int, height: Int) -> CGImage? {
        IOSurfaceLock(s, .readOnly, nil)
        defer { IOSurfaceUnlock(s, .readOnly, nil) }
        let stride = IOSurfaceGetBytesPerRow(s)
        let data = Data(bytes: IOSurfaceGetBaseAddress(s), count: stride * height)
        guard let provider = CGDataProvider(data: data as CFData) else { return nil }
        return CGImage(width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: stride,
                       space: CGColorSpace(name: CGColorSpace.sRGB)!,
                       bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
                       provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent)
    }

    override func mouseDown(with event: NSEvent) {
        dragLast = convert(event.locationInWindow, from: nil)
        NSCursor.closedHand.push()
    }

    override func mouseDragged(with event: NSEvent) {
        let p = convert(event.locationInWindow, from: nil)
        guard let last = dragLast else { return }
        let scale = window?.backingScaleFactor ?? 2
        tools.panDetail(dx: (p.x - last.x) * scale, dy: (p.y - last.y) * scale)
        dragLast = p
    }

    override func mouseUp(with event: NSEvent) {
        dragLast = nil
        NSCursor.pop()
    }
}

struct DetailPreview1to1: NSViewRepresentable {
    func makeNSView(context: Context) -> DetailPreviewView { DetailPreviewView(frame: .zero) }
    func updateNSView(_ nsView: DetailPreviewView, context: Context) {}
}
