import AppKit
import TesseraCore

/// Grey channel images from the engine's RGBA8 thumbnail surfaces.
enum ChannelImages {
    struct Gray {
        /// White = selected / full ink.
        let image: CGImage
        /// White = masked (for "colour indicates masked areas").
        let inverted: CGImage
    }

    static func gray(_ s: IOSurfaceRef) -> Gray? {
        let w = IOSurfaceGetWidth(s), h = IOSurfaceGetHeight(s), stride = IOSurfaceGetBytesPerRow(s)
        guard w > 0, h > 0 else { return nil }
        var g = [UInt8](repeating: 0, count: w * h)
        IOSurfaceLock(s, .readOnly, nil)
        let base = IOSurfaceGetBaseAddress(s).assumingMemoryBound(to: UInt8.self)
        for y in 0..<h {
            for x in 0..<w { g[y * w + x] = base[y * stride + x * 4] }
        }
        IOSurfaceUnlock(s, .readOnly, nil)
        guard let a = image(g, w, h), let b = image(g.map { 255 - $0 }, w, h) else { return nil }
        return Gray(image: a, inverted: b)
    }

    static func image(_ g: [UInt8], _ w: Int, _ h: Int) -> CGImage? {
        guard let provider = CGDataProvider(data: Data(g) as CFData) else { return nil }
        return CGImage(width: w, height: h, bitsPerComponent: 8, bitsPerPixel: 8, bytesPerRow: w,
                       space: CGColorSpaceCreateDeviceGray(), bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.none.rawValue),
                       provider: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent)
    }

    /// The composite with hidden components removed; a single visible component as grey (Photoshop's
    /// single-channel view).
    static func components(_ s: IOSurfaceRef, _ v: ComponentVisibility) -> CGImage? {
        let w = IOSurfaceGetWidth(s), h = IOSurfaceGetHeight(s), stride = IOSurfaceGetBytesPerRow(s)
        guard w > 0, h > 0 else { return nil }
        let on = [v.red, v.green, v.blue]
        let single = on.filter { $0 }.count == 1 ? on.firstIndex(of: true) : nil
        var out = [UInt8](repeating: 0, count: w * h * 4)
        IOSurfaceLock(s, .readOnly, nil)
        let base = IOSurfaceGetBaseAddress(s).assumingMemoryBound(to: UInt8.self)
        for y in 0..<h {
            for x in 0..<w {
                let i = y * stride + x * 4, o = (y * w + x) * 4
                for c in 0..<3 { out[o + c] = single.map { base[i + $0] } ?? (on[c] ? base[i + c] : 0) }
                out[o + 3] = base[i + 3]
            }
        }
        IOSurfaceUnlock(s, .readOnly, nil)
        guard let provider = CGDataProvider(data: Data(out) as CFData) else { return nil }
        return CGImage(width: w, height: h, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: w * 4,
                       space: CGColorSpace(name: CGColorSpace.sRGB)!,
                       bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
                       provider: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent)
    }
}

/// Builds what the Channels preview draws over the viewport and installs the overlay view on demand.
@MainActor
final class ChannelOverlayController {
    private var view: ChannelOverlayView?
    private var grays: [String: ChannelImages.Gray] = [:]
    private var composite: (key: String, image: CGImage)?

    func update(_ doc: DocumentController?, channels: DocumentChannels) {
        guard let doc, let viewport = doc.viewport, let b = channels.backend(doc) else { remove(); return }
        let comps = channels.componentVisibility(doc)
        let px = UInt32(min(4096, max(doc.info.width, doc.info.height, 1)))
        var items: [ChannelOverlayView.Item] = []
        var backdrop = comps.none
        var image: CGImage?
        if !comps.all && !comps.none {
            backdrop = true
            let key = "\(doc.id):\(doc.info.epoch):\(comps.red)\(comps.green)\(comps.blue)"
            if composite?.key != key, let sid = try? doc.backend.compositeThumbnail(maxPx: px), let s = IOSurfaceLookup(sid),
               let cg = ChannelImages.components(s, comps) {
                composite = (key, cg)
            }
            image = composite?.image
        }
        var grayShown = false
        for r in channels.records where r.visible {
            let key = "\(doc.id):\(r.id):\(r.revision):\(px)"
            if grays[key] == nil, let sid = try? b.channelThumbnail(id: r.id, maxPx: px), let s = IOSurfaceLookup(sid) {
                if grays.count > 32 { grays.removeAll() }
                grays[key] = ChannelImages.gray(s)
            }
            guard let g = grays[key] else { continue }
            switch r.kind {
            case .spot:
                items.append(.tint(mask: g.image, color: r.color, alpha: r.opacity))
            case .alpha:
                if comps.none && !grayShown {
                    items.append(.gray(g.image))
                    grayShown = true
                } else {
                    let st = channels.style(doc, r.id)
                    items.append(.tint(mask: st.indicatesSelected ? g.image : g.inverted, color: st.color, alpha: st.opacity))
                }
            }
        }
        guard backdrop || !items.isEmpty else { remove(); return }
        let v = view ?? ChannelOverlayView()
        if v.superview !== viewport {
            v.removeFromSuperview()
            v.frame = viewport.bounds
            v.autoresizingMask = [.width, .height]
            viewport.addSubview(v, positioned: .below, relativeTo: viewport.toolOverlay)
        }
        view = v
        v.configure(viewport: viewport, canvas: CGSize(width: Double(doc.info.width), height: Double(doc.info.height)),
                    backdrop: backdrop, composite: image, items: items)
    }

    private func remove() {
        view?.stop()
        view?.removeFromSuperview()
        view = nil
    }
}

/// Draws the Channels preview over the canvas: an ink backdrop when colour components are hidden, the
/// remaining components, then the visible channels (alpha: overlay colour over masked or selected
/// areas; spot: ink at its solidity). Follows pan and zoom by polling the viewport's mapping while shown.
final class ChannelOverlayView: NSView {
    enum Item {
        case gray(CGImage)
        case tint(mask: CGImage, color: ToolColor, alpha: Float)
    }

    private weak var viewport: DocumentViewportView?
    private var canvas = CGSize.zero
    private var backdrop = false
    private var composite: CGImage?
    private var items: [Item] = []
    private var lastRect = CGRect.null
    private var timer: Timer?

    override var isFlipped: Bool { true }
    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    func configure(viewport: DocumentViewportView, canvas: CGSize, backdrop: Bool, composite: CGImage?, items: [Item]) {
        self.viewport = viewport
        self.canvas = canvas
        self.backdrop = backdrop
        self.composite = composite
        self.items = items
        needsDisplay = true
        if timer == nil {
            let t = Timer(timeInterval: 1.0 / 30, repeats: true) { [weak self] _ in
                MainActor.assumeIsolated { self?.follow() }
            }
            RunLoop.main.add(t, forMode: .common)
            timer = t
        }
    }

    func stop() {
        timer?.invalidate()
        timer = nil
    }

    private var canvasRect: CGRect {
        guard let viewport else { return .null }
        let a = viewport.viewPoint(canvas: .zero)
        let b = viewport.viewPoint(canvas: CGPoint(x: canvas.width, y: canvas.height))
        return CGRect(x: a.x, y: a.y, width: b.x - a.x, height: b.y - a.y)
    }

    private func follow() {
        let r = canvasRect
        if r != lastRect { needsDisplay = true }
    }

    override func draw(_ dirtyRect: NSRect) {
        guard let ctx = NSGraphicsContext.current?.cgContext else { return }
        let r = canvasRect
        lastRect = r
        guard !r.isNull, r.width > 0, r.height > 0 else { return }
        ctx.saveGState()
        // Flip so CGImages draw upright in this flipped view.
        ctx.translateBy(x: 0, y: r.minY + r.maxY)
        ctx.scaleBy(x: 1, y: -1)
        ctx.interpolationQuality = .medium
        if backdrop {
            ctx.setFillColor(Theme.Palette.OnImage.ink.cgColor)
            ctx.fill(r)
        }
        if let composite { ctx.draw(composite, in: r) }
        for item in items {
            switch item {
            case .gray(let g):
                ctx.draw(g, in: r)
            case .tint(let mask, let c, let a):
                ctx.saveGState()
                ctx.clip(to: r, mask: mask)
                let color = CGColor(srgbRed: CGFloat(c.r), green: CGFloat(c.g), blue: CGFloat(c.b), alpha: CGFloat(a)) // lint:allow (user-chosen channel colour)
                ctx.setFillColor(color)
                ctx.fill(r)
                ctx.restoreGState()
            }
        }
        ctx.restoreGState()
    }
}
