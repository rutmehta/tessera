import CoreGraphics
import Foundation

/// Zoom, pan and level maths of the document viewport (pure, unit-tested).
///
/// `zoom` is drawable (device) pixels per level-0 canvas pixel: 1 = 100 % = one image pixel per
/// screen pixel. `center` is the canvas point shown at the middle of the view.
public struct DocumentViewportMath: Equatable, Sendable {
    public var canvasWidth: Double
    public var canvasHeight: Double
    /// Drawable size in device pixels.
    public var viewWidth: Double
    public var viewHeight: Double
    public var zoom: Double
    public var center: CGPoint

    public static let minZoom = 1.0 / 64
    public static let maxZoom = 32.0
    /// Photoshop's zoom presets (⌘+ / ⌘−).
    public static let steps: [Double] = [
        1.0 / 64, 1.0 / 48, 1.0 / 32, 1.0 / 24, 1.0 / 16, 1.0 / 12, 1.0 / 8, 1.0 / 6, 1.0 / 4, 1.0 / 3, 1.0 / 2,
        2.0 / 3, 1, 2, 3, 4, 5, 6, 7, 8, 12, 16, 32,
    ]
    /// Fit leaves this fraction of the view around the canvas.
    public static let fitMargin = 0.94

    public init(canvasWidth: Double, canvasHeight: Double, viewWidth: Double, viewHeight: Double,
                zoom: Double? = nil, center: CGPoint? = nil) {
        self.canvasWidth = max(canvasWidth, 1)
        self.canvasHeight = max(canvasHeight, 1)
        self.viewWidth = max(viewWidth, 1)
        self.viewHeight = max(viewHeight, 1)
        self.zoom = 1
        self.center = center ?? CGPoint(x: canvasWidth / 2, y: canvasHeight / 2)
        self.zoom = zoom ?? fitZoom
    }

    /// Largest zoom that shows the whole canvas (never above 100 %).
    public var fitZoom: Double {
        Self.clampZoom(min(viewWidth * Self.fitMargin / canvasWidth, viewHeight * Self.fitMargin / canvasHeight, 1))
    }

    public static func clampZoom(_ z: Double) -> Double { min(max(z, minZoom), maxZoom) }

    /// Coarsest pyramid level with at least one texel per screen pixel: `floor(log2(1 / zoom))`,
    /// clamped to `0...maxLevel`.
    public static func level(forZoom zoom: Double, maxLevel: Int = 15) -> Int {
        guard zoom > 0, zoom.isFinite else { return 0 }
        let l = Int(floor(log2(1 / zoom) + 1e-9))
        return min(max(l, 0), maxLevel)
    }

    /// Levels the canvas has (halving until the long edge is one pixel).
    public var maxLevel: Int { max(Int(floor(log2(max(canvasWidth, canvasHeight)))), 0) }
    public var level: Int { Self.level(forZoom: zoom, maxLevel: maxLevel) }

    /// Next preset above / below the current zoom.
    public static func zoomIn(from z: Double) -> Double { steps.first { $0 > z * 1.0001 } ?? maxZoom }
    public static func zoomOut(from z: Double) -> Double { steps.last { $0 < z / 1.0001 } ?? minZoom }

    // MARK: Mapping (view pixels are top-left origin, y down)

    public func canvasPoint(view p: CGPoint) -> CGPoint {
        CGPoint(x: center.x + (p.x - viewWidth / 2) / zoom, y: center.y + (p.y - viewHeight / 2) / zoom)
    }

    public func viewPoint(canvas p: CGPoint) -> CGPoint {
        CGPoint(x: (p.x - center.x) * zoom + viewWidth / 2, y: (p.y - center.y) * zoom + viewHeight / 2)
    }

    /// The canvas as it lies on screen, in view pixels.
    public var canvasOnScreen: CGRect {
        let o = viewPoint(canvas: .zero)
        return CGRect(x: o.x, y: o.y, width: canvasWidth * zoom, height: canvasHeight * zoom)
    }

    /// Visible part of the canvas, in integer level-0 pixels (empty when the canvas is off screen).
    public var visibleCanvasRect: CanvasRect {
        let a = canvasPoint(view: .zero), b = canvasPoint(view: CGPoint(x: viewWidth, y: viewHeight))
        let x0 = max(floor(a.x), 0), y0 = max(floor(a.y), 0)
        let x1 = min(ceil(b.x), canvasWidth), y1 = min(ceil(b.y), canvasHeight)
        guard x1 > x0, y1 > y0 else { return CanvasRect(x: 0, y: 0, width: 0, height: 0) }
        return CanvasRect(x: Int32(x0), y: Int32(y0), width: UInt32(x1 - x0), height: UInt32(y1 - y0))
    }

    /// Texels of `rect` at `level` (ceil of the halvings).
    public static func levelSize(_ rect: CanvasRect, level: Int) -> (width: UInt32, height: UInt32) {
        let s = Double(1 << level)
        return (UInt32(max(ceil(Double(rect.width) / s), 1)), UInt32(max(ceil(Double(rect.height) / s), 1)))
    }

    // MARK: Changes

    /// Zoom to `z` keeping the canvas point under view point `anchor` fixed (pinch, ⌥-drag, ⌘+ at the pointer).
    public mutating func setZoom(_ z: Double, anchor: CGPoint? = nil) {
        let a = anchor ?? CGPoint(x: viewWidth / 2, y: viewHeight / 2)
        let fixed = canvasPoint(view: a)
        zoom = Self.clampZoom(z)
        center = CGPoint(x: fixed.x - (a.x - viewWidth / 2) / zoom, y: fixed.y - (a.y - viewHeight / 2) / zoom)
        clampCenter()
    }

    public mutating func fit() {
        zoom = fitZoom
        center = CGPoint(x: canvasWidth / 2, y: canvasHeight / 2)
    }

    /// Pan by a view-pixel delta (content follows the pointer).
    public mutating func pan(dx: Double, dy: Double) {
        center.x -= dx / zoom
        center.y -= dy / zoom
        clampCenter()
    }

    public mutating func resize(viewWidth w: Double, viewHeight h: Double) {
        viewWidth = max(w, 1)
        viewHeight = max(h, 1)
        clampCenter()
    }

    /// Keeps the canvas reachable: the view centre stays over the canvas (no panning into the void).
    public mutating func clampCenter() {
        center.x = min(max(center.x, 0), canvasWidth)
        center.y = min(max(center.y, 0), canvasHeight)
    }

    /// "100 %", "33.3 %", "1600 %".
    public static func percentText(_ zoom: Double) -> String {
        let p = zoom * 100
        if abs(p - p.rounded()) < 0.05 { return "\(Int(p.rounded())) %" }
        return String(format: p < 10 ? "%.2f %%" : "%.1f %%", p)
    }
}
