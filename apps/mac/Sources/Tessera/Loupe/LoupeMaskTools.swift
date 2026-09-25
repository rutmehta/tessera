import AppKit
import TesseraCore
import TesseraFFI

/// A mask gesture on the loupe (M2-14).
enum MaskDrag {
    /// Painting (or erasing) a brush stroke.
    case brush(erase: Bool)
    /// A new linear gradient from `start` (mask space).
    case newLinear(start: (x: Double, y: Double))
    /// A new radial gradient centred at `center` (mask space).
    case newRadial(center: (x: Double, y: Double))
    /// Dragging a handle of the selected group's component.
    case linear(index: Int, shape: LinearGradientShape, handle: GradientHandle, from: (x: Double, y: Double))
    case radial(index: Int, shape: RadialGradientShape, handle: GradientHandle, from: (x: Double, y: Double))
    /// A box (objects, people) or click.
    case box(tool: MaskTool, start: CGPoint, current: CGPoint, option: Bool)
}

enum GradientHandle { case start, end, move, radiusX, radiusY }

/// Loupe mask tools: brush, gradient creation and handles, range picking, object and people prompts.
/// Points are mapped view → displayed uv (`MetalLoupeView.displayUV`) → mask space (`MaskSpace`).
extension LoupeToolOverlay {
    private var space: MaskSpace? {
        guard let d = masks.develop else { return nil }
        return MaskSpace(orientation: Int(d.info.orientation), crop: tools.storedCrop())
    }

    private func toMask(_ p: CGPoint) -> (x: Double, y: Double)? {
        guard let uv = loupe?.displayUV(p), let space else { return nil }
        return space.toMask(uv.u, uv.v)
    }

    private func toView(_ m: (x: Double, y: Double)) -> CGPoint? {
        guard let space else { return nil }
        let uv = space.fromMask(m.x, m.y)
        return loupe?.viewPoint(u: uv.u, v: uv.v)
    }

    private func insidePicture(_ p: CGPoint) -> Bool {
        guard let uv = loupe?.displayUV(p) else { return false }
        return (0...1).contains(uv.u) && (0...1).contains(uv.v)
    }

    /// Brush radius in mask units for the current brush size.
    private var maskBrushRadius: Double? {
        guard let d = masks.develop, let space, let width = loupe?.pictureWidthPoints, width > 0 else { return nil }
        return space.maskRadius(displayedFraction: masks.brushSize / width, imageWidth: Double(d.info.width),
                                imageHeight: Double(d.info.height))
    }

    // MARK: Drawing

    func drawMasks() {
        if let g = masks.selected { drawHandles(g) }
        switch maskDrag {
        case .box(_, let a, let b, _):
            let r = CGRect(x: min(a.x, b.x), y: min(a.y, b.y), width: abs(b.x - a.x), height: abs(b.y - a.y))
            Theme.Palette.OnImage.guide.setStroke()
            let path = NSBezierPath(rect: r.insetBy(dx: 0.5, dy: 0.5))
            path.setLineDash([4, 3], count: 2, phase: 0)
            path.stroke()
        default: break
        }
        if masks.tool == .brush, let p = pointer {
            let erase = NSEvent.modifierFlags.contains(.option) || { if case .brush(true) = maskDrag { true } else { false } }()
            let r = masks.brushSize
            let outer = NSBezierPath(ovalIn: CGRect(x: p.x - r, y: p.y - r, width: 2 * r, height: 2 * r))
            outer.lineWidth = 1
            (erase ? Theme.Palette.OnImage.ink : Theme.Palette.OnImage.guide).withAlphaComponent(0.85).setStroke()
            outer.stroke()
            let ri = r * (1 - masks.brushFeather / 100)
            if ri > 1 {
                let inner = NSBezierPath(ovalIn: CGRect(x: p.x - ri, y: p.y - ri, width: 2 * ri, height: 2 * ri))
                inner.setLineDash([2, 3], count: 2, phase: 0)
                inner.stroke()
            }
            if erase {
                let minus = NSBezierPath()
                minus.move(to: CGPoint(x: p.x - 4, y: p.y)); minus.line(to: CGPoint(x: p.x + 4, y: p.y))
                minus.stroke()
            }
        }
        if let t = masks.tool, maskDrag == nil {
            hint(t.hint)
        } else if masks.tool == nil, maskDrag == nil {
            hint(masks.list.groups.isEmpty ? "Pick a mask tool above the photo · M leaves masking"
                                           : "Drag the handles to adjust · O overlay · X invert · M leaves masking")
        }
    }

    private func drawHandles(_ g: MaskGroupInfo) {
        let line = { (points: [CGPoint], width: CGFloat, alpha: CGFloat) in
            guard points.count > 1 else { return }
            let path = NSBezierPath()
            path.move(to: points[0])
            points.dropFirst().forEach { path.line(to: $0) }
            path.lineWidth = width
            Theme.Palette.OnImage.shadow.withAlphaComponent(0.35 * alpha).setStroke()
            path.stroke()
            path.lineWidth = max(width - 0.5, 0.5)
            Theme.Palette.OnImage.guide.withAlphaComponent(0.85 * alpha).setStroke()
            path.stroke()
        }
        for c in g.components {
            if let s = LinearGradientShape(json: c.definitionJson) {
                // Iso-lines are perpendicular in normalised mask space: draw them there.
                let d = (s.end.x - s.start.x, s.end.y - s.start.y)
                let n = max(hypot(d.0, d.1), 1e-9)
                let perp = (-d.1 / n, d.0 / n)
                for (t, alpha) in [(0.0, 1.0), (0.5, 0.5), (1.0, 1.0)] {
                    let c = (s.start.x + d.0 * t, s.start.y + d.1 * t)
                    let pts = stride(from: -2.0, through: 2.0, by: 0.25).compactMap { k in toView((c.0 + perp.0 * k, c.1 + perp.1 * k)) }
                    line(pts, 1.5, alpha)
                }
                if let a = toView(s.start), let b = toView(s.end) { line([a, b], 1, 0.6); knob(a); knob(b) }
            } else if let r = RadialGradientShape(json: c.definitionJson) {
                line(r.outline().compactMap(toView), 1.5, 1)
                let inner = 1 - r.feather / 100
                if inner > 0.02 { line(r.outline(scale: inner).compactMap(toView), 1, 0.5) }
                if let p = toView(r.center) { knob(p) }
                for h in radialHandles(r) { if let p = toView(h.1) { knob(p, small: true) } }
            }
        }
    }

    private func knob(_ p: CGPoint, small: Bool = false) {
        let r: CGFloat = small ? 3.5 : 5
        let o = NSBezierPath(ovalIn: CGRect(x: p.x - r, y: p.y - r, width: 2 * r, height: 2 * r))
        Theme.Palette.OnImage.guide.setFill()
        o.fill()
        Theme.Palette.OnImage.shadow.setStroke()
        o.lineWidth = 1
        o.stroke()
    }

    private func radialHandles(_ r: RadialGradientShape) -> [(GradientHandle, (x: Double, y: Double))] {
        let a = r.angle * .pi / 180, (c, s) = (cos(a), sin(a))
        return [(.radiusX, (r.center.x + c * r.radii.x, r.center.y + s * r.radii.x)),
                (.radiusY, (r.center.x - s * r.radii.y, r.center.y + c * r.radii.y))]
    }

    // MARK: Cursor

    func maskCursor(_ p: CGPoint) {
        switch masks.tool {
        case .brush: NSCursor.crosshair.set()
        case .colorRange, .luminanceRange: NSCursor.crosshair.set()
        case .linear, .radial, .object, .person: NSCursor.crosshair.set()
        case nil: (handleHit(p) != nil ? NSCursor.openHand : NSCursor.arrow).set()
        }
    }

    // MARK: Mouse

    private func handleHit(_ p: CGPoint) -> MaskDrag? {
        guard let g = masks.selected else { return nil }
        let near = { (m: (x: Double, y: Double)) -> Bool in
            guard let v = self.toView(m) else { return false }
            return hypot(v.x - p.x, v.y - p.y) < 9
        }
        guard let from = toMask(p) else { return nil }
        for (i, c) in g.components.enumerated().reversed() {
            if let s = LinearGradientShape(json: c.definitionJson) {
                if near(s.start) { return .linear(index: i, shape: s, handle: .start, from: from) }
                if near(s.end) { return .linear(index: i, shape: s, handle: .end, from: from) }
                if near(((s.start.x + s.end.x) / 2, (s.start.y + s.end.y) / 2)) {
                    return .linear(index: i, shape: s, handle: .move, from: from)
                }
            } else if let r = RadialGradientShape(json: c.definitionJson) {
                if near(r.center) { return .radial(index: i, shape: r, handle: .move, from: from) }
                for (h, m) in radialHandles(r) where near(m) { return .radial(index: i, shape: r, handle: h, from: from) }
            }
        }
        return nil
    }

    func maskMouseDown(_ p: CGPoint, _ event: NSEvent) {
        let option = event.modifierFlags.contains(.option)
        let shift = event.modifierFlags.contains(.shift)
        let pressure = event.pressure > 0 ? Double(event.pressure) : 1
        // Handles of the selected mask take precedence over creating new geometry.
        if masks.tool != .brush, let hit = handleHit(p) {
            switch hit {
            case .linear(let i, _, _, _): masks.editGradient(group: masks.selected!.id, index: i, title: "Linear Gradient")
            case .radial(let i, _, _, _): masks.editGradient(group: masks.selected!.id, index: i, title: "Radial Gradient")
            default: break
            }
            maskDrag = hit
            NSCursor.closedHand.set()
            return
        }
        guard let tool = masks.tool, let m = toMask(p) else { return }
        switch tool {
        case .brush:
            guard let radius = maskBrushRadius else { return }
            masks.beginStroke(at: m, pressure: pressure, erase: option, radius: radius)
            maskDrag = .brush(erase: option)
        case .linear:
            maskDrag = .newLinear(start: m)
            _ = masks.beginGradient(LinearGradientShape(start: m, end: (m.x, m.y + 0.01)).json,
                                    title: "Linear Gradient", option: option)
        case .radial:
            maskDrag = .newRadial(center: m)
            _ = masks.beginGradient(RadialGradientShape(center: m, radii: (0.01, 0.01)).json,
                                    title: "Radial Gradient", option: option)
        case .colorRange, .luminanceRange:
            guard insidePicture(p) else { return }
            masks.pickRange(tool == .colorRange ? .color : .luminance, at: m, option: option,
                            addSample: shift && tool == .colorRange)
        case .object, .person:
            maskDrag = .box(tool: tool, start: p, current: p, option: option)
        }
        needsDisplay = true
    }

    func maskMouseDragged(_ p: CGPoint, _ event: NSEvent) {
        pointer = p
        guard let m = toMask(p) else { return }
        switch maskDrag {
        case .brush:
            masks.continueStroke(at: m, pressure: event.pressure > 0 ? Double(event.pressure) : 1)
        case .newLinear(let start):
            masks.shapeGradient(LinearGradientShape(start: start, end: m).json, final: false)
        case .newRadial(let c):
            masks.shapeGradient(radial(center: c, to: m, circle: event.modifierFlags.contains(.shift)).json, final: false)
        case .linear(_, var s, let h, let from):
            let (dx, dy) = (m.x - from.x, m.y - from.y)
            switch h {
            case .start: s.start = (s.start.x + dx, s.start.y + dy)
            case .end: s.end = (s.end.x + dx, s.end.y + dy)
            default: s.start = (s.start.x + dx, s.start.y + dy); s.end = (s.end.x + dx, s.end.y + dy)
            }
            masks.shapeGradient(s.json, final: false)
        case .radial(_, var r, let h, let from):
            let a = r.angle * .pi / 180
            let (qx, qy) = (m.x - r.center.x, m.y - r.center.y)
            switch h {
            case .radiusX: r.radii.x = max(abs(cos(a) * qx + sin(a) * qy), 0.002)
            case .radiusY: r.radii.y = max(abs(-sin(a) * qx + cos(a) * qy), 0.002)
            default: r.center = (r.center.x + m.x - from.x, r.center.y + m.y - from.y)
            }
            masks.shapeGradient(r.json, final: false)
        case .box(let tool, let start, _, let option):
            maskDrag = .box(tool: tool, start: start, current: p, option: option)
        case nil: break
        }
        needsDisplay = true
    }

    func maskMouseUp(_ p: CGPoint, _ event: NSEvent) {
        let m = toMask(p)
        switch maskDrag {
        case .brush(let erase):
            if let m { masks.continueStroke(at: m, pressure: 1) }
            masks.endStroke(erase: erase)
        case .newLinear(let start):
            let end = m ?? (start.x, start.y + 0.2)
            let long = hypot(end.x - start.x, end.y - start.y) > 0.005
            masks.shapeGradient(LinearGradientShape(start: start, end: long ? end : (start.x, start.y + 0.25)).json, final: true)
        case .newRadial(let c):
            var r = radial(center: c, to: m ?? c, circle: event.modifierFlags.contains(.shift))
            if r.radii.x < 0.01 || r.radii.y < 0.01 { r.radii = (0.15, 0.15 * aspect) }
            masks.shapeGradient(r.json, final: true)
        case .linear(_, var s, let h, let from):
            if let m {
                let (dx, dy) = (m.x - from.x, m.y - from.y)
                switch h {
                case .start: s.start = (s.start.x + dx, s.start.y + dy)
                case .end: s.end = (s.end.x + dx, s.end.y + dy)
                default: s.start = (s.start.x + dx, s.start.y + dy); s.end = (s.end.x + dx, s.end.y + dy)
                }
            }
            masks.shapeGradient(s.json, final: true)
        case .radial(_, var r, let h, let from):
            if let m {
                let a = r.angle * .pi / 180
                let (qx, qy) = (m.x - r.center.x, m.y - r.center.y)
                switch h {
                case .radiusX: r.radii.x = max(abs(cos(a) * qx + sin(a) * qy), 0.002)
                case .radiusY: r.radii.y = max(abs(-sin(a) * qx + cos(a) * qy), 0.002)
                default: r.center = (r.center.x + m.x - from.x, r.center.y + m.y - from.y)
                }
            }
            masks.shapeGradient(r.json, final: true)
        case .box(let tool, let start, _, let option):
            finishBox(tool, from: start, to: p, option: option)
        case nil: break
        }
        maskDrag = nil
        needsDisplay = true
        updateCursor(p)
    }

    /// Image height / width in mask units, for circular defaults.
    private var aspect: Double {
        guard let d = masks.develop, d.info.height > 0 else { return 1 }
        return Double(d.info.width) / Double(d.info.height)
    }

    private func radial(center c: (x: Double, y: Double), to m: (x: Double, y: Double), circle: Bool) -> RadialGradientShape {
        var rx = max(abs(m.x - c.x), 0.002), ry = max(abs(m.y - c.y), 0.002)
        if circle {
            // Equal radii in pixels: rx·width = ry·height.
            let px = max(rx, ry / aspect)
            (rx, ry) = (px, px * aspect)
        }
        return RadialGradientShape(center: c, radii: (rx, ry), feather: 50)
    }

    private func finishBox(_ tool: MaskTool, from a: CGPoint, to b: CGPoint, option: Bool) {
        let click = hypot(b.x - a.x, b.y - a.y) < 5
        if click {
            guard tool == .object, insidePicture(b), let m = toMask(b) else {
                if tool == .person { masks.model.statusMessage = "Drag a box around a face" }
                return
            }
            masks.runAI(.object(points: [MaskPoint(x: Float(m.x), y: Float(m.y))], region: nil), title: "Object", option: option)
            return
        }
        // A view-aligned box maps to a quad in mask space: use its bounds.
        let corners = [a, b, CGPoint(x: a.x, y: b.y), CGPoint(x: b.x, y: a.y)].compactMap(toMask)
        guard corners.count == 4 else { return }
        let xs = corners.map(\.x), ys = corners.map(\.y)
        let rect = MaskRect(left: Float(max(xs.min()!, 0)), top: Float(max(ys.min()!, 0)),
                            right: Float(min(xs.max()!, 1)), bottom: Float(min(ys.max()!, 1)))
        guard rect.right > rect.left, rect.bottom > rect.top else { return }
        if tool == .person {
            masks.runAI(.person(face: rect), title: "Person", option: option)
        } else {
            masks.runAI(.object(points: [], region: rect), title: "Object", option: option)
        }
    }
}
