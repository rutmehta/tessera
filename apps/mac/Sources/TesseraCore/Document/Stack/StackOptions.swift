import Foundation

// Auto-Align Layers, Auto-Blend Layers and Photomerge (WP B5-19): the pure models behind the sheets and
// the menu enablement. The engine adapter maps them onto the FFI records of
// crates/tessera-ffi/src/document/stack.rs (EngineDocumentBackend+Stack.swift); the rules here repeat
// the session's checks in the same words so menus disable and sheets explain before any engine call.

/// Photoshop's Layout / Projection choices.
public enum StackAlignLayout: String, CaseIterable, Sendable, Identifiable {
    case auto, perspective, cylindrical, spherical, collage, reposition
    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .auto: "Auto"
        case .perspective: "Perspective"
        case .cylindrical: "Cylindrical"
        case .spherical: "Spherical"
        case .collage: "Collage"
        case .reposition: "Reposition"
        }
    }

    public var help: String {
        switch self {
        case .auto: "Picks the projection that fits the photos best."
        case .perspective: "Keeps the reference flat and stretches the others to match."
        case .cylindrical: "Wraps a wide panorama around a cylinder (reduces bow-tie distortion)."
        case .spherical: "Maps the photos onto a sphere, for very wide or tall panoramas."
        case .collage: "Moves, rotates and scales the photos without distorting them."
        case .reposition: "Moves the photos only."
        }
    }
}

/// Auto-Blend Layers methods.
public enum StackBlendMethod: String, CaseIterable, Sendable, Identifiable {
    case panorama, stackImages
    public var id: String { rawValue }
    public var title: String { self == .panorama ? "Panorama" : "Stack Images" }
    public var help: String {
        self == .panorama ? "Hides seams between overlapping photos with layer masks."
            : "Keeps the sharpest parts of each layer (focus stacking) with layer masks."
    }
}

public struct StackAlignSettings: Equatable, Sendable {
    public var layout: StackAlignLayout
    /// Index into the layers (bottom first) or photos of the one that stays put.
    public var referenceIndex: UInt32
    public var vignetteRemoval: Bool
    public var geometricDistortion: Bool
    public var seed: UInt64
    public init(layout: StackAlignLayout = .auto, referenceIndex: UInt32 = 0, vignetteRemoval: Bool = false,
                geometricDistortion: Bool = false, seed: UInt64 = 1) {
        self.layout = layout; self.referenceIndex = referenceIndex; self.vignetteRemoval = vignetteRemoval
        self.geometricDistortion = geometricDistortion; self.seed = seed
    }
}

public struct StackBlendSettings: Equatable, Sendable {
    public var method: StackBlendMethod
    public var seamlessTones: Bool
    /// Content-Aware Fill Transparent Areas.
    public var contentAwareFill: Bool
    public var seed: UInt64
    public init(method: StackBlendMethod = .panorama, seamlessTones: Bool = true, contentAwareFill: Bool = false,
                seed: UInt64 = 1) {
        self.method = method; self.seamlessTones = seamlessTones; self.contentAwareFill = contentAwareFill; self.seed = seed
    }
}

/// The session's answer for a Layers selection.
public struct StackEligibilityInfo: Equatable, Sendable {
    public var canAlign: Bool
    public var canBlend: Bool
    public var reason: String?
    public init(canAlign: Bool, canBlend: Bool, reason: String?) {
        self.canAlign = canAlign; self.canBlend = canBlend; self.reason = reason
    }
}

// MARK: - Menu rules

public enum StackCommandRules {
    public static let maxLayers = 128

    /// Lens profiles are not mapped to calibrations yet, so the lens toggles stay off.
    public static let lensCorrectionAvailable = false
    public static let lensCorrectionNote =
        "Vignette removal and geometric distortion correction need a lens calibration for every photo; "
        + "library lens profiles are not available in document mode yet."
    /// Alignment and blending run in the engine without a cancellation point yet.
    public static let busyNote = "This can take a while for large photos and cannot be cancelled."

    private enum Stage { case align, blend }

    private static func problem(_ layers: [LayerRecord], _ selected: [DocLayerID], _ stage: Stage) -> String? {
        let (verb, title) = stage == .align ? ("align", "Auto-Align") : ("blend", "Auto-Blend")
        if selected.count < 2 { return "Select two or more layers to \(verb)" }
        if selected.count > maxLayers { return "\(title) takes at most \(maxLayers) layers" }
        for (i, id) in selected.enumerated() {
            if selected[..<i].contains(id) { return "layer \(id) is selected twice" }
            guard let l = layers.first(where: { $0.id == id }) else { return "layer \(id) not found" }
            if l.parent != nil { return "\(title) works on top-level layers; “\(l.name)” is inside a group" }
            if l.locks.all || l.locks.pixels || l.locks.position { return "“\(l.name)” is locked" }
            if l.clipped { return "“\(l.name)” is part of a clipping mask" }
            let ok = l.kind == .pixel || (stage == .blend && l.kind == .smartObject)
            if !ok { return "\(title) needs pixel layers; “\(l.name)” is not a pixel layer" }
        }
        return nil
    }

    public static func alignProblem(layers: [LayerRecord], selected: [DocLayerID]) -> String? {
        problem(layers, selected, .align)
    }

    public static func blendProblem(layers: [LayerRecord], selected: [DocLayerID]) -> String? {
        problem(layers, selected, .blend)
    }

    public static func canAutoAlign(layers: [LayerRecord], selected: [DocLayerID]) -> Bool {
        alignProblem(layers: layers, selected: selected) == nil
    }

    public static func canAutoBlend(layers: [LayerRecord], selected: [DocLayerID]) -> Bool {
        blendProblem(layers: layers, selected: selected) == nil
    }

    /// `selected` in stack order, bottom first (`layers` is the Layers panel order, top first), so the
    /// default reference is the bottom layer.
    public static func orderedIDs(layers: [LayerRecord], selected: [DocLayerID]) -> [DocLayerID] {
        let set = Set(selected)
        return layers.filter { set.contains($0.id) }.map(\.id).reversed()
    }
}

// MARK: - Photomerge form

/// One photo to merge: a library image (by engine id) or a file.
public enum PhotomergeSource: Hashable, Sendable, Identifiable {
    case image(id: String, name: String)
    case file(URL)

    public var id: String { argument }

    /// What the session takes: the image id or the file path.
    public var argument: String {
        switch self {
        case .image(let id, _): id
        case .file(let url): url.path
        }
    }

    public var title: String {
        switch self {
        case .image(_, let name): name
        case .file(let url): url.lastPathComponent
        }
    }
}

/// File ▸ Automate ▸ Photomerge…: the photos, layout and blend options.
public struct PhotomergeForm: Equatable, Sendable {
    public var sources: [PhotomergeSource]
    public var layout: StackAlignLayout = .auto
    public var seamlessTones = true
    public var contentAwareFill = false
    /// Merge into the current document instead of a new one.
    public var intoCurrentDocument = false

    public init(sources: [PhotomergeSource] = []) { self.sources = sources }

    public var problem: String? {
        if sources.count < 2 { return "Choose two or more photos to merge" }
        if sources.count > StackCommandRules.maxLayers { return "Photomerge takes at most \(StackCommandRules.maxLayers) photos" }
        return nil
    }

    public var request: (sources: [String], align: StackAlignSettings, blend: StackBlendSettings)? {
        guard problem == nil else { return nil }
        return (sources.map(\.argument), StackAlignSettings(layout: layout),
                StackBlendSettings(method: .panorama, seamlessTones: seamlessTones, contentAwareFill: contentAwareFill))
    }

    /// Keeps the first of each repeated photo.
    public mutating func removeDuplicates() {
        var seen = Set<String>()
        sources = sources.filter { seen.insert($0.argument).inserted }
    }
}
