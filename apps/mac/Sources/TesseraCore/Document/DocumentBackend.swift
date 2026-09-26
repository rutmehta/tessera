import Foundation
import IOSurface

// The layered-document backend the mac app's document mode codes against (WP M5-10).
//
// This mirrors the `DocumentSession` UniFFI object of WP M5-09 (crates/tessera-ffi/src/document.rs)
// one to one: every record below is the Swift shape of an M5-09 record, every requirement is one
// session call (snake_case in Rust, camelCase here), so M5-10b wires the real session by writing a
// thin adapter (`extension DocumentSession: DocumentBackend`) that converts records field by field.
// Until then `StubDocumentBackend` implements it in Swift so the UI runs and tests pass without the
// engine. Conventions from apps/mac/README.md hold: ids and small records cross the bridge, pixels
// never do (surfaces are IOSurfaces shared by id).

/// `LayerId` of engine-api: unique within a document, never reused. 0 is the root, never a layer.
public typealias DocLayerID = UInt64
/// `HistoryEntryId`: 1-based, stable for the life of the document.
public typealias DocHistoryID = UInt64

/// Document sample depth (`DocBitDepth`).
public enum DocBitDepth: String, CaseIterable, Sendable, Identifiable, Codable {
    case u8, u16, f32
    public var id: String { rawValue }
    public var title: String {
        switch self {
        case .u8: "8-bit"
        case .u16: "16-bit"
        case .f32: "32-bit float"
        }
    }
}

/// `info()`.
public struct DocumentSummary: Equatable, Sendable {
    /// `DocumentId` as a string (session scoped).
    public var id: String
    /// File the document was opened from or last saved to; nil for a new, unsaved document.
    public var path: String?
    /// File name, or "Untitled-N".
    public var title: String
    public var width: UInt32
    public var height: UInt32
    public var depth: DocBitDepth
    public var profileName: String
    /// Unsaved changes since open / the last save.
    public var dirty: Bool
    /// Current history entry, nil = as opened.
    public var historyHead: DocHistoryID?
    public var selectedLayerIds: [DocLayerID]
    /// Library image the document was made from (`open_document_from_image`), if any.
    public var sourceImageId: String?

    public init(id: String, path: String?, title: String, width: UInt32, height: UInt32, depth: DocBitDepth,
                profileName: String, dirty: Bool, historyHead: DocHistoryID?, selectedLayerIds: [DocLayerID],
                sourceImageId: String? = nil) {
        self.id = id; self.path = path; self.title = title; self.width = width; self.height = height
        self.depth = depth; self.profileName = profileName; self.dirty = dirty; self.historyHead = historyHead
        self.selectedLayerIds = selectedLayerIds; self.sourceImageId = sourceImageId
    }
}

/// `LayerRecord.kind`.
public enum LayerKindTag: String, CaseIterable, Sendable, Codable {
    case pixel, adjustment, fill, group
    case smartObject = "smart_object"
    case text

    public var title: String {
        switch self {
        case .pixel: "Pixel"
        case .adjustment: "Adjustment"
        case .fill: "Fill"
        case .group: "Group"
        case .smartObject: "Smart Object"
        case .text: "Text"
        }
    }
}

/// `Locks` of compositor/document.rs.
public struct LayerLockFlags: Equatable, Sendable, Codable {
    public var transparency = false
    public var pixels = false
    public var position = false
    public var all = false
    public init(transparency: Bool = false, pixels: Bool = false, position: Bool = false, all: Bool = false) {
        self.transparency = transparency; self.pixels = pixels; self.position = position; self.all = all
    }
    public var any: Bool { transparency || pixels || position || all }
}

/// A rectangle in level-0 canvas pixels (`bounds`, dirty rects, viewport rects).
public struct CanvasRect: Equatable, Sendable, Codable {
    public var x: Int32
    public var y: Int32
    public var width: UInt32
    public var height: UInt32
    public init(x: Int32, y: Int32, width: UInt32, height: UInt32) {
        self.x = x; self.y = y; self.width = width; self.height = height
    }
    public var isEmpty: Bool { width == 0 || height == 0 }
}

/// Group composition (`GroupMode`): stable strings `pass_through` / `isolated`.
public enum LayerGroupMode: String, CaseIterable, Sendable, Codable {
    case passThrough = "pass_through"
    case isolated
}

/// One row of `layers()`: a flat pre-order walk of the tree. Siblings are listed top first (the
/// order of the Layers panel); `index` is the compositor's child index, 0 = bottom.
public struct LayerRecord: Equatable, Sendable, Identifiable {
    public var id: DocLayerID
    /// nil = a child of the document root.
    public var parent: DocLayerID?
    public var index: UInt32
    /// 0 for root children.
    public var depth: UInt32
    public var kind: LayerKindTag
    public var name: String
    public var visible: Bool
    public var opacity: Float
    public var fillOpacity: Float
    /// `BlendMode` serde name (COMPOSITOR.md §2), e.g. `normal`, `linear_dodge`.
    public var blendMode: String
    /// Groups only.
    public var groupMode: LayerGroupMode?
    public var clipped: Bool
    public var locks: LayerLockFlags
    public var hasMask: Bool
    public var maskEnabled: Bool
    public var maskLinked: Bool
    /// `Adjustment` serde JSON (adjustment layers).
    public var adjustmentJson: String?
    /// `Fill` serde JSON (fill layers).
    public var fillJson: String?
    /// Content bounds, nil when empty or unbounded (adjustments, fills).
    public var bounds: CanvasRect?
    /// Bumped whenever the layer's rendered content or thumbnail changes.
    public var revision: UInt64

    public init(id: DocLayerID, parent: DocLayerID?, index: UInt32, depth: UInt32, kind: LayerKindTag, name: String,
                visible: Bool = true, opacity: Float = 1, fillOpacity: Float = 1, blendMode: String = "normal",
                groupMode: LayerGroupMode? = nil, clipped: Bool = false, locks: LayerLockFlags = LayerLockFlags(),
                hasMask: Bool = false, maskEnabled: Bool = true, maskLinked: Bool = true,
                adjustmentJson: String? = nil, fillJson: String? = nil, bounds: CanvasRect? = nil, revision: UInt64 = 0) {
        self.id = id; self.parent = parent; self.index = index; self.depth = depth; self.kind = kind
        self.name = name; self.visible = visible; self.opacity = opacity; self.fillOpacity = fillOpacity
        self.blendMode = blendMode; self.groupMode = groupMode; self.clipped = clipped; self.locks = locks
        self.hasMask = hasMask; self.maskEnabled = maskEnabled; self.maskLinked = maskLinked
        self.adjustmentJson = adjustmentJson; self.fillJson = fillJson; self.bounds = bounds; self.revision = revision
    }
}

/// `add_layer(kind: NewLayerKind, …)`.
public enum NewLayerKind: Equatable, Sendable {
    /// A blank (transparent) pixel layer.
    case pixel
    case group
    /// `Adjustment` serde JSON.
    case adjustment(json: String)
    /// `Fill` serde JSON.
    case fill(json: String)
}

/// `add_mask(id, LayerMaskInit)`.
public enum LayerMaskInit: String, Sendable, CaseIterable {
    case revealAll = "reveal_all"
    case hideAll = "hide_all"
    case fromSelection = "from_selection"
}

/// `set_props(id, LayerProperties)`: the whole common property set in one history step.
public struct LayerProperties: Equatable, Sendable {
    public var name: String
    public var visible: Bool
    public var opacity: Float
    public var fillOpacity: Float
    public var blendMode: String
    public var clipped: Bool
    public var locks: LayerLockFlags
    public var colorTag: String?
    public init(name: String, visible: Bool, opacity: Float, fillOpacity: Float, blendMode: String, clipped: Bool,
                locks: LayerLockFlags, colorTag: String? = nil) {
        self.name = name; self.visible = visible; self.opacity = opacity; self.fillOpacity = fillOpacity
        self.blendMode = blendMode; self.clipped = clipped; self.locks = locks; self.colorTag = colorTag
    }
    public init(_ node: LayerRecord) {
        self.init(name: node.name, visible: node.visible, opacity: node.opacity, fillOpacity: node.fillOpacity,
                  blendMode: node.blendMode, clipped: node.clipped, locks: node.locks)
    }
}

/// What one edit changed (`DocumentChange`).
public struct DocumentChange: Equatable, Sendable {
    /// Layers whose rows need refreshing; for `add_layer` / `duplicate_layer` the new layer is first.
    public var layersChanged: [DocLayerID]
    public var historyHead: DocHistoryID?
    /// Level-0 canvas area to recomposite (nil: nothing visible changed).
    public var dirtyRect: CanvasRect?
    public init(layersChanged: [DocLayerID], historyHead: DocHistoryID?, dirtyRect: CanvasRect?) {
        self.layersChanged = layersChanged; self.historyHead = historyHead; self.dirtyRect = dirtyRect
    }
}

/// `history_items()`.
public struct DocHistoryEntry: Equatable, Sendable, Identifiable {
    public var id: DocHistoryID
    public var label: String
    public var parent: DocHistoryID?
    public var isCurrent: Bool
    /// "user", "agent:<name>", …
    public var author: String
    public init(id: DocHistoryID, label: String, parent: DocHistoryID?, isCurrent: Bool, author: String = "user") {
        self.id = id; self.label = label; self.parent = parent; self.isCurrent = isCurrent; self.author = author
    }
}

/// `snapshots()`.
public struct DocSnapshot: Equatable, Sendable, Identifiable {
    public var name: String
    /// History entry the snapshot names (nil = as opened).
    public var head: DocHistoryID?
    public var id: String { name }
    public init(name: String, head: DocHistoryID?) { self.name = name; self.head = head }
}

/// `plan_surface(width, height) -> SurfacePlan`: the surface size the backend wants for a viewport
/// of that many level pixels (it may coarsen the level to bound the cost).
public struct DocViewportPlan: Equatable, Sendable {
    public var level: UInt8
    public var width: UInt32
    public var height: UInt32
    public init(level: UInt8, width: UInt32, height: UInt32) { self.level = level; self.width = width; self.height = height }
}

/// `DocumentBackendListener.on_frame(FrameInfo)`: one composite presented into an attached surface.
/// Surfaces are RGBA8, sRGB-encoded, **straight (unpremultiplied) alpha**; transparent areas stay
/// transparent and the app draws the checkerboard under them.
public struct DocFrame: Equatable, Sendable {
    public var surfaceId: UInt32
    /// Level rendered (the size of one texel is 2^level canvas pixels).
    public var level: UInt8
    /// Level-0 canvas rectangle the valid region covers.
    public var rect: CanvasRect
    /// Valid region, anchored top-left in the surface, in texels.
    public var width: UInt32
    public var height: UInt32
    public var renderMs: Double
    /// Increments per presented frame.
    public var generation: UInt64
    /// False for coarse frames of an interactive drag that a finer frame will replace.
    public var isFinal: Bool
    public init(surfaceId: UInt32, level: UInt8, rect: CanvasRect, width: UInt32, height: UInt32, renderMs: Double,
                generation: UInt64, isFinal: Bool) {
        self.surfaceId = surfaceId; self.level = level; self.rect = rect; self.width = width; self.height = height
        self.renderMs = renderMs; self.generation = generation; self.isFinal = isFinal
    }
}

/// `export_flat(path, ExportFormat, quality, ExportColor)`.
public enum DocExportFormat: String, CaseIterable, Sendable {
    case jpeg, png, tiff
}

/// Colour space of a flat export (the export sheet's `ExportSettings.ColorSpace` strings).
public enum DocExportColor: String, CaseIterable, Sendable {
    case srgb
    case displayP3 = "display_p3"
    case rec2020
    case prophoto
}

/// `EngineError` variants a document call can raise.
public enum DocumentError: LocalizedError, Equatable {
    case notFound(String)
    case invalid(String)
    case unsupported(String)
    case io(String)

    public var errorDescription: String? {
        switch self {
        case .notFound(let s): "Not found: \(s)"
        case .invalid(let s): s
        case .unsupported(let s): s
        case .io(let s): s
        }
    }
}

/// `DocumentBackendListener`: called at most once per coalesced frame, from any thread.
public protocol DocumentBackendListener: AnyObject, Sendable {
    func onFrame(_ frame: DocFrame)
    func onLayersChanged()
    func onHistoryChanged()
}

/// `Engine`'s document entry points (`new_document`, `open_document`, `open_document_from_image`).
public protocol DocumentEngine: AnyObject, Sendable {
    func newDocument(width: UInt32, height: UInt32, depth: DocBitDepth, profile: String?) throws -> any DocumentBackend
    /// `.tessera-doc`, `.psd` / `.psb`, or a flat JPEG / PNG / TIFF (one pixel layer). The same path
    /// opened twice returns the same session.
    func openDocument(path: String) throws -> any DocumentBackend
    /// A library image as one pixel layer (its develop recipe applied when `developed`).
    func openDocumentFromImage(imageId: String, developed: Bool) throws -> any DocumentBackend
}

/// `DocumentSession`. Every edit is one `DocOp` (one history entry) unless it says `interactive`:
/// interactive calls re-render without a history entry until `commit(label)`.
public protocol DocumentBackend: AnyObject, Sendable {
    // Model reads
    func info() -> DocumentSummary
    func layers() -> [LayerRecord]
    /// Writes an RGBA8 IOSurface (straight alpha, at most `maxPx` on the long edge), cached per
    /// layer revision, and returns its `IOSurfaceID`. The backend keeps the surface alive.
    func layerThumbnail(id: DocLayerID, maxPx: UInt32) throws -> UInt32
    /// The mask of `id` as an RGBA8 grey IOSurface (M5-10 addition; see IMPLEMENTATION-STATUS.md).
    func maskThumbnail(id: DocLayerID, maxPx: UInt32) throws -> UInt32
    func compositeThumbnail(maxPx: UInt32) throws -> UInt32

    // Edits
    func addLayer(kind: NewLayerKind, name: String, parent: DocLayerID?, index: UInt32?) throws -> DocumentChange
    func duplicateLayer(id: DocLayerID) throws -> DocumentChange
    func removeLayer(id: DocLayerID) throws -> DocumentChange
    /// Removes `id` from its parent, then inserts it at compositor `index` (0 = bottom) of `parent`.
    func moveLayer(id: DocLayerID, parent: DocLayerID?, index: UInt32) throws -> DocumentChange
    func setProps(id: DocLayerID, props: LayerProperties) throws -> DocumentChange
    func setVisible(id: DocLayerID, visible: Bool) throws -> DocumentChange
    func setOpacity(id: DocLayerID, value: Float, interactive: Bool) throws -> DocumentChange
    /// M5-10 addition (the Fill slider's live path); `set_props` covers the committed value.
    func setFillOpacity(id: DocLayerID, value: Float, interactive: Bool) throws -> DocumentChange
    func setBlendMode(id: DocLayerID, mode: String) throws -> DocumentChange
    func setGroupMode(id: DocLayerID, mode: LayerGroupMode) throws -> DocumentChange
    func setAdjustmentJson(id: DocLayerID, json: String, interactive: Bool) throws -> DocumentChange
    func setFillJson(id: DocLayerID, json: String) throws -> DocumentChange
    func setMaskEnabled(id: DocLayerID, enabled: Bool) throws -> DocumentChange
    /// M5-10 addition (the link chain between thumbnail and mask).
    func setMaskLinked(id: DocLayerID, linked: Bool) throws -> DocumentChange
    func removeMask(id: DocLayerID) throws -> DocumentChange
    func addMask(id: DocLayerID, initial: LayerMaskInit) throws -> DocumentChange
    func setClipped(id: DocLayerID, clipped: Bool) throws -> DocumentChange
    func mergeDown(id: DocLayerID) throws -> DocumentChange
    func flatten() throws -> DocumentChange
    func setSelectionRect(x: Int32, y: Int32, width: UInt32, height: UInt32, feather: Float) throws
    func clearSelection() throws
    /// Records the interactive changes since the last commit as one history entry.
    func commit(label: String) throws

    // History
    func undo() throws -> Bool
    func redo() throws -> Bool
    func historyItems() -> [DocHistoryEntry]
    /// `id` 0 checks out the document as opened (engine-api head `None`; M5-10 convention).
    func checkoutHistory(id: DocHistoryID) throws
    func snapshot(name: String) throws
    func snapshots() -> [DocSnapshot]
    func restoreSnapshot(name: String) throws
    func setMaxStates(_ count: UInt32)
    /// M5-10 addition: bytes held by history states (the History panel's memory line).
    func historyMemoryBytes() -> UInt64

    // Presentation
    func setListener(_ listener: (any DocumentBackendListener)?)
    func planSurface(width: UInt32, height: UInt32) -> DocViewportPlan
    func attachSurface(iosurfaceId: UInt32, width: UInt32, height: UInt32) throws
    /// Level-0 canvas rectangle `x, y, w, h` shown at `zoom` (screen pixels per canvas pixel),
    /// rendered at `level`. Triggers a render.
    func setViewport(level: UInt8, x: Int32, y: Int32, width: UInt32, height: UInt32, zoom: Float)
    func setDisplayHeadroom(_ headroom: Float)
    func refresh()
    func detachSurfaces()

    // Output
    func save() throws
    func saveAs(path: String) throws
    func exportFlat(path: String, format: DocExportFormat, quality: UInt8, color: DocExportColor) throws
    func close()
}

/// The IOSurfaces document mode shares with a backend: RGBA8 ('RGBA'), straight alpha, sRGB-encoded.
public enum DocumentSurfaces {
    public static let pixelFormat: UInt32 = 0x5247_4241

    public static func make(width: Int, height: Int) -> IOSurfaceRef? {
        let props: [CFString: Any] = [
            kIOSurfaceWidth: max(width, 1),
            kIOSurfaceHeight: max(height, 1),
            kIOSurfaceBytesPerElement: 4,
            kIOSurfaceBytesPerRow: IOSurfaceAlignProperty(kIOSurfaceBytesPerRow, max(width, 1) * 4),
            kIOSurfacePixelFormat: pixelFormat,
        ]
        return IOSurfaceCreate(props as CFDictionary)
    }
}
