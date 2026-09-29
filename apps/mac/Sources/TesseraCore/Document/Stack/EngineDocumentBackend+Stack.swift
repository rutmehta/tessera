import Foundation
import TesseraFFI

// `EngineDocumentBackend` / `EngineDocumentEngine` as stack backends (WP B5-19): each call is the session
// or engine call of the same name, settings converted field by field.

extension StackAlignLayout {
    init(_ m: StackAlignMode) {
        switch m {
        case .auto: self = .auto
        case .perspective: self = .perspective
        case .cylindrical: self = .cylindrical
        case .spherical: self = .spherical
        case .collage: self = .collage
        case .reposition: self = .reposition
        }
    }
    var ffi: StackAlignMode {
        switch self {
        case .auto: .auto
        case .perspective: .perspective
        case .cylindrical: .cylindrical
        case .spherical: .spherical
        case .collage: .collage
        case .reposition: .reposition
        }
    }
}

extension StackAlignSettings {
    /// No lens calibration is sent (`StackCommandRules.lensCorrectionAvailable`).
    var ffi: StackAlignOptions {
        StackAlignOptions(mode: layout.ffi, referenceIndex: referenceIndex, vignetteRemoval: vignetteRemoval,
                          geometricDistortion: geometricDistortion, lensCorrections: [], seed: seed)
    }
}

extension StackBlendSettings {
    var ffi: StackBlendOptions {
        StackBlendOptions(mode: method == .panorama ? .panorama : .stackImages, seamlessTones: seamlessTones,
                          contentAwareFill: contentAwareFill, seed: seed)
    }
}

extension EngineDocumentBackend: DocumentStackBackend {
    public func stackEligibility(ids: [DocLayerID]) throws -> StackEligibilityInfo {
        let e = try bridged { try session.stackEligibility(ids: ids) }
        return StackEligibilityInfo(canAlign: e.canAlign, canBlend: e.canBlend, reason: e.reason)
    }

    public func autoAlignLayers(ids: [DocLayerID], options: StackAlignSettings) throws -> DocumentChange {
        try change { try session.autoAlignLayers(ids: ids, options: options.ffi) }
    }

    public func autoBlendLayers(ids: [DocLayerID], options: StackBlendSettings) throws -> DocumentChange {
        try change { try session.autoBlendLayers(ids: ids, options: options.ffi) }
    }

    public func photomergeIntoLayers(sources: [String], align: StackAlignSettings, blend: StackBlendSettings) throws
        -> DocumentChange {
        try change { try session.photomergeIntoLayers(sources: sources, align: align.ffi, blend: blend.ffi) }
    }
}

extension EngineDocumentEngine: DocumentStackEngine {
    /// Blocking (decodes, aligns and blends every photo): call off the main thread.
    public func photomergeDocument(sources: [String], align: StackAlignSettings, blend: StackBlendSettings) throws
        -> any DocumentBackend {
        let s = try bridged { try engine.photomergeDocument(sources: sources, align: align.ffi, blend: blend.ffi) }
        return backend(for: s)
    }
}
