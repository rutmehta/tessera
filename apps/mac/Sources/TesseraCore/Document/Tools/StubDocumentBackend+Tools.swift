import Foundation

// `DocumentToolsBackend` on the stub (WP B5-04). The stub keeps a rectangular selection and has no
// pixels, so it adopts the geometric subset — marquees, lassos and every other selection as their
// bounding rectangle, Select All / Deselect, a rectangular outline — and reports painting,
// transforms and image-driven tools as needing the engine (`--stub-library` runs and unit tests).

extension StubDocumentBackend: DocumentToolsBackend {
    private static func needsEngine(_ what: String) -> DocumentError {
        .unsupported("\(what) needs the engine (the stub backend has no pixels)")
    }

    private func rectSelection(_ r: CGRect, op: SelectionCombine) throws -> DocumentChange {
        let info = try info()
        let canvas = CGRect(x: 0, y: 0, width: Double(info.width), height: Double(info.height))
        let current = info.selectionBounds.map {
            CGRect(x: Double($0.x), y: Double($0.y), width: Double($0.width), height: Double($0.height))
        }
        var next: CGRect? = r.intersection(canvas)
        switch op {
        case .replace: break
        case .add: next = current.map { $0.union(next ?? .null) } ?? next
        case .subtract: next = current   // rectangles cannot subtract; keep the current bounds
        case .intersect: next = current.flatMap { c in next.map { c.intersection($0) } }
        }
        guard let n = next?.integral, !n.isNull, n.width >= 1, n.height >= 1 else { return try clearSelection() }
        return try setSelectionRect(x: Int64(n.minX), y: Int64(n.minY), width: Int64(n.width), height: Int64(n.height),
                                    feather: 0)
    }

    private static func bounds(_ pts: [CanvasPoint]) -> CGRect {
        let xs = pts.map { Double($0.x) }, ys = pts.map { Double($0.y) }
        guard let x0 = xs.min(), let x1 = xs.max(), let y0 = ys.min(), let y1 = ys.max() else { return .null }
        return CGRect(x: x0, y: y0, width: x1 - x0, height: y1 - y0)
    }

    public func beginStroke(layer: DocLayerID, target: BrushStrokeTarget, tool: BrushToolKind, brush: BrushOptions,
                            color: ToolColor) throws { throw Self.needsEngine("Painting") }
    public func strokePoints(_ points: [PenSample]) throws -> StrokeFrameResult { throw Self.needsEngine("Painting") }
    public func endStroke() throws -> DocumentChange { throw Self.needsEngine("Painting") }
    public func cancelStroke() throws -> DocumentChange { try commit(label: "") }
    public func setCloneSource(layer: DocLayerID, dx: Float, dy: Float) throws {}

    public func selectMarquee(_ kind: MarqueeKind, rect: CGRect, feather: Float, antialias: Bool, op: SelectionCombine) throws
        -> DocumentChange {
        let info = try info()
        let r: CGRect = switch kind {
        case .rect, .ellipse: rect
        case .row: CGRect(x: 0, y: rect.minY.rounded(.down), width: Double(info.width), height: 1)
        case .column: CGRect(x: rect.minX.rounded(.down), y: 0, width: 1, height: Double(info.height))
        }
        return try rectSelection(r, op: op)
    }
    public func selectLasso(_ points: [CanvasPoint], mode: LassoMode, feather: Float, antialias: Bool,
                            op: SelectionCombine) throws -> DocumentChange {
        guard points.count >= 3 else { throw DocumentError.invalid("a lasso needs at least 3 points") }
        return try rectSelection(Self.bounds(points), op: op)
    }
    public func magneticPath(from: CanvasPoint, to: CanvasPoint) throws -> [CanvasPoint] { [from, to] }
    public func selectWand(at p: CanvasPoint, tolerance: Float, contiguous: Bool, sampleAll: Bool, antialias: Bool,
                           op: SelectionCombine) throws -> DocumentChange { throw Self.needsEngine("Magic Wand") }
    public func selectQuick(stroke: [CanvasPoint], radius: Float, sampleAll: Bool, op: SelectionCombine) throws -> DocumentChange {
        throw Self.needsEngine("Quick Selection")
    }
    public func selectColorRange(_ color: ToolColor, fuzziness: Float, op: SelectionCombine) throws -> DocumentChange {
        throw Self.needsEngine("Color Range")
    }
    public func selectSubject(op: SelectionCombine) throws -> DocumentChange { throw Self.needsEngine("Select Subject") }
    public func selectSky(op: SelectionCombine) throws -> DocumentChange { throw Self.needsEngine("Select Sky") }
    public func selectObject(at p: CanvasPoint, op: SelectionCombine) throws -> DocumentChange {
        throw Self.needsEngine("Object Selection")
    }
    public func selectAll() throws -> DocumentChange {
        let i = try info()
        return try setSelectionRect(x: 0, y: 0, width: Int64(i.width), height: Int64(i.height), feather: 0)
    }
    public func selectInverse() throws -> DocumentChange { throw Self.needsEngine("Inverse") }
    public func modifySelection(_ kind: SelectionModifyKind, px: Float) throws -> DocumentChange {
        guard let b = try info().selectionBounds else { throw DocumentError.invalid("there is no selection") }
        let d = Double(px)
        let r = CGRect(x: Double(b.x), y: Double(b.y), width: Double(b.width), height: Double(b.height))
        switch kind {
        case .expand: return try rectSelection(r.insetBy(dx: -d, dy: -d), op: .replace)
        case .contract: return try rectSelection(r.insetBy(dx: d, dy: d), op: .replace)
        default: return try rectSelection(r, op: .replace)
        }
    }
    public func refineEdge(_ settings: RefineEdgeSettings, interactive: Bool) throws -> DocumentChange {
        throw Self.needsEngine("Select and Mask")
    }
    public func cancelRefineEdge() throws -> DocumentChange { try commit(label: "") }
    public func selectionOutline(level: UInt8) throws -> [SelectionOutline] {
        guard let b = try info().selectionBounds else { return [] }
        let (x0, y0) = (Float(b.x), Float(b.y)), (x1, y1) = (Float(b.x + b.width), Float(b.y + b.height))
        return [SelectionOutline(points: [CanvasPoint(x: x0, y: y0), CanvasPoint(x: x1, y: y0), CanvasPoint(x: x1, y: y1),
                                          CanvasPoint(x: x0, y: y1)], closed: true)]
    }
    public func saveSelection(name: String) throws { throw Self.needsEngine("Save Selection") }
    public func loadSelection(name: String, op: SelectionCombine) throws -> DocumentChange {
        throw Self.needsEngine("Load Selection")
    }
    public func selectionChannels() throws -> [String] { [] }

    public func beginTransform(layers: [DocLayerID]) throws -> TransformStart { throw Self.needsEngine("Free Transform") }
    public func setTransform(_ m: AffineTransform2D, interpolation: ResampleMode) throws -> DocumentChange {
        throw Self.needsEngine("Free Transform")
    }
    public func commitTransform() throws -> DocumentChange { throw Self.needsEngine("Free Transform") }
    public func cancelTransform() throws -> DocumentChange { try commit(label: "") }

    public func fillSelection(layer: DocLayerID, fill: SelectionFillKind, opacity: Float) throws -> DocumentChange {
        throw Self.needsEngine("Fill")
    }
    public func deleteSelection(layer: DocLayerID, background: ToolColor) throws -> DocumentChange {
        throw Self.needsEngine("Clear")
    }
    public func sampleColor(at p: CanvasPoint, sampleAll: Bool, layer: DocLayerID?, radius: UInt32) throws -> ToolColor {
        throw Self.needsEngine("The eyedropper")
    }

    public func brushTips() -> [BrushTip] { [] }
    public func importAbr(path: String) throws -> [BrushTip] { throw Self.needsEngine("Brush import") }
    public func brushTipPreview(id: String, maxPx: UInt32) throws -> BrushTipBitmap {
        // A computed round tip, for the Brushes panel without the engine.
        let n = Int(max(4, min(maxPx, 256)))
        let hardness = Float(id.hasPrefix("round:") ? id.dropFirst(6) : "1") ?? 1
        let r = Float(n) / 2 - 1
        var px = [UInt8](repeating: 0, count: n * n)
        for y in 0..<n {
            for x in 0..<n {
                let d = hypotf(Float(x) + 0.5 - Float(n) / 2, Float(y) + 0.5 - Float(n) / 2)
                let w = (1 - hardness) * r + 1
                let t = min(max((r + 0.5 - d) / w, 0), 1)
                px[y * n + x] = UInt8((t * t * (3 - 2 * t) * 255).rounded())
            }
        }
        return BrushTipBitmap(width: UInt32(n), height: UInt32(n), pixels: px)
    }
}
