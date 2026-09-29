import CoreGraphics
import Foundation

// Content-Aware Move / Extend (WP B5-13): the protocol document backends adopt, mirroring the B5-13 session calls
// (crates/tessera-ffi/src/document/content_aware.rs). The selection is frozen engine-side when a move starts and
// the result is installed as one node covering source AND destination (the destination may lie outside the
// selection). UI-free state (options → JSON, drag → integer offset, enablement) is unit tested
// (DocumentContentAwareTests).

/// Move cuts the selection and heals the hole; Extend keeps the original (FFI `ContentAwareMode`).
public enum ContentAwareMoveMode: String, CaseIterable, Sendable, Identifiable {
    case move, extend
    public var id: String { rawValue }
    public var title: String { self == .move ? "Move" : "Extend" }
    public var historyLabel: String { self == .move ? "Content-Aware Move" : "Content-Aware Extend" }
    public var help: String {
        self == .move ? "Moves the selection and fills the place it left from its surroundings"
            : "Copies the selection to the new place; the original stays"
    }
}

/// How the pasted subject's colours adapt at its seam (FFI `ContentAwareSeam`), Photoshop's Color control.
public enum ContentAwareSeamLevel: String, CaseIterable, Sendable, Identifiable {
    case none, standard, high, veryHigh
    public var id: String { rawValue }
    public var title: String {
        switch self {
        case .none: "None"
        case .standard: "Default"
        case .high: "High"
        case .veryHigh: "Very High"
        }
    }
}

/// Options bar state, turned into the adapter's `fill` JSON.
public struct ContentAwareOptions: Equatable, Sendable {
    public var mode: ContentAwareMoveMode = .move
    /// Photoshop's Structure 1…7: how closely the fill keeps patch structure (patch radius 1…7).
    public private(set) var structure: Int = 4
    public var seam: ContentAwareSeamLevel = .standard
    /// PatchMatch seed: the same seed repeats the same result.
    public var seed: UInt64 = 1
    public init() {}

    public mutating func setStructure(_ v: Int) { structure = min(max(v, 1), 7) }

    /// The adapter's `fill` object (`caf::FillParams`).
    public var fillJson: String { #"{"patch_radius":\#(structure),"iterations":5,"seed":\#(seed)}"# }
}

/// A move being arranged (FFI `ContentAwareMoveInfo`).
public struct ContentAwareMoveSession: Equatable, Sendable {
    public var token: UInt64
    public var layer: DocLayerID
    public var mode: ContentAwareMoveMode
    public var smartObject: Bool
    public var selectionBounds: CanvasRect
    public var width: UInt32
    public var height: UInt32
    public init(token: UInt64, layer: DocLayerID, mode: ContentAwareMoveMode, smartObject: Bool, selectionBounds: CanvasRect,
                width: UInt32, height: UInt32) {
        self.token = token; self.layer = layer; self.mode = mode; self.smartObject = smartObject
        self.selectionBounds = selectionBounds; self.width = width; self.height = height
    }
}

/// A computed preview (FFI `ContentAwarePreviewResult`).
public struct ContentAwarePreviewInfo: Equatable, Sendable {
    public var dx: Int32
    public var dy: Int32
    public var affected: CanvasRect?
    public var millis: Double
    public init(dx: Int32, dy: Int32, affected: CanvasRect?, millis: Double) {
        self.dx = dx; self.dy = dy; self.affected = affected; self.millis = millis
    }
}

/// Pointer → document-pixel offset. The drag is measured in canvas (level-0) pixels, whatever the zoom, and
/// rounded to whole pixels (the engine moves by integers to keep source detail); the offset keeps the frozen
/// selection at least partly on the canvas.
public enum ContentAwareDrag {
    public static func offset(from start: CGPoint, to current: CGPoint, bounds: CanvasRect, canvasWidth: UInt32,
                              canvasHeight: UInt32) -> (dx: Int32, dy: Int32) {
        var dx = (Double(current.x) - Double(start.x)).rounded()
        var dy = (Double(current.y) - Double(start.y)).rounded()
        // Keep at least one selected column/row on the canvas.
        dx = min(max(dx, Double(-(bounds.x + bounds.width - 1))), Double(Int64(canvasWidth) - 1 - bounds.x))
        dy = min(max(dy, Double(-(bounds.y + bounds.height - 1))), Double(Int64(canvasHeight) - 1 - bounds.y))
        return (Int32(dx), Int32(dy))
    }

    /// The frozen selection's bounds moved by the offset (the ghost the canvas draws while dragging).
    public static func moved(_ r: CanvasRect, dx: Int32, dy: Int32) -> CanvasRect {
        CanvasRect(x: r.x + Int64(dx), y: r.y + Int64(dy), width: r.width, height: r.height)
    }
}

public enum ContentAwareMenuState {
    /// The tool can start: a pixel layer or a smart object with a selection.
    public static func canStart(layerKind: LayerKindTag?, hasSelection: Bool) -> String? {
        guard let k = layerKind else { return "Select a layer" }
        guard k == .pixel || k == .smartObject else { return "Content-Aware Move works on pixel layers and smart objects" }
        guard hasSelection else { return "Make a selection around what to move, then drag it" }
        return nil
    }
}

public protocol DocumentContentAwareBackend: AnyObject, Sendable {
    func beginContentAwareMove(layer: DocLayerID, mode: ContentAwareMoveMode) throws -> ContentAwareMoveSession
    /// Blocking (full resolution): off the main thread.
    func previewContentAwareMove(token: UInt64, dx: Int32, dy: Int32, fillJson: String,
                                 seam: ContentAwareSeamLevel) throws -> ContentAwarePreviewInfo
    func commitContentAwareMove(token: UInt64) throws -> DocumentChange
    func cancelContentAwareMove(token: UInt64)
}
