import Foundation
import TesseraFFI

// `DocumentToolsBackend` over the engine session (WP M5-11): one session call per requirement,
// records converted field by field (name table in DocumentToolsBackend.swift).

extension ToolColor {
    init(_ c: PaintColor) { self.init(r: c.r, g: c.g, b: c.b) }
    var ffi: PaintColor { PaintColor(r: r, g: g, b: b) }
}

extension CanvasPoint {
    init(_ p: ToolPoint) { self.init(x: p.x, y: p.y) }
    var ffi: ToolPoint { ToolPoint(x: x, y: y) }
}

extension BrushStrokeTarget {
    var ffi: StrokeTarget { self == .pixels ? .pixels : .mask }
}

extension BrushToolKind {
    var ffi: StrokeTool {
        switch self {
        case .brush: .brush
        case .eraser: .eraser
        case .clone: .clone
        case .heal: .heal
        }
    }
}

extension BrushSymmetry {
    var ffi: PaintSymmetry {
        switch self {
        case .none: .none
        case .vertical: .vertical
        case .horizontal: .horizontal
        case .dual: .dual
        case .diagonal: .diagonal
        case .radial: .radial
        case .mandala: .mandala
        }
    }
}

extension BrushOptions {
    var ffi: PaintBrush {
        PaintBrush(size: size, hardness: hardness, opacity: opacity, flow: flow, spacing: spacing, angle: angle,
                   roundness: roundness, blendMode: blendMode, pressureSize: pressureSize,
                   pressureOpacity: pressureOpacity, pressureFlow: pressureFlow, smoothing: smoothing,
                   symmetry: symmetry.ffi, symmetryX: symmetryX, symmetryY: symmetryY, symmetryCount: symmetryCount,
                   tipId: tipId, sampleAllLayers: sampleAllLayers)
    }
}

extension PenSample {
    var ffi: StrokeSample {
        StrokeSample(x: x, y: y, pressure: pressure, tiltX: tiltX, tiltY: tiltY, timestamp: timestamp)
    }
}

extension StrokeFrameResult {
    init(_ f: StrokeFrame) {
        self.init(dirtyRect: f.dirtyRect.map(CanvasRect.init), dabs: f.dabs, totalDabs: f.totalDabs, rasterMs: f.rasterMs,
                  epoch: f.epoch)
    }
}

extension SelectionCombine {
    var ffi: SelectionOp {
        switch self {
        case .replace: .replace
        case .add: .add
        case .subtract: .subtract
        case .intersect: .intersect
        }
    }
}

extension MarqueeKind {
    var ffi: MarqueeShape {
        switch self {
        case .rect: .rect
        case .ellipse: .ellipse
        case .row: .row
        case .column: .column
        }
    }
}

extension LassoMode {
    var ffi: LassoKind {
        switch self {
        case .free: .free
        case .polygon: .polygon
        case .magnetic: .magnetic
        }
    }
}

extension SelectionModifyKind {
    var ffi: SelectionModify {
        switch self {
        case .border: .border
        case .smooth: .smooth
        case .expand: .expand
        case .contract: .contract
        case .feather: .feather
        }
    }
}

extension RefineEdgeSettings {
    var ffi: RefineEdgeParams {
        RefineEdgeParams(radius: radius, smartRadius: smartRadius, smooth: smooth, feather: feather, contrast: contrast,
                         shiftEdge: shiftEdge)
    }
}

extension AffineTransform2D {
    var ffi: TransformMatrix { TransformMatrix(a: a, b: b, c: c, d: d, e: e, f: f) }
}

extension ResampleMode {
    var ffi: TransformInterpolation {
        switch self {
        case .nearest: .nearest
        case .bilinear: .bilinear
        case .bicubic: .bicubic
        }
    }
}

extension BrushTip {
    init(_ t: BrushTipInfo) {
        self.init(id: t.id, name: t.name, width: t.width, height: t.height, diameter: t.diameter, spacing: t.spacing,
                  sampled: t.sampled)
    }
}

extension EngineDocumentBackend: DocumentToolsBackend {
    public func beginStroke(layer: DocLayerID, target: BrushStrokeTarget, tool: BrushToolKind, brush: BrushOptions,
                            color: ToolColor) throws {
        try bridged { try session.beginStroke(layer: layer, target: target.ffi, tool: tool.ffi, brush: brush.ffi, color: color.ffi) }
    }
    public func strokePoints(_ points: [PenSample]) throws -> StrokeFrameResult {
        StrokeFrameResult(try bridged { try session.strokePoints(points: points.map(\.ffi)) })
    }
    public func endStroke() throws -> DocumentChange { try change { try session.endStroke() } }
    public func cancelStroke() throws -> DocumentChange { try change { try session.cancelStroke() } }
    public func setCloneSource(layer: DocLayerID, dx: Float, dy: Float) throws {
        try bridged { try session.setCloneSource(layer: layer, dx: dx, dy: dy) }
    }

    public func selectMarquee(_ kind: MarqueeKind, rect: CGRect, feather: Float, antialias: Bool, op: SelectionCombine) throws
        -> DocumentChange {
        try change {
            try session.selectMarquee(shape: kind.ffi, x: rect.minX, y: rect.minY, width: rect.width, height: rect.height,
                                      feather: feather, antialias: antialias, op: op.ffi)
        }
    }
    public func selectLasso(_ points: [CanvasPoint], mode: LassoMode, feather: Float, antialias: Bool,
                            op: SelectionCombine) throws -> DocumentChange {
        try change { try session.selectLasso(points: points.map(\.ffi), kind: mode.ffi, feather: feather, antialias: antialias, op: op.ffi) }
    }
    public func magneticPath(from: CanvasPoint, to: CanvasPoint) throws -> [CanvasPoint] {
        try bridged { try session.magneticPath(from: from.ffi, to: to.ffi) }.map(CanvasPoint.init)
    }
    public func selectWand(at p: CanvasPoint, tolerance: Float, contiguous: Bool, sampleAll: Bool, antialias: Bool,
                           op: SelectionCombine) throws -> DocumentChange {
        try change {
            try session.selectWand(x: p.x, y: p.y, tolerance: tolerance, contiguous: contiguous, sampleAll: sampleAll,
                                   antialias: antialias, op: op.ffi)
        }
    }
    public func selectQuick(stroke: [CanvasPoint], radius: Float, sampleAll: Bool, op: SelectionCombine) throws -> DocumentChange {
        try change { try session.selectQuick(stroke: stroke.map(\.ffi), radius: radius, sampleAll: sampleAll, op: op.ffi) }
    }
    public func selectColorRange(_ color: ToolColor, fuzziness: Float, op: SelectionCombine) throws -> DocumentChange {
        try change { try session.selectColorRange(color: color.ffi, fuzziness: fuzziness, op: op.ffi) }
    }
    public func selectSubject(op: SelectionCombine) throws -> DocumentChange { try change { try session.selectSubject(op: op.ffi) } }
    public func selectSky(op: SelectionCombine) throws -> DocumentChange { try change { try session.selectSky(op: op.ffi) } }
    public func selectObject(at p: CanvasPoint, op: SelectionCombine) throws -> DocumentChange {
        try change { try session.selectObject(x: p.x, y: p.y, op: op.ffi) }
    }
    public func selectAll() throws -> DocumentChange { try change { try session.selectAll() } }
    public func selectInverse() throws -> DocumentChange { try change { try session.selectInverse() } }
    public func modifySelection(_ kind: SelectionModifyKind, px: Float) throws -> DocumentChange {
        try change { try session.modifySelection(kind: kind.ffi, px: px) }
    }
    public func refineEdge(_ settings: RefineEdgeSettings, interactive: Bool) throws -> DocumentChange {
        try change { try session.refineEdge(params: settings.ffi, interactive: interactive) }
    }
    public func cancelRefineEdge() throws -> DocumentChange { try change { try session.cancelRefineEdge() } }
    public func selectionOutline(level: UInt8) throws -> [SelectionOutline] {
        try bridged { try session.selectionOutline(level: level) }
            .map { SelectionOutline(points: $0.points.map(CanvasPoint.init), closed: $0.closed) }
    }
    public func saveSelection(name: String) throws { try bridged { try session.saveSelection(name: name) } }
    public func loadSelection(name: String, op: SelectionCombine) throws -> DocumentChange {
        try change { try session.loadSelection(name: name, op: op.ffi) }
    }
    public func selectionChannels() throws -> [String] { try bridged { try session.selectionChannels() } }

    public func beginTransform(layers: [DocLayerID]) throws -> TransformStart {
        let i = try bridged { try session.beginTransform(layers: layers) }
        return TransformStart(layers: i.layers, bounds: i.bounds.map(CanvasRect.init))
    }
    public func setTransform(_ m: AffineTransform2D, interpolation: ResampleMode) throws -> DocumentChange {
        try change { try session.setTransform(matrix: m.ffi, interpolation: interpolation.ffi) }
    }
    public func commitTransform() throws -> DocumentChange { try change { try session.commitTransform() } }
    public func cancelTransform() throws -> DocumentChange { try change { try session.cancelTransform() } }

    public func fillSelection(layer: DocLayerID, fill: SelectionFillKind, opacity: Float) throws -> DocumentChange {
        let f: SelectionFill = switch fill {
        case .color(let c): .color(color: c.ffi)
        case .contentAware: .contentAware
        }
        return try change { try session.fillSelection(layer: layer, fill: f, opacity: opacity) }
    }
    public func deleteSelection(layer: DocLayerID, background: ToolColor) throws -> DocumentChange {
        try change { try session.deleteSelection(layer: layer, background: background.ffi) }
    }
    public func sampleColor(at p: CanvasPoint, sampleAll: Bool, layer: DocLayerID?, radius: UInt32) throws -> ToolColor {
        ToolColor(try bridged { try session.sampleColor(x: p.x, y: p.y, sampleAll: sampleAll, layer: layer, radius: radius) })
    }

    public func brushTips() -> [BrushTip] { TesseraFFI.brushTips().map(BrushTip.init) }
    public func importAbr(path: String) throws -> [BrushTip] { try bridged { try TesseraFFI.importAbr(path: path) }.map(BrushTip.init) }
    public func brushTipPreview(id: String, maxPx: UInt32) throws -> BrushTipBitmap {
        let i = try bridged { try TesseraFFI.brushTipPreview(id: id, maxPx: maxPx) }
        return BrushTipBitmap(width: i.width, height: i.height, pixels: [UInt8](i.pixels))
    }
}
