import AppKit
import TesseraCore

/// How the crop tool shows the image: rotated about the crop so the crop box is axis-aligned on
/// screen. `imageCenter` (displayed-image pixels) appears at `screenCenter` (view points, y-down),
/// scaled by `scale` points per image pixel and turned by `angle` (clockwise, degrees).
struct CropView: Equatable {
    var imageCenter: (x: Double, y: Double)
    var screenCenter: CGPoint
    var scale: Double
    var angle: Double
    var imageSize: (width: Double, height: Double)
    /// Bright rectangle (the crop box) in view points.
    var keep: CGRect

    static func == (a: CropView, b: CropView) -> Bool {
        a.imageCenter == b.imageCenter && a.screenCenter == b.screenCenter && a.scale == b.scale
            && a.angle == b.angle && a.keep == b.keep
    }

    /// Fits crop `g` into `bounds` (points) with room for the handles.
    static func fit(_ g: CropGeometry, in bounds: CGRect) -> CropView {
        let k = min(bounds.width * 0.8 / g.cropWidth, bounds.height * 0.8 / g.cropHeight)
        var v = CropView(imageCenter: (g.centerX, g.centerY), screenCenter: CGPoint(x: bounds.midX, y: bounds.midY),
                         scale: k, angle: g.angle, imageSize: (g.width, g.height), keep: .zero)
        v.keep = v.box(g)
        return v
    }

    func screen(_ x: Double, _ y: Double) -> CGPoint {
        let r = angle * .pi / 180, (dx, dy) = (x - imageCenter.x, y - imageCenter.y)
        return CGPoint(x: screenCenter.x + scale * (cos(r) * dx - sin(r) * dy),
                       y: screenCenter.y + scale * (sin(r) * dx + cos(r) * dy))
    }

    func image(_ p: CGPoint) -> (x: Double, y: Double) {
        let r = angle * .pi / 180
        let (qx, qy) = ((p.x - screenCenter.x) / scale, (p.y - screenCenter.y) / scale)
        return (imageCenter.x + cos(r) * qx + sin(r) * qy, imageCenter.y - sin(r) * qx + cos(r) * qy)
    }

    /// The crop box of `g` on screen (axis-aligned while `angle == g.angle`).
    func box(_ g: CropGeometry) -> CGRect {
        let pts = g.corners.map { screen($0.x, $0.y) }
        let xs = pts.map(\.x), ys = pts.map(\.y)
        return CGRect(x: xs.min()!, y: ys.min()!, width: xs.max()! - xs.min()!, height: ys.max()! - ys.min()!)
    }

    /// The renderer's drawable-pixel affine map for this view.
    func placement(scale s: Double) -> LoupePlacement {
        let r = angle * .pi / 180, (c, n) = (cos(r), sin(r))
        let (bx, by, k) = (Double(screenCenter.x), Double(screenCenter.y), scale)
        let (w, h) = imageSize
        return LoupePlacement(
            row0: SIMD3(c / (k * s * w), n / (k * s * w), (imageCenter.x - (c * bx + n * by) / k) / w),
            row1: SIMD3(-n / (k * s * h), c / (k * s * h), (imageCenter.y - (-n * bx + c * by) / k) / h),
            pixelsPerImageWidth: k * s * w,
            keep: CGRect(x: keep.minX * s, y: keep.minY * s, width: keep.width * s, height: keep.height * s))
    }
}

/// Interaction layer over the loupe image for the develop tools. Transparent to the mouse unless
/// a tool is armed: the crop & straighten tool, the HSL targeted adjustment or the detail target.
@MainActor
final class LoupeToolOverlay: NSView {
    weak var loupe: MetalLoupeView?
    var tools: DevelopTools { .shared }

    private enum Drag {
        case resize(sx: Int, sy: Int)
        case move
        case rotate(startAngle: Double, startPointer: Double)
        case straighten(from: CGPoint)
        case targeted(TargetedHSLAdjustment, CGPoint)
    }
    private var drag: Drag?
    private var dragStart: CGPoint = .zero
    private var startGeometry: CropGeometry?
    /// View frozen during resize/move drags (the box moves, the image stays).
    private var frozen: CropView?
    private var straightenLine: (CGPoint, CGPoint)?
    /// Keep the crop presentation until the first cropped frame after leaving the tool.
    private var awaitingCroppedFrame = false
    private var tracking: NSTrackingArea?

    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { false }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layer?.backgroundColor = .clear
    }
    required init?(coder: NSCoder) { fatalError() }

    private var armed: Bool { tools.cropActive || tools.hslPicker != nil || tools.detailPicking || masks.active }
    var masks: MaskTools { .shared }
    /// Mask gesture in progress and the last pointer position (brush cursor).
    var maskDrag: MaskDrag?
    var pointer: CGPoint?

    override func hitTest(_ point: NSPoint) -> NSView? { armed ? super.hitTest(point) : nil }

    // MARK: State from the tools

    /// Tool state changed (crop begun/ended/edited, picker armed).
    func toolsChanged() {
        if tools.cropActive, let g = tools.crop {
            loupe?.cropView = currentView(g)
        } else if loupe?.cropView != nil {
            awaitingCroppedFrame = true
        }
        window?.invalidateCursorRects(for: self)
        needsDisplay = true
    }

    func frameDidArrive() {
        if awaitingCroppedFrame, !tools.cropActive {
            awaitingCroppedFrame = false
            loupe?.cropView = nil
            loupe?.cropDidChange()
        }
    }

    private func currentView(_ g: CropGeometry) -> CropView {
        guard var v = frozen else { return .fit(g, in: bounds) }
        v.keep = v.box(g)
        return v
    }

    override func layout() {
        super.layout()
        if tools.cropActive, let g = tools.crop, frozen == nil { loupe?.cropView = .fit(g, in: bounds) }
    }

    // MARK: Drawing

    override func draw(_ dirtyRect: NSRect) {
        if masks.active, !tools.cropActive {
            drawMasks()
        } else if tools.cropActive, let g = tools.crop, let view = loupe?.cropView {
            drawCrop(g, view)
        } else if let p = tools.hslPicker {
            hint("Drag up or down on a colour in the photo to adjust its \(p.title.lowercased()) · Esc to finish")
        } else if tools.detailPicking {
            hint("Click the photo to choose the 1:1 detail area")
        }
    }

    func hint(_ text: String) {
        let font = Theme.NSFonts.captionMedium
        let attrs: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: Theme.Palette.OnImage.text]
        let str = text as NSString
        let size = str.size(withAttributes: attrs)
        let h = Theme.Height.large
        // Bottom centre, above the shortcut line: the top is the mask toolbar's.
        let r = NSRect(x: bounds.midX - size.width / 2 - Theme.Space.m, y: bounds.height - h - Theme.Space.xxl - Theme.Space.s,
                       width: ceil(size.width) + 2 * Theme.Space.m, height: h)
        Theme.Palette.OnImage.scrim.setFill()
        NSBezierPath(roundedRect: r, xRadius: Theme.Radius.card, yRadius: Theme.Radius.card).fill()
        str.draw(at: NSPoint(x: r.minX + Theme.Space.m, y: r.midY - ceil(font.ascender - font.descender) / 2), withAttributes: attrs)
    }

    private func drawCrop(_ g: CropGeometry, _ view: CropView) {
        let box = view.box(g)
        // Composition guides.
        let guide = NSBezierPath()
        let line = { (a: CGPoint, b: CGPoint) in guide.move(to: a); guide.line(to: b) }
        let fx = { (f: Double) in box.minX + box.width * f }, fy = { (f: Double) in box.minY + box.height * f }
        switch tools.cropOverlay {
        case .thirds:
            for f in [1.0 / 3, 2.0 / 3] {
                line(CGPoint(x: fx(f), y: box.minY), CGPoint(x: fx(f), y: box.maxY))
                line(CGPoint(x: box.minX, y: fy(f)), CGPoint(x: box.maxX, y: fy(f)))
            }
        case .grid:
            for i in 1..<8 {
                let f = Double(i) / 8
                line(CGPoint(x: fx(f), y: box.minY), CGPoint(x: fx(f), y: box.maxY))
                line(CGPoint(x: box.minX, y: fy(f)), CGPoint(x: box.maxX, y: fy(f)))
            }
        case .goldenRatio:
            for f in [0.382, 0.618] {
                line(CGPoint(x: fx(f), y: box.minY), CGPoint(x: fx(f), y: box.maxY))
                line(CGPoint(x: box.minX, y: fy(f)), CGPoint(x: box.maxX, y: fy(f)))
            }
        case .diagonals:
            let s = min(box.width, box.height)
            line(CGPoint(x: box.minX, y: box.minY), CGPoint(x: box.minX + s, y: box.minY + s))
            line(CGPoint(x: box.maxX, y: box.minY), CGPoint(x: box.maxX - s, y: box.minY + s))
            line(CGPoint(x: box.minX, y: box.maxY), CGPoint(x: box.minX + s, y: box.maxY - s))
            line(CGPoint(x: box.maxX, y: box.maxY), CGPoint(x: box.maxX - s, y: box.maxY - s))
        case .none: break
        }
        Theme.Palette.OnImage.guideFaint.setStroke()
        guide.lineWidth = 0.5
        guide.stroke()
        // While rotating, a fine grid helps levelling.
        if case .rotate = drag {
            let fine = NSBezierPath()
            for i in 1..<16 {
                let f = Double(i) / 16
                fine.move(to: CGPoint(x: fx(f), y: box.minY)); fine.line(to: CGPoint(x: fx(f), y: box.maxY))
                fine.move(to: CGPoint(x: box.minX, y: fy(f))); fine.line(to: CGPoint(x: box.maxX, y: fy(f)))
            }
            Theme.Palette.OnImage.guideFaint.withAlphaComponent(0.18).setStroke()
            fine.lineWidth = 0.5
            fine.stroke()
        }
        // Frame and handles.
        Theme.Palette.OnImage.guide.setStroke()
        let frame = NSBezierPath(rect: box.insetBy(dx: 0.5, dy: 0.5))
        frame.lineWidth = 1
        frame.stroke()
        Theme.Palette.OnImage.guide.setFill()
        for (sx, sy) in Self.handles {
            let c = handlePoint(box, sx, sy)
            let r = (sx != 0 && sy != 0)
                ? NSRect(x: c.x - 3, y: c.y - 3, width: 6, height: 6)
                : (sx == 0 ? NSRect(x: c.x - 9, y: c.y - 1.5, width: 18, height: 3)
                           : NSRect(x: c.x - 1.5, y: c.y - 9, width: 3, height: 18))
            NSBezierPath(roundedRect: r, xRadius: 1, yRadius: 1).fill()
        }
        if let (a, b) = straightenLine {
            let p = NSBezierPath()
            p.move(to: a); p.line(to: b)
            p.lineWidth = 1.5
            Theme.Palette.accent.setStroke()
            p.stroke()
        }
        var info = String(format: "%.0f × %.0f", g.cropWidth, g.cropHeight)
        if g.angle != 0 || drag.map({ if case .rotate = $0 { true } else { false } }) == true {
            info += String(format: "   %+.2f°", g.angle)
        }
        // Dimensions in a scrim chip under the box (legible over either canvas appearance).
        let font = Theme.NSFonts.captionNumericMedium
        let attrs: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: Theme.Palette.OnImage.text]
        let tw = ceil((info as NSString).size(withAttributes: attrs).width)
        let chip = NSRect(x: box.minX, y: box.maxY + Theme.Space.s - Theme.Space.xxs, width: tw + 2 * Theme.Space.s - 2 * Theme.Space.xxs,
                          height: Theme.Height.small)
        Theme.Palette.OnImage.scrim.setFill()
        NSBezierPath(roundedRect: chip, xRadius: Theme.Radius.chip, yRadius: Theme.Radius.chip).fill()
        (info as NSString).draw(at: NSPoint(x: chip.minX + Theme.Space.s - Theme.Space.xxs,
                                            y: chip.midY - ceil(font.ascender - font.descender) / 2), withAttributes: attrs)
    }

    private static let handles: [(Int, Int)] = [(-1, -1), (0, -1), (1, -1), (1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0)]

    private func handlePoint(_ box: CGRect, _ sx: Int, _ sy: Int) -> CGPoint {
        CGPoint(x: sx < 0 ? box.minX : sx > 0 ? box.maxX : box.midX, y: sy < 0 ? box.minY : sy > 0 ? box.maxY : box.midY)
    }

    private func handle(at p: CGPoint, box: CGRect) -> (Int, Int)? {
        Self.handles.first { hypot(handlePoint(box, $0.0, $0.1).x - p.x, handlePoint(box, $0.0, $0.1).y - p.y) < 10 }
            ?? {
                // Edges anywhere along their length.
                let near = { (a: CGFloat, b: CGFloat) in abs(a - b) < 6 }
                if near(p.x, box.minX), (box.minY...box.maxY).contains(p.y) { return (-1, 0) }
                if near(p.x, box.maxX), (box.minY...box.maxY).contains(p.y) { return (1, 0) }
                if near(p.y, box.minY), (box.minX...box.maxX).contains(p.x) { return (0, -1) }
                if near(p.y, box.maxY), (box.minX...box.maxX).contains(p.x) { return (0, 1) }
                return nil
            }()
    }

    // MARK: Cursors

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let tracking { removeTrackingArea(tracking) }
        let t = NSTrackingArea(rect: .zero, options: [.mouseMoved, .mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect, .cursorUpdate],
                               owner: self, userInfo: nil)
        addTrackingArea(t)
        tracking = t
    }

    override func cursorUpdate(with event: NSEvent) { updateCursor(convert(event.locationInWindow, from: nil)) }
    override func mouseMoved(with event: NSEvent) {
        let p = convert(event.locationInWindow, from: nil)
        if masks.active { pointer = p; needsDisplay = true }
        updateCursor(p)
    }
    override func mouseExited(with event: NSEvent) {
        pointer = nil
        needsDisplay = true
    }

    func updateCursor(_ p: CGPoint) {
        guard armed else { return }
        if masks.active, !tools.cropActive { maskCursor(p); return }
        if tools.hslPicker != nil || tools.detailPicking || tools.straightening { NSCursor.crosshair.set(); return }
        guard let g = tools.crop, let view = loupe?.cropView else { return }
        let box = view.box(g)
        if let (sx, sy) = handle(at: p, box: box) {
            if sx == 0 { NSCursor.resizeUpDown.set() } else if sy == 0 { NSCursor.resizeLeftRight.set() } else { NSCursor.crosshair.set() }
        } else if box.contains(p) {
            NSCursor.openHand.set()
        } else {
            NSCursor.pointingHand.set()   // outside the box: drag to rotate
        }
    }

    // MARK: Mouse

    override func mouseDown(with event: NSEvent) {
        let p = convert(event.locationInWindow, from: nil)
        dragStart = p
        if masks.active, !tools.cropActive { maskMouseDown(p, event); return }
        if let _ = tools.hslPicker {
            guard let rgb = loupe?.sampleColor(at: p), let t = tools.beginTargetedHSL(sample: rgb) else {
                tools.model.statusMessage = "No colour there to adjust (neutral or outside the photo)"
                return
            }
            drag = .targeted(t, p)
            NSCursor.resizeUpDown.set()
            return
        }
        if tools.detailPicking {
            if let loc = loupe?.pictureLocation(p) { tools.pickDetail(u: loc.u, v: loc.v) }
            toolsChanged()
            return
        }
        guard tools.cropActive, let g = tools.crop, let view = loupe?.cropView else { return }
        startGeometry = g
        if event.clickCount == 2, view.box(g).contains(p) {
            tools.commitCrop()
            return
        }
        if tools.straightening {
            drag = .straighten(from: p)
            return
        }
        let box = view.box(g)
        if let (sx, sy) = handle(at: p, box: box) {
            drag = .resize(sx: sx, sy: sy)
            frozen = view
        } else if box.contains(p) {
            drag = .move
            frozen = view
            NSCursor.closedHand.set()
        } else {
            drag = .rotate(startAngle: g.angle, startPointer: pointerAngle(p, view))
        }
    }

    private func pointerAngle(_ p: CGPoint, _ view: CropView) -> Double {
        atan2(p.y - view.screenCenter.y, p.x - view.screenCenter.x) * 180 / .pi
    }

    override func mouseDragged(with event: NSEvent) {
        let p = convert(event.locationInWindow, from: nil)
        if maskDrag != nil { maskMouseDragged(p, event); return }
        let fine = event.modifierFlags.contains(.option) ? 0.25 : 1.0
        switch drag {
        case .targeted(let t, let start):
            tools.dragTargetedHSL(t, delta: ((start.y - p.y) * 0.5 * fine).rounded(), final: false)
        case .resize(let sx, let sy):
            guard var g = startGeometry, let view = frozen else { return }
            let aspect = tools.cropAspect.value(imageAspect: g.width / g.height, portrait: tools.cropPortrait)
                ?? (event.modifierFlags.contains(.shift) ? g.aspect : nil)
            g.resize(sx: sx, sy: sy, dx: (p.x - dragStart.x) / view.scale, dy: (p.y - dragStart.y) / view.scale,
                     aspect: aspect, constrain: tools.constrainCrop)
            tools.updateCrop(g, settled: false)
        case .move:
            guard var g = startGeometry, let view = frozen else { return }
            // Screen offsets are crop-frame offsets; turn them into image offsets.
            let r = view.angle * .pi / 180
            let (qx, qy) = ((p.x - dragStart.x) / view.scale, (p.y - dragStart.y) / view.scale)
            g.move(dx: cos(r) * qx + sin(r) * qy, dy: -sin(r) * qx + cos(r) * qy, constrain: tools.constrainCrop)
            tools.updateCrop(g, settled: false)
        case .rotate(let a0, let p0):
            guard let view = loupe?.cropView, var g = startGeometry else { return }
            var d = pointerAngle(p, view) - p0
            if d > 180 { d -= 360 } else if d < -180 { d += 360 }
            g.rotate(to: ((a0 + d * fine) * 100).rounded() / 100, constrain: tools.constrainCrop)
            // The image turns under a fixed box: keep scale and screen centre.
            var v = view
            v.angle = g.angle
            v.imageCenter = (g.centerX, g.centerY)
            frozen = v
            tools.updateCrop(g, settled: false)
        case .straighten(let a):
            straightenLine = (a, p)
            needsDisplay = true
        case nil: break
        }
    }

    override func mouseUp(with event: NSEvent) {
        let p = convert(event.locationInWindow, from: nil)
        if maskDrag != nil { maskMouseUp(p, event); return }
        switch drag {
        case .targeted(let t, let start):
            tools.dragTargetedHSL(t, delta: ((start.y - p.y) * 0.5).rounded(), final: true)
        case .straighten(let a):
            straightenLine = nil
            if var g = tools.crop, let angle = g.straightenAngle(from: (a.x, a.y), to: (p.x, p.y)) {
                g.rotate(to: (angle * 100).rounded() / 100, constrain: tools.constrainCrop)
                tools.straightening = false
                tools.updateCrop(g, settled: true)
            }
        case .resize, .move, .rotate:
            if let g = tools.crop { tools.updateCrop(g, settled: true) }
        case nil: break
        }
        drag = nil
        frozen = nil
        startGeometry = nil
        toolsChanged()
        updateCursor(p)
    }
}

extension DevelopTools {
    /// Crop-tool and picker keys (before the culling map). Returns true when handled.
    func handleKey(_ event: NSEvent) -> Bool {
        let loupe = model.viewMode == .loupe
        guard loupe, ready else { return false }
        let ch = event.charactersIgnoringModifiers?.lowercased() ?? ""
        if cropActive {
            switch event.keyCode {
            case 36, 76: commitCrop(); return true                 // Return
            case 53: cancelCrop(); return true                     // Esc
            case 123, 124, 125, 126:                               // arrows nudge the crop
                guard var g = crop else { return true }
                let step = event.modifierFlags.contains(.shift) ? 10.0 : 1.0
                let (dx, dy): (Double, Double) = switch event.keyCode {
                case 123: (-step, 0); case 124: (step, 0); case 125: (0, step); default: (0, -step)
                }
                g.move(dx: dx * g.width / 1000, dy: dy * g.height / 1000, constrain: constrainCrop)
                updateCrop(g, settled: true)
                return true
            default: break
            }
            switch ch {
            case "o": cropOverlay = cropOverlay.next; onLoupeToolChange?(); return true
            case "x": flipCropOrientation(); return true
            case "r": commitCrop(); return true
            default: return true   // swallow culling keys while cropping
            }
        }
        if hslPicker != nil || detailPicking, event.keyCode == 53 {
            hslPicker = nil
            detailPicking = false
            onLoupeToolChange?()
            return true
        }
        if ch == "r", event.modifierFlags.intersection(.deviceIndependentFlagsMask).isEmpty {
            beginCrop()
            return true
        }
        return false
    }
}
