import CoreGraphics
import Foundation
import TesseraFFI

// Warp, Perspective Warp, Puppet Warp and Content-Aware Scale (WP B5-12): a protocol next to
// `DocumentBackend`, adopted by `EngineDocumentBackend` over crates/tessera-ffi/src/document/transform.rs.
// Records carry TesseraCore names distinct from the FFI's: `TransformStageInfo` ⇄ `TransformStageRecord`,
// `AdvancedTransformStart` ⇄ `AdvancedTransformInfo`, `PuppetMeshInfo` ⇄ `PuppetMeshRecord`,
// `AdvancedTransformTag` ⇄ `AdvancedTransformKind`.

/// One `transform` stage of a smart object's filter stack.
public struct TransformStageInfo: Equatable, Sendable {
    public var layer: DocLayerID
    public var index: UInt32
    public var kind: AdvancedTransformTag
    /// `TransformOp` JSON (a protect mask replaced by null).
    public var json: String
    public var hasProtection: Bool
    public var enabled: Bool
    public var blendMode: String
    public var opacity: Float
    public init(layer: DocLayerID, index: UInt32, kind: AdvancedTransformTag, json: String, hasProtection: Bool,
                enabled: Bool, blendMode: String, opacity: Float) {
        self.layer = layer; self.index = index; self.kind = kind; self.json = json; self.hasProtection = hasProtection
        self.enabled = enabled; self.blendMode = blendMode; self.opacity = opacity
    }
}

/// What `begin_advanced_transform` captured.
public struct AdvancedTransformStart: Equatable, Sendable {
    public var token: UInt64
    public var layer: DocLayerID
    public var layerKind: LayerKindTag
    public var kind: AdvancedTransformTag
    public var editingIndex: UInt32?
    public var insertIndex: UInt32
    /// Apply wraps the layer in a smart object: ask first.
    public var needsConversion: Bool
    public var childWidth: UInt32
    public var childHeight: UInt32
    public var childToDocument: AffineTransform2D
    /// Content bounds in child pixels.
    public var contentBounds: CanvasRect?
    public var existing: TransformStageInfo?
    public var revision: UInt64
    /// Pyramid level of drag drafts (0: every preview is exact).
    public var draftLevel: UInt8
    public var otherStages: UInt32
    public var limitations: [String]
    public init(token: UInt64, layer: DocLayerID, layerKind: LayerKindTag, kind: AdvancedTransformTag, editingIndex: UInt32?,
                insertIndex: UInt32, needsConversion: Bool, childWidth: UInt32, childHeight: UInt32,
                childToDocument: AffineTransform2D, contentBounds: CanvasRect?, existing: TransformStageInfo?, revision: UInt64,
                draftLevel: UInt8, otherStages: UInt32, limitations: [String]) {
        self.token = token; self.layer = layer; self.layerKind = layerKind; self.kind = kind; self.editingIndex = editingIndex
        self.insertIndex = insertIndex; self.needsConversion = needsConversion; self.childWidth = childWidth
        self.childHeight = childHeight; self.childToDocument = childToDocument; self.contentBounds = contentBounds
        self.existing = existing; self.revision = revision; self.draftLevel = draftLevel; self.otherStages = otherStages
        self.limitations = limitations
    }

    public var mapping: ChildMapping { ChildMapping(childToDocument) }
    public var childRect: CGRect { CGRect(x: 0, y: 0, width: CGFloat(childWidth), height: CGFloat(childHeight)) }
    /// Content bounds (child pixels), or the child canvas.
    public var contentRect: CGRect {
        guard let b = contentBounds, b.width > 0, b.height > 0 else { return childRect }
        return CGRect(x: CGFloat(b.x), y: CGFloat(b.y), width: CGFloat(b.width), height: CGFloat(b.height))
    }
}

/// A preview's frame request plus, for puppet warp, the solved (deformed) vertices.
public struct AdvancedTransformPreviewResult: Equatable, Sendable {
    public var change: DocumentChange
    public var deformed: [CGPoint]?
    public init(change: DocumentChange, deformed: [CGPoint]?) { self.change = change; self.deformed = deformed }
}

public struct PuppetMeshInfo: Equatable, Sendable {
    public var mesh: PuppetModel
    public var vertexCount: UInt32
    public var triangleCount: UInt32
    public var cellPx: UInt32
    public var level: UInt8
    public var note: String?
}

/// A saved alpha channel offered for content-aware protection.
public struct ProtectionChannel: Equatable, Sendable, Identifiable, Hashable {
    public var id: UInt64
    public var name: String
    public init(id: UInt64, name: String) { self.id = id; self.name = name }
}

/// The session's B5-12 calls. Previews record nothing; commit records ONE node; cancel none.
public protocol DocumentTransformsBackend: AnyObject, Sendable {
    func transformStages(layer: DocLayerID) throws -> [TransformStageInfo]
    func beginAdvancedTransform(layer: DocLayerID, index: UInt32?, kind: AdvancedTransformTag) throws -> AdvancedTransformStart
    func previewAdvancedTransform(token: UInt64, json: String, draft: Bool) throws -> AdvancedTransformPreviewResult
    func contentAwareScale(token: UInt64, width: UInt32, height: UInt32, amount: Float, protect: UInt64?, draft: Bool) throws -> DocumentChange
    /// `convert`: the user agreed to wrap a non-smart layer in a smart object.
    func commitAdvancedTransform(token: UInt64, convert: Bool) throws -> DocumentChange
    func cancelAdvancedTransform(token: UInt64) throws -> DocumentChange
    func puppetMesh(layer: DocLayerID, density: PuppetDensityTag, expansion: UInt32) throws -> PuppetMeshInfo
    func protectionChannels() -> [ProtectionChannel]
    /// A PSD / PSB copy with smart filter and transform stacks rasterized (the session is unchanged).
    func savePSDRasterizingTransforms(path: String) throws
}

// MARK: - Engine adoption

extension AdvancedTransformTag {
    init(_ k: AdvancedTransformKind) {
        switch k {
        case .warp: self = .warp
        case .perspective: self = .perspective
        case .puppet: self = .puppet
        case .contentAwareScale: self = .contentAwareScale
        case .free: self = .free
        case .displacement: self = .displacement
        }
    }

    var ffi: AdvancedTransformKind {
        switch self {
        case .warp: .warp
        case .perspective: .perspective
        case .puppet: .puppet
        case .contentAwareScale: .contentAwareScale
        case .free: .free
        case .displacement: .displacement
        }
    }
}

extension TransformStageInfo {
    init(_ r: TransformStageRecord) {
        self.init(layer: r.layer, index: r.index, kind: AdvancedTransformTag(r.kind), json: r.transformJson,
                  hasProtection: r.hasProtection, enabled: r.enabled, blendMode: r.blendMode, opacity: r.opacity)
    }
}

/// Free engine helpers (no session): warp presets, splits and grids.
public enum TransformBridge {
    public static func presetNames() -> [String] { TesseraFFI.warpPresetNames() }

    public static func preset(width: Double, height: Double, name: String, bend: Double) throws -> WarpMeshModel {
        let json = try bridged { try TesseraFFI.warpPreset(width: width, height: height, preset: name, bend: bend) }
        guard let m = WarpMeshModel(json: json) else { throw DocumentError.invalid("warp preset: unreadable mesh") }
        return m
    }

    /// Exact de Casteljau split through child point `p` (the surface does not move).
    public static func split(_ mesh: WarpMeshModel, at p: CGPoint, vertical: Bool, horizontal: Bool) throws -> WarpMeshModel {
        let json = try bridged {
            try TesseraFFI.warpSplit(meshJson: mesh.json, x: Double(p.x), y: Double(p.y), splitU: vertical, splitV: horizontal)
        }
        guard let m = WarpMeshModel(json: json) else { throw DocumentError.invalid("warp split: unreadable mesh") }
        return m
    }

    public static func subdivide(_ mesh: WarpMeshModel, columns: UInt32, rows: UInt32) throws -> WarpMeshModel {
        let json = try bridged { try TesseraFFI.warpSubdivide(meshJson: mesh.json, columns: columns, rows: rows) }
        guard let m = WarpMeshModel(json: json) else { throw DocumentError.invalid("warp grid: unreadable mesh") }
        return m
    }
}

extension EngineDocumentBackend: DocumentTransformsBackend {
    public func transformStages(layer: DocLayerID) throws -> [TransformStageInfo] {
        try bridged { try session.transformStages(layer: layer) }.map(TransformStageInfo.init)
    }

    public func beginAdvancedTransform(layer: DocLayerID, index: UInt32?, kind: AdvancedTransformTag) throws -> AdvancedTransformStart {
        let r = try bridged { try session.beginAdvancedTransform(layer: layer, index: index, kind: kind.ffi) }
        return AdvancedTransformStart(
            token: r.token, layer: r.layer, layerKind: LayerKindTag(r.layerKind), kind: AdvancedTransformTag(r.kind),
            editingIndex: r.editingIndex, insertIndex: r.insertIndex, needsConversion: r.needsConversion,
            childWidth: r.childWidth, childHeight: r.childHeight, childToDocument: AffineTransform2D(r.childToDocument),
            contentBounds: r.contentBounds.map(CanvasRect.init), existing: r.existing.map(TransformStageInfo.init),
            revision: r.revision, draftLevel: r.draftLevel, otherStages: r.otherStages, limitations: r.limitations)
    }

    public func previewAdvancedTransform(token: UInt64, json: String, draft: Bool) throws -> AdvancedTransformPreviewResult {
        var deformed: String?
        let c = try change {
            let p = try session.previewAdvancedTransform(token: token, transformJson: json, draft: draft)
            deformed = p.deformedJson
            return p.update
        }
        let points = deformed.flatMap { try? JSONDecoder().decode([[Double]].self, from: Data($0.utf8)) }?
            .map { CGPoint(x: CGFloat($0[0]), y: CGFloat($0[1])) }
        return AdvancedTransformPreviewResult(change: c, deformed: points)
    }

    public func contentAwareScale(token: UInt64, width: UInt32, height: UInt32, amount: Float, protect: UInt64?,
                                  draft: Bool) throws -> DocumentChange {
        try change {
            try session.contentAwareScaleFromChannel(token: token, targetWidth: width, targetHeight: height, amount: amount,
                                                     channelId: protect, draft: draft).update
        }
    }

    public func commitAdvancedTransform(token: UInt64, convert: Bool) throws -> DocumentChange {
        try change { try session.commitAdvancedTransform(token: token, convertToSmartObject: convert) }
    }

    public func cancelAdvancedTransform(token: UInt64) throws -> DocumentChange {
        try change { try session.cancelAdvancedTransform(token: token) }
    }

    public func puppetMesh(layer: DocLayerID, density: PuppetDensityTag, expansion: UInt32) throws -> PuppetMeshInfo {
        let r = try bridged { try session.puppetMeshFromLayer(layer: layer, density: density.rawValue, expansion: expansion) }
        guard let mesh = PuppetModel(json: r.meshJson) else { throw DocumentError.invalid("puppet mesh: unreadable") }
        return PuppetMeshInfo(mesh: mesh, vertexCount: r.vertexCount, triangleCount: r.triangleCount, cellPx: r.cellPx,
                              level: r.level, note: r.note)
    }

    public func protectionChannels() -> [ProtectionChannel] {
        ((try? session.documentChannels()) ?? []).filter { $0.kind == .alpha }.map { ProtectionChannel(id: $0.id, name: $0.name) }
    }

    public func savePSDRasterizingTransforms(path: String) throws {
        try bridged { try session.savePsdRasterizingTransforms(path: path) }
    }
}
