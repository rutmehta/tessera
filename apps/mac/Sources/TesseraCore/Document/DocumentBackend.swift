import Foundation
import IOSurface

// The layered-document backend the mac app's document mode codes against (WP B5-02).
//
// This mirrors the `DocumentSession` UniFFI object of WP B5-01 (crates/tessera-ffi/src/document.rs)
// one to one: every record below is the Swift shape of an B5-01 record, every requirement is one
// session call (snake_case in Rust, camelCase here), so B5-03 wires the real session by writing a
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

/// `info()` (FFI `DocumentInfo`).
public struct DocumentSummary: Equatable, Sendable {
    /// `doc#N` (session scoped).
    public var id: String
    /// Where `save` writes (`.tessera-doc`, `.psd`, `.psb`); nil for new documents and flat images.
    public var path: String?
    /// File name, or "Untitled-N".
    public var title: String
    public var width: UInt32
    public var height: UInt32
    public var depth: DocBitDepth
    /// Profile description; nil = untagged (treated as sRGB).
    public var profileName: String?
    /// Unsaved changes.
    public var dirty: Bool
    /// Current history node; 0 = the document as opened.
    public var historyHead: DocHistoryID
    public var canUndo: Bool
    public var canRedo: Bool
    public var selectedLayerIds: [DocLayerID]
    /// Bounds of the marquee selection (nil: none).
    public var selectionBounds: CanvasRect?
    /// Library image the document was made from (`open_document_from_image`).
    public var sourceImageId: String?
    public var layerCount: UInt32
    /// Increments on every mutation.
    public var epoch: UInt64
    /// "Metal (<adapter>)", "CPU", or "Stub".
    public var backend: String

    public init(id: String, path: String?, title: String, width: UInt32, height: UInt32, depth: DocBitDepth,
                profileName: String?, dirty: Bool, historyHead: DocHistoryID, canUndo: Bool, canRedo: Bool,
                selectedLayerIds: [DocLayerID], selectionBounds: CanvasRect?, sourceImageId: String?, layerCount: UInt32,
                epoch: UInt64, backend: String) {
        self.id = id; self.path = path; self.title = title; self.width = width; self.height = height; self.depth = depth
        self.profileName = profileName; self.dirty = dirty; self.historyHead = historyHead; self.canUndo = canUndo
        self.canRedo = canRedo; self.selectedLayerIds = selectedLayerIds; self.selectionBounds = selectionBounds
        self.sourceImageId = sourceImageId; self.layerCount = layerCount; self.epoch = epoch; self.backend = backend
    }
}

/// `LayerNode.kind` (FFI `DocLayerKind`).
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

/// `LayerLocks` (compositor `Locks`).
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

/// `DocRect`: a rectangle in canvas pixels.
public struct CanvasRect: Equatable, Sendable, Codable {
    public var x: Int64
    public var y: Int64
    public var width: Int64
    public var height: Int64
    public init(x: Int64, y: Int64, width: Int64, height: Int64) {
        self.x = x; self.y = y; self.width = width; self.height = height
    }
    public var isEmpty: Bool { width <= 0 || height <= 0 }
}

/// `DocGroupMode`. Groups report `pass_through` as their blend mode while in pass-through.
public enum LayerGroupMode: String, CaseIterable, Sendable, Codable {
    case passThrough = "pass_through"
    case isolated
}

/// One row of `layers()` (FFI `LayerNode`): a flat pre-order walk of the tree. Siblings are
/// listed top first (the order of the Layers panel); `index` is the compositor's child index,
/// 0 = bottom.
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
    /// `BlendMode` serde name (COMPOSITOR.md §2), e.g. `normal`, `linear_dodge`; `pass_through`
    /// for groups in pass-through.
    public var blendMode: String
    /// Groups only.
    public var groupMode: LayerGroupMode?
    public var clipped: Bool
    public var locks: LayerLockFlags
    /// `none`, `shallow` or `deep`.
    public var knockout: String
    /// The document's Background layer.
    public var background: Bool
    public var hasMask: Bool
    public var maskEnabled: Bool
    public var maskLinked: Bool
    public var maskDensity: Float
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
                knockout: String = "none", background: Bool = false,
                hasMask: Bool = false, maskEnabled: Bool = true, maskLinked: Bool = true, maskDensity: Float = 1,
                adjustmentJson: String? = nil, fillJson: String? = nil, bounds: CanvasRect? = nil, revision: UInt64 = 0) {
        self.id = id; self.parent = parent; self.index = index; self.depth = depth; self.kind = kind
        self.name = name; self.visible = visible; self.opacity = opacity; self.fillOpacity = fillOpacity
        self.blendMode = blendMode; self.groupMode = groupMode; self.clipped = clipped; self.locks = locks
        self.knockout = knockout; self.background = background
        self.hasMask = hasMask; self.maskEnabled = maskEnabled; self.maskLinked = maskLinked; self.maskDensity = maskDensity
        self.adjustmentJson = adjustmentJson; self.fillJson = fillJson; self.bounds = bounds; self.revision = revision
    }
}

/// `add_layer(kind: NewLayer, …)`.
public enum NewLayerKind: Equatable, Sendable {
    /// A blank (transparent) pixel layer.
    case pixel
    case group(mode: LayerGroupMode)
    /// `Adjustment` serde JSON.
    case adjustment(json: String)
    /// `Fill` serde JSON.
    case fill(json: String)
}

/// `add_mask(id, MaskInit)`.
public enum LayerMaskInit: String, Sendable, CaseIterable {
    case revealAll = "reveal_all"
    case hideAll = "hide_all"
    case fromSelection = "from_selection"
}

/// `set_props(id, LayerPropsRecord)`: the whole common property set in one history step.
public struct LayerProperties: Equatable, Sendable {
    public var name: String
    public var visible: Bool
    public var opacity: Float
    public var fillOpacity: Float
    public var blendMode: String
    public var clipped: Bool
    public var locks: LayerLockFlags
    public var knockout: String
    public var colorTag: String?
    public init(name: String, visible: Bool, opacity: Float, fillOpacity: Float, blendMode: String, clipped: Bool,
                locks: LayerLockFlags, knockout: String = "none", colorTag: String? = nil) {
        self.name = name; self.visible = visible; self.opacity = opacity; self.fillOpacity = fillOpacity
        self.blendMode = blendMode; self.clipped = clipped; self.locks = locks; self.knockout = knockout; self.colorTag = colorTag
    }
    public init(_ node: LayerRecord) {
        self.init(name: node.name, visible: node.visible, opacity: node.opacity, fillOpacity: node.fillOpacity,
                  blendMode: node.blendMode, clipped: node.clipped, locks: node.locks, knockout: node.knockout)
    }
}

/// What one edit did (FFI `DocumentUpdate`).
public struct DocumentChange: Equatable, Sendable {
    /// Rows that may have changed: edited, added or removed layers and their ancestor groups.
    public var layersChanged: [DocLayerID]
    /// Layers the edit created (added, duplicated, merged, grouped).
    public var created: [DocLayerID]
    /// Current history node (unchanged by interactive edits).
    public var historyHead: DocHistoryID
    /// Level-0 region whose composite may have changed (nil: nothing).
    public var dirtyRect: CanvasRect?
    public var epoch: UInt64
    public var dirty: Bool
    public init(layersChanged: [DocLayerID], created: [DocLayerID], historyHead: DocHistoryID, dirtyRect: CanvasRect?,
                epoch: UInt64, dirty: Bool) {
        self.layersChanged = layersChanged; self.created = created; self.historyHead = historyHead
        self.dirtyRect = dirtyRect; self.epoch = epoch; self.dirty = dirty
    }
}

/// `history_items()` (FFI `DocHistoryItem`).
public struct DocHistoryEntry: Equatable, Sendable, Identifiable {
    public var id: DocHistoryID
    public var label: String
    /// nil for the opened state (or after pruning).
    public var parent: DocHistoryID?
    public var isCurrent: Bool
    /// "user", "agent:<name>", …
    public var author: String
    public init(id: DocHistoryID, label: String, parent: DocHistoryID?, isCurrent: Bool, author: String = "user") {
        self.id = id; self.label = label; self.parent = parent; self.isCurrent = isCurrent; self.author = author
    }
}

/// `plan_surface(width, height)` (FFI `DocSurfacePlan`): the surface extent for a fit-to-window
/// viewport of that many device pixels — the coarsest level covering it.
public struct DocViewportPlan: Equatable, Sendable {
    public var level: UInt8
    public var width: UInt32
    public var height: UInt32
    public init(level: UInt8, width: UInt32, height: UInt32) { self.level = level; self.width = width; self.height = height }
}

/// A presented frame (FFI `DocFrameInfo`). Surfaces are RGBA8, sRGB-encoded, **straight
/// (unpremultiplied) alpha**; transparent areas stay transparent and the app draws the checkerboard.
public struct DocFrame: Equatable, Sendable {
    /// Surface written (0 when none is attached).
    public var surfaceId: UInt32
    public var level: UInt8
    /// The presented region in `level` coordinates; its `width × height` texels sit top-left in the surface.
    public var x: UInt32
    public var y: UInt32
    public var width: UInt32
    public var height: UInt32
    /// The same region in level-0 canvas pixels (clipped to the canvas).
    public var canvasRect: CanvasRect
    /// Extent of the whole level.
    public var levelWidth: UInt32
    public var levelHeight: UInt32
    /// Echo of the last `set_viewport` zoom.
    public var zoom: Double
    public var epoch: UInt64
    public var renderMs: Double
    public var fullRecomposite: Bool
    public var blocks: UInt32
    public init(surfaceId: UInt32, level: UInt8, x: UInt32, y: UInt32, width: UInt32, height: UInt32, canvasRect: CanvasRect,
                levelWidth: UInt32, levelHeight: UInt32, zoom: Double, epoch: UInt64, renderMs: Double,
                fullRecomposite: Bool, blocks: UInt32) {
        self.surfaceId = surfaceId; self.level = level; self.x = x; self.y = y; self.width = width; self.height = height
        self.canvasRect = canvasRect; self.levelWidth = levelWidth; self.levelHeight = levelHeight; self.zoom = zoom
        self.epoch = epoch; self.renderMs = renderMs; self.fullRecomposite = fullRecomposite; self.blocks = blocks
    }
}

/// `export_flat(path, ExportFormat, quality, ExportColor)`.
public enum DocExportFormat: String, CaseIterable, Sendable {
    case png, jpeg, tiff
}

/// Colour space of a flat export (FFI `ExportColor`).
public enum DocExportColor: String, CaseIterable, Sendable {
    /// The document's own profile.
    case document
    case srgb
    case displayP3 = "display_p3"
    case adobeRgb = "adobe_rgb"
    case proPhoto = "prophoto"
    case rec2020
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

/// `DocumentListener`: called from the render thread, at most once per coalesced frame.
public protocol DocumentBackendListener: AnyObject, Sendable {
    func onFrame(frame: DocFrame)
    /// Rows that may have changed (re-read them with `layers()`).
    func onLayersChanged(layerIds: [DocLayerID])
    func onHistoryChanged(head: DocHistoryID)
    func onRenderFailed(message: String)
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

/// `DocumentSession`, call for call (B5-01 `crates/tessera-ffi/src/document.rs`). Every edit is one
/// `DocOp` (one history node) unless it says `interactive`: interactive calls re-render without a
/// history node until `commit(label)`.
public protocol DocumentBackend: AnyObject, Sendable {
    func id() -> String

    // Model reads
    func info() throws -> DocumentSummary
    func layers() throws -> [LayerRecord]
    func layer(id: DocLayerID) throws -> LayerRecord
    /// The Layers panel's selection, kept in the document (reported by `info().selectedLayerIds`).
    func setSelectedLayers(ids: [DocLayerID]) throws
    /// Writes an RGBA8 IOSurface (straight alpha, at most `maxPx` on the long edge), cached per
    /// layer revision, and returns its `IOSurfaceID`. The backend keeps the surface alive.
    func layerThumbnail(id: DocLayerID, maxPx: UInt32) throws -> UInt32
    /// The layer mask as a grey RGBA8 IOSurface, cached per revision.
    func maskThumbnail(id: DocLayerID, maxPx: UInt32) throws -> UInt32
    func compositeThumbnail(maxPx: UInt32) throws -> UInt32

    // Edits
    func addLayer(kind: NewLayerKind, name: String, parent: DocLayerID?, index: UInt32?) throws -> DocumentChange
    func duplicateLayer(id: DocLayerID) throws -> DocumentChange
    func removeLayer(id: DocLayerID) throws -> DocumentChange
    /// Removes `id` from its parent, then inserts it at compositor `index` (0 = bottom) of `parent`.
    func moveLayer(id: DocLayerID, parent: DocLayerID?, index: UInt32) throws -> DocumentChange
    func setProps(id: DocLayerID, props: LayerProperties) throws -> DocumentChange
    func renameLayer(id: DocLayerID, name: String) throws -> DocumentChange
    func setVisible(id: DocLayerID, visible: Bool) throws -> DocumentChange
    func setOpacity(id: DocLayerID, value: Float, interactive: Bool) throws -> DocumentChange
    func setFillOpacity(id: DocLayerID, value: Float, interactive: Bool) throws -> DocumentChange
    func setBlendMode(id: DocLayerID, mode: String) throws -> DocumentChange
    func setGroupMode(id: DocLayerID, mode: LayerGroupMode) throws -> DocumentChange
    func setLocks(id: DocLayerID, locks: LayerLockFlags) throws -> DocumentChange
    func setAdjustmentJson(id: DocLayerID, json: String, interactive: Bool) throws -> DocumentChange
    func setFillJson(id: DocLayerID, json: String, interactive: Bool) throws -> DocumentChange
    func addMask(id: DocLayerID, mask: LayerMaskInit) throws -> DocumentChange
    func removeMask(id: DocLayerID) throws -> DocumentChange
    func setMaskEnabled(id: DocLayerID, enabled: Bool) throws -> DocumentChange
    func setMaskDensity(id: DocLayerID, density: Float) throws -> DocumentChange
    /// Session state only (not a history node).
    func setMaskLinked(id: DocLayerID, linked: Bool) throws
    func setClipped(id: DocLayerID, clipped: Bool) throws -> DocumentChange
    func mergeDown(id: DocLayerID) throws -> DocumentChange
    /// Every visible layer into one opaque Background layer.
    func flatten() throws -> DocumentChange
    /// Sibling layers into a new group at the topmost one's position (one history node).
    func groupLayers(ids: [DocLayerID], name: String) throws -> DocumentChange
    /// Replaces a group by its children.
    func ungroupLayer(id: DocLayerID) throws -> DocumentChange
    /// The rectangular marquee in level-0 pixels (a history node, like Photoshop's).
    func setSelectionRect(x: Int64, y: Int64, width: Int64, height: Int64, feather: Float) throws -> DocumentChange
    func clearSelection() throws -> DocumentChange
    /// Records the pending interactive edits as one history node (nothing without pending edits).
    func commit(label: String) throws -> DocumentChange

    // History
    func undo() throws -> DocumentChange
    func redo() throws -> DocumentChange
    func historyItems() throws -> [DocHistoryEntry]
    /// Any retained state; 0 = as opened.
    func checkoutHistory(id: DocHistoryID) throws -> DocumentChange
    func snapshot(name: String) throws
    func snapshots() throws -> [String]
    func restoreSnapshot(name: String) throws -> DocumentChange
    func setMaxStates(maxStates: UInt32) throws
    /// Bytes held by the retained history.
    func historyMemoryBytes() throws -> UInt64

    // Presentation
    func setListener(listener: (any DocumentBackendListener)?)
    func planSurface(width: UInt32, height: UInt32) throws -> DocViewportPlan
    func attachSurface(iosurfaceId: UInt32, width: UInt32, height: UInt32) throws
    /// Shows `width × height` pixels at `(x, y)` of pyramid `level` (level coordinates) top-left in
    /// the surfaces, clipped to the level and the surface size; `zoom` (1 = 100 %) is echoed in frames.
    func setViewport(level: UInt8, x: UInt32, y: UInt32, width: UInt32, height: UInt32, zoom: Double) throws
    func setDisplayHeadroom(headroom: Float) throws
    func refresh() throws
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
