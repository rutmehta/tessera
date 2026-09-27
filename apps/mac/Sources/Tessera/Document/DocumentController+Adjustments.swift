import Foundation
import Observation
import TesseraCore

/// Image-dependent adjustments (WP B5-06): Equalize, Auto, Match Color, Black & White Auto and HDR Toning's
/// Equalize Histogram freeze parameters computed from pixels (`AdjustmentAnalysis`). The pixels come from the
/// backend's thumbnails (256 px on the long side), which is plenty for histograms and Lab statistics.
extension DocumentController {
    static let analysisPixels: UInt32 = 256

    /// The composite's pixels. With `id`, that adjustment layer is set to its identity for the read (Photoshop
    /// analyses the image below an adjustment); the caller commits the new parameters right after.
    func compositeSamples(neutralizing id: DocLayerID? = nil) -> [SIMD3<Float>] {
        var restore: String?
        if let id, let json = node(id)?.adjustmentJson, AdjustmentModel(json: json) != nil {
            // Neutral Levels is an exact identity for every kind (a kind's own neutral need not be: Invert).
            if (try? backend.setAdjustmentJson(id: id, json: AdjustmentModel.Kind.levels.neutral.json, interactive: true)) != nil {
                restore = json
            }
        }
        let samples = (try? backend.compositeThumbnail(maxPx: Self.analysisPixels)).map(AdjustmentAnalysis.samples) ?? []
        if let id, let restore { _ = try? backend.setAdjustmentJson(id: id, json: restore, interactive: true) }
        return samples
    }

    /// One layer's own pixels (no mask, opacity or blending), as the engine's Match Color reads a source.
    func layerSamples(_ id: DocLayerID) -> [SIMD3<Float>] {
        (try? backend.layerThumbnail(id: id, maxPx: Self.analysisPixels)).map(AdjustmentAnalysis.samples) ?? []
    }

    /// Layers Match Color can take its source from (pixel content: pixel, smart object and text layers).
    var matchColorSources: [LayerRecord] {
        outline.flattened.compactMap { node($0) }.filter { [.pixel, .smartObject, .text].contains($0.kind) }
    }

    /// Match Color's default source: the selected pixel layer, else the lowest one.
    func defaultMatchSource(excluding: DocLayerID? = nil) -> DocLayerID? {
        let candidates = matchColorSources.filter { $0.id != excluding }
        if let p = primary, candidates.contains(where: { $0.id == p.id }) { return p.id }
        return candidates.last?.id
    }

    /// A new adjustment layer's parameters: neutral, or analysed from the composite for the image-dependent kinds.
    func initialAdjustment(_ kind: AdjustmentModel.Kind) -> AdjustmentModel {
        switch kind {
        case .equalize, .auto:
            return analyzed(kind.neutral, samples: compositeSamples())
        case .matchColor:
            guard let source = defaultMatchSource() else { return kind.neutral }
            return AdjustmentAnalysis.matchColor(sourceLayer: source, source: layerSamples(source),
                                                 target: compositeSamples(), neutralize: false).map { .matchColor($0) }
                ?? kind.neutral
        default:
            return kind.neutral
        }
    }

    /// `model` with its frozen parameters recomputed from `samples` (Auto keeps its mode, `clip` is its tail).
    func analyzed(_ model: AdjustmentModel, samples: [SIMD3<Float>], clip: Double = AdjustmentEditorState.defaultAutoClip) -> AdjustmentModel {
        guard !samples.isEmpty else { return model }
        switch model {
        case .equalize:
            return .equalize(maps: AdjustmentAnalysis.equalizeMaps(AdjustmentAnalysis.histograms(samples)))
        case .auto(let m):
            return .auto(AdjustmentAnalysis.auto(m.mode, histograms: AdjustmentAnalysis.histograms(samples), clip: clip))
        case .hdrToning(let m):
            return m.method == .equalizeHistogram ? .hdrToning(AdjustmentAnalysis.hdrEqualize(samples, base: m)) : model
        default:
            return model
        }
    }
}

/// Editor state that outlives an editor view (the Image ▸ Adjustments sheet rebuilds its editor on every
/// committed change): tone range, colour row, Auto clip and the file a Color Lookup was loaded from, per layer.
@MainActor @Observable
final class AdjustmentEditorState {
    static let shared = AdjustmentEditorState()
    /// Photoshop's Auto default: 0.1 % from each end.
    nonisolated static let defaultAutoClip = 0.001
    var colorBalanceRange: [DocLayerID: Int] = [:]
    var selectiveColorRow: [DocLayerID: Int] = [:]
    var autoClip: [DocLayerID: Double] = [:]
    /// The `.cube` / `.3dl` each Color Lookup layer was loaded from this session (the engine stores samples,
    /// not a file name; COMPOSITOR.md §4).
    var lookupFile: [String: String] = [:]
}
