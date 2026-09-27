import Foundation

// Remove tool, Edit ▸ Content-Aware Fill, Remove Distractions review and Filter ▸ Neural Filters (WP B5-09):
// the third protocol document backends adopt. It mirrors the B5-09 calls of `DocumentSession`
// (crates/tessera-ffi/src/document/retouch.rs), which build masks engine-side from the selection or a
// Remove stroke. The UI-free state (options → JSON, suggestion review, neural sheet destinations, error
// presentation, outline request generations) lives here too and is unit tested (DocumentRetouchTests).

/// Which inpainting backend Remove asks for (FFI `RemoveBackend`).
public enum RemoveEngine: String, CaseIterable, Sendable, Identifiable {
    case auto, patchMatch, lama
    public var id: String { rawValue }
    public var title: String {
        switch self {
        case .auto: "Auto"
        case .patchMatch: "PatchMatch"
        case .lama: "LaMa"
        }
    }
    public var help: String {
        switch self {
        case .auto: "LaMa when its model is installed, PatchMatch otherwise"
        case .patchMatch: "Patch-based fill on the CPU; no model needed"
        case .lama: "The LaMa inpainting model (downloaded when you ask for it, if Settings ▸ AI allows model downloads)"
        }
    }
}

/// Neural filters M5-29 registers (FFI `NeuralFilterKind`).
public enum NeuralKind: String, CaseIterable, Sendable, Identifiable {
    case skinSmoothing, colorize, jpegArtifactRemoval
    public var id: String { rawValue }
    /// The adapter id stored in smart filters.
    public var filterId: String {
        switch self {
        case .skinSmoothing: "neural/skin_smoothing"
        case .colorize: "neural/colorize"
        case .jpegArtifactRemoval: "neural/jpeg_artifact_removal"
        }
    }
    public init?(filterId: String) {
        guard let k = NeuralKind.allCases.first(where: { $0.filterId == filterId }) else { return nil }
        self = k
    }
}

/// Where a neural filter's result goes (FFI `NeuralDestination`).
public enum NeuralOutput: String, CaseIterable, Sendable, Identifiable {
    case currentLayer, newLayer, smartFilter
    public var id: String { rawValue }
    public var title: String {
        switch self {
        case .currentLayer: "Current layer"
        case .newLayer: "New layer"
        case .smartFilter: "Smart filter"
        }
    }
}

/// One control of a neural filter.
public struct NeuralControl: Equatable, Sendable, Identifiable {
    public var key: String
    public var label: String
    public var min: Double
    public var max: Double
    public var defaultValue: Double
    public var id: String { key }
    /// Fixed at one value (the engine's "unavailable" controls): shown, not editable.
    public var isFixed: Bool { max <= min }
    public init(key: String, label: String, min: Double, max: Double, defaultValue: Double) {
        self.key = key; self.label = label; self.min = min; self.max = max; self.defaultValue = defaultValue
    }
}

/// A neural filter of the panel (FFI `NeuralFilterInfo`).
public struct NeuralFilterSpec: Equatable, Sendable, Identifiable {
    public var kind: NeuralKind
    public var name: String
    public var controls: [NeuralControl]
    public var requiresWeights: Bool
    public var limitation: String?
    public var id: NeuralKind { kind }
    public init(kind: NeuralKind, name: String, controls: [NeuralControl], requiresWeights: Bool, limitation: String?) {
        self.kind = kind; self.name = name; self.controls = controls; self.requiresWeights = requiresWeights
        self.limitation = limitation
    }
}

/// A model file and whether it is installed (FFI `RetouchModel`).
public struct RetouchModelInfo: Equatable, Sendable, Identifiable {
    public var modelId: String
    public var usedBy: String
    public var installed: Bool
    public var cachePath: String
    public var sourceURL: String
    /// Pinned registry version (what the model download asks for; B5-09b).
    public var version: String
    public var id: String { modelId }
    public init(modelId: String, usedBy: String, installed: Bool, cachePath: String, sourceURL: String, version: String = "") {
        self.modelId = modelId; self.usedBy = usedBy; self.installed = installed; self.cachePath = cachePath
        self.sourceURL = sourceURL; self.version = version
    }
}

/// What one apply did (FFI `RetouchResult`).
public struct RetouchOutcome: Equatable, Sendable {
    public var change: DocumentChange
    /// The backend that actually ran.
    public var backend: String
    public var note: String?
    public var millis: Double
    public init(change: DocumentChange, backend: String, note: String?, millis: Double) {
        self.change = change; self.backend = backend; self.note = note; self.millis = millis
    }
}

public enum DistractionCandidateKind: String, Sendable {
    /// A thin straight ridge or valley (geometric wire proxy).
    case wire
    /// A dilated face box (not a person mask).
    case faceBox
    public var title: String { self == .wire ? "Wire-like line" : "Face box" }
}

/// One suggestion of the distraction detector (FFI `DistractionSuggestion`).
public struct DistractionCandidate: Equatable, Sendable, Identifiable {
    public var id: UInt32
    public var kind: DistractionCandidateKind
    public var bounds: CanvasRect
    public var pixels: UInt64
    public init(id: UInt32, kind: DistractionCandidateKind, bounds: CanvasRect, pixels: UInt64) {
        self.id = id; self.kind = kind; self.bounds = bounds; self.pixels = pixels
    }
}

/// A detector run (FFI `DistractionScan`).
public struct DistractionScanResult: Equatable, Sendable {
    public var candidates: [DistractionCandidate]
    public var faces: String
    public var limitation: String
    public init(candidates: [DistractionCandidate], faces: String, limitation: String) {
        self.candidates = candidates; self.faces = faces; self.limitation = limitation
    }
}

public protocol DocumentRetouchBackend: AnyObject, Sendable {
    /// The neural filters with their controls (static catalogue).
    func neuralFilterSpecs() -> [NeuralFilterSpec]
    /// Model files and whether they are installed (reads the local cache only).
    func retouchModels() throws -> [RetouchModelInfo]
    /// Edit ▸ Content-Aware Fill on the selection (one node). Blocking.
    func contentAwareFill(layer: DocLayerID, paramsJson: String) throws -> RetouchOutcome
    /// Remove inside the selection (one node). Blocking.
    func removeSelection(layer: DocLayerID, engine: RemoveEngine, paramsJson: String) throws -> RetouchOutcome
    func beginRemoveStroke(layer: DocLayerID, size: Float, engine: RemoveEngine) throws
    /// Adds stroke points; returns the canvas rectangle the new dabs covered.
    func removeStrokePoints(_ points: [CanvasPoint]) throws -> CanvasRect?
    func cancelRemoveStroke()
    /// Removes what the stroke covered (one node). Blocking.
    func endRemoveStroke(paramsJson: String) throws -> RetouchOutcome
    /// Runs the detector and keeps its suggestions for review (no history).
    func detectDistractions(layer: DocLayerID, paramsJson: String) throws -> DistractionScanResult
    /// Removes the accepted suggestions (one node). Blocking.
    func removeDistractions(layer: DocLayerID, accepted: [UInt32], engine: RemoveEngine, paramsJson: String) throws -> RetouchOutcome
    func clearDistractions()
    /// A neural filter (one node). Blocking.
    func neuralFilter(layer: DocLayerID, kind: NeuralKind, paramsJson: String, output: NeuralOutput) throws -> RetouchOutcome
    /// Cancels a running retouch, neural or filter apply.
    func cancelRetouch()
}

// MARK: - Remove tool options

/// The Remove tool's options bar.
public struct RemoveToolOptions: Equatable, Sendable {
    public static let sizeRange: ClosedRange<Float> = 1...2000
    public var size: Float = 60
    public var engine: RemoveEngine = .auto
    /// Pixels the mask grows before filling (engine range 0…64).
    public var dilation: Int = 2
    public init() {}

    /// The Remove options JSON (`backend` is sent separately).
    public var paramsJson: String { "{\"dilation\":\(min(max(dilation, 0), 64))}" }

    public mutating func setSize(_ s: Float) { size = min(max(s.isFinite ? s : 60, Self.sizeRange.lowerBound), Self.sizeRange.upperBound) }

    /// `[` / `]` with the brush's steps.
    public mutating func bracket(larger: Bool) { setSize(BrushHUDMath.bracket(size: size, larger: larger)) }
}

// MARK: - Distraction review

/// Remove Distractions review: every suggestion starts accepted; the user unticks what should stay, then
/// applies only the accepted ones.
public struct DistractionReview: Equatable, Sendable {
    public let layer: DocLayerID
    public let scan: DistractionScanResult
    public private(set) var accepted: Set<UInt32>
    public init(layer: DocLayerID, scan: DistractionScanResult) {
        self.layer = layer
        self.scan = scan
        accepted = Set(scan.candidates.map(\.id))
    }

    public var candidates: [DistractionCandidate] { scan.candidates }
    public var acceptedIds: [UInt32] { scan.candidates.map(\.id).filter { accepted.contains($0) } }
    public var canApply: Bool { !accepted.isEmpty }
    public func isAccepted(_ id: UInt32) -> Bool { accepted.contains(id) }

    public mutating func toggle(_ id: UInt32) {
        guard scan.candidates.contains(where: { $0.id == id }) else { return }
        if accepted.contains(id) { accepted.remove(id) } else { accepted.insert(id) }
    }
    public mutating func setAll(_ on: Bool) { accepted = on ? Set(scan.candidates.map(\.id)) : [] }

    /// The topmost suggestion under a canvas point (smallest area first, so a wire inside a face box wins).
    public func hit(_ p: CanvasPoint) -> DistractionCandidate? {
        scan.candidates
            .filter { c in
                let b = c.bounds
                return Double(p.x) >= Double(b.x) && Double(p.x) < Double(b.x + b.width)
                    && Double(p.y) >= Double(b.y) && Double(p.y) < Double(b.y + b.height)
            }
            .min { $0.bounds.width * $0.bounds.height < $1.bounds.width * $1.bounds.height }
    }

    public var summary: String {
        let n = scan.candidates.count
        if n == 0 { return "Nothing found" }
        return "\(accepted.count) of \(n) suggestion\(n == 1 ? "" : "s") selected"
    }
}

// MARK: - Neural Filters sheet

/// The Neural Filters sheet's state: filter, values, output. Knows which outputs a layer allows.
public struct NeuralSheetState: Equatable, Sendable {
    public let specs: [NeuralFilterSpec]
    public let layerKind: LayerKindTag
    public let hasSelection: Bool
    /// Re-editing smart filter `index` (the output is fixed).
    public let smartIndex: UInt32?
    public var kind: NeuralKind
    public var values: [NeuralKind: [String: Double]]
    public var output: NeuralOutput

    public init(specs: [NeuralFilterSpec], layerKind: LayerKindTag, hasSelection: Bool, kind: NeuralKind? = nil,
                smartIndex: UInt32? = nil, filterJson: String? = nil) {
        self.specs = specs
        self.layerKind = layerKind
        self.hasSelection = hasSelection
        self.smartIndex = smartIndex
        self.kind = kind ?? specs.first?.kind ?? .skinSmoothing
        var v: [NeuralKind: [String: Double]] = [:]
        for s in specs { v[s.kind] = Dictionary(uniqueKeysWithValues: s.controls.map { ($0.key, $0.defaultValue) }) }
        if let json = filterJson, let params = NeuralSheetState.params(json), var cur = v[self.kind] {
            for (k, value) in params { if let d = value as? Double { cur[k] = d } else if let n = value as? NSNumber { cur[k] = n.doubleValue } }
            v[self.kind] = cur
            faces = params["faces"].flatMap(NeuralSheetState.faces)
        }
        values = v
        output = layerKind == .smartObject || smartIndex != nil ? .smartFilter : .currentLayer
        if !allowed(output) { output = NeuralOutput.allCases.first(where: allowed) ?? .currentLayer }
    }

    /// Face boxes of a re-edited smart filter (kept as they were).
    public private(set) var faces: [[Double]]?

    public var spec: NeuralFilterSpec? { specs.first { $0.kind == kind } }

    public func value(_ c: NeuralControl) -> Double { values[kind]?[c.key] ?? c.defaultValue }
    public mutating func set(_ c: NeuralControl, _ v: Double) {
        guard !c.isFixed else { return }
        values[kind, default: [:]][c.key] = min(max(v, c.min), c.max)
    }
    public mutating func reset() {
        if let s = spec { values[kind] = Dictionary(uniqueKeysWithValues: s.controls.map { ($0.key, $0.defaultValue) }) }
    }

    /// Outputs the layer allows: a new layer needs pixels; a smart filter from pixels needs no selection
    /// (the engine keeps smart retouch filters unmasked); re-editing keeps the smart filter.
    public func allowed(_ o: NeuralOutput) -> Bool {
        if smartIndex != nil { return o == .smartFilter }
        switch o {
        case .currentLayer: return layerKind == .pixel || layerKind == .smartObject
        case .newLayer: return layerKind == .pixel
        case .smartFilter: return layerKind == .smartObject ? !hasSelection : (layerKind == .pixel && !hasSelection)
        }
    }

    /// Why an output is unavailable (the picker's help tag).
    public func reason(_ o: NeuralOutput) -> String? {
        guard !allowed(o) else { return nil }
        if smartIndex != nil { return "Re-editing keeps the smart filter" }
        switch o {
        case .newLayer: return "New layer output needs a pixel layer"
        case .smartFilter: return hasSelection ? "Deselect first: smart retouch filters are not masked by a selection"
            : "Needs a pixel layer or a smart object"
        case .currentLayer: return "Needs a pixel layer or a smart object"
        }
    }

    /// The params JSON for the engine: the values (fixed controls omitted), plus kept face boxes.
    public var paramsJson: String {
        var obj: [String: Any] = [:]
        for c in spec?.controls ?? [] where !c.isFixed { obj[c.key] = value(c) }
        if kind == .skinSmoothing, let f = faces { obj["faces"] = f }
        let data = (try? JSONSerialization.data(withJSONObject: obj, options: [.sortedKeys])) ?? Data("{}".utf8)
        return String(decoding: data, as: UTF8.self)
    }

    /// The smart filter JSON for a re-edit (`{"id":…,"params":{…}}`).
    public var filterJson: String { "{\"id\":\"\(kind.filterId)\",\"params\":\(paramsJson)}" }

    static func params(_ filterJson: String) -> [String: Any]? {
        guard let o = try? JSONSerialization.jsonObject(with: Data(filterJson.utf8)) as? [String: Any] else { return nil }
        return o["params"] as? [String: Any] ?? o
    }

    static func faces(_ v: Any) -> [[Double]]? {
        guard let a = v as? [[Any]] else { return nil }
        let boxes = a.map { $0.compactMap { ($0 as? NSNumber)?.doubleValue } }.filter { $0.count == 4 }
        return boxes.isEmpty ? nil : boxes
    }
}

// MARK: - Error presentation

/// How a retouch error is shown: a short title, the engine's detail, and for missing weights the model
/// id and where its file comes from (no download is offered: files are installed by hand).
public struct RetouchErrorPresentation: Equatable, Sendable {
    public var title: String
    public var detail: String
    public var modelId: String?
    public var sourceURL: String?
    public var isMissingWeights: Bool { modelId != nil }

    public init(operation: String, message: String) {
        let m = message.hasPrefix("Invalid: ") ? String(message.dropFirst(9)) : message
        detail = m
        if m.contains("model weights, which are not installed") {
            modelId = RetouchErrorPresentation.token(after: "needs the ", in: m)
            sourceURL = RetouchErrorPresentation.token(after: "comes from ", in: m)
            title = "\(operation) needs a model that is not installed"
        } else if m.lowercased().contains("cancelled") {
            title = "\(operation) cancelled"
            detail = "Nothing was changed."
        } else if m.lowercased().contains("make a selection") || m.lowercased().contains("selection is empty") {
            title = "\(operation) needs a selection"
        } else {
            title = "\(operation) failed"
        }
    }

    public var isCancel: Bool { title.hasSuffix("cancelled") }

    /// One line for the status bar.
    public var statusLine: String {
        if let id = modelId { return "\(title): \(id) (see the message for where it comes from)" }
        return isCancel ? title : "\(title): \(detail)"
    }

    static func token(after marker: String, in s: String) -> String? {
        guard let r = s.range(of: marker) else { return nil }
        let rest = s[r.upperBound...]
        let tok = rest.prefix { !$0.isWhitespace }
        return tok.isEmpty ? nil : String(tok)
    }
}

// MARK: - Outline requests

/// Generation counter for asynchronous selection-outline requests: every refresh (including one that clears
/// the outline) starts a new generation, and a result is used only if its generation is still the latest.
/// Fixes stale marching ants when the selection is cleared while an outline request is in flight.
public struct OutlineRequestGate: Equatable, Sendable {
    public private(set) var generation: UInt64 = 0
    public init() {}
    /// A new request (or a synchronous clear): results of earlier requests are now stale.
    public mutating func begin() -> UInt64 {
        generation &+= 1
        return generation
    }
    public func accepts(_ g: UInt64) -> Bool { g == generation }
}
