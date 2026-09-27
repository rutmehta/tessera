import Foundation
import TesseraFFI

// Editable text layers (WP B5-10): a protocol next to `DocumentBackend`, `DocumentToolsBackend` and
// `DocumentChannelsBackend`, adopted by `EngineDocumentBackend` over crates/tessera-ffi/src/document/text.rs.
// Records carry TesseraCore names distinct from the FFI's: `TextLayerSource` ⇄ `TextLayerRecord`,
// `TextFontFamilyInfo` ⇄ `TextFontFamily`, `TextLayoutInfo` ⇄ `TextLayoutRecord`.

/// A text layer's live source (`text_layer`).
public struct TextLayerSource: Equatable, Sendable {
    public var id: DocLayerID
    public var model: TextSourceModel
    /// Local level-0 pixels → document pixels, row-major `[a, b, c, d, e, f]`.
    public var transform: AffineTransform2D
    /// Committed content revision (pass back as the expected revision).
    public var revision: UInt64
    public var draftPending: Bool
    /// Point / paragraph text without warp, path or vertical data.
    public var caretEditable: Bool
    public var limitations: [String]
    public init(id: DocLayerID, model: TextSourceModel, transform: AffineTransform2D, revision: UInt64,
                draftPending: Bool = false, caretEditable: Bool = true, limitations: [String] = []) {
        self.id = id; self.model = model; self.transform = transform; self.revision = revision
        self.draftPending = draftPending; self.caretEditable = caretEditable; self.limitations = limitations
    }
}

public struct TextFontFaceInfo: Equatable, Sendable, Hashable {
    public var postScriptName: String
    public var weight: UInt16
    public var italic: Bool
    public init(postScriptName: String, weight: UInt16, italic: Bool) {
        self.postScriptName = postScriptName; self.weight = weight; self.italic = italic
    }
    /// "Bold Italic", "Regular", …
    public var styleName: String {
        let w: String = switch weight {
        case ..<150: "Thin"
        case ..<250: "Extra Light"
        case ..<350: "Light"
        case ..<450: "Regular"
        case ..<550: "Medium"
        case ..<650: "Semibold"
        case ..<750: "Bold"
        case ..<850: "Extra Bold"
        default: "Black"
        }
        if italic { return w == "Regular" ? "Italic" : "\(w) Italic" }
        return w
    }
}

public struct TextFontFamilyInfo: Equatable, Sendable, Identifiable {
    public var family: String
    public var faces: [TextFontFaceInfo]
    public var id: String { family }
    public init(family: String, faces: [TextFontFaceInfo]) { self.family = family; self.faces = faces }
}

/// The session's B5-10 calls. Drafts (`interactive`) record nothing; final calls and `commitDraft`
/// record one node; `cancelSourcePreview` drops a draft with no history change.
public protocol DocumentTextBackend: AnyObject, Sendable {
    func textLayer(id: DocLayerID) throws -> TextLayerSource
    func availableTextFonts() -> [TextFontFamilyInfo]
    /// The engine's layout (the caret oracle) of `model` over the renderers' font snapshot.
    func layoutText(_ model: TextSourceModel) throws -> TextLayoutInfo
    func addTextLayer(name: String, parent: DocLayerID?, index: UInt32?, model: TextSourceModel,
                      transform: AffineTransform2D, interactive: Bool) throws -> DocumentChange
    func setTextLayer(id: DocLayerID, model: TextSourceModel, transform: AffineTransform2D, interactive: Bool,
                      expectedRevision: UInt64?) throws -> DocumentChange
    func editTextRuns(id: DocLayerID, runs: Range<Int>, with replacement: [TextRunModel],
                      expectedRevision: UInt64?) throws -> DocumentChange
    func cancelSourcePreview() throws -> DocumentChange
    func convertToPixels(id: DocLayerID) throws -> DocumentChange
}

// MARK: - Engine adoption

extension TextLayoutInfo {
    init(_ r: TextLayoutRecord) {
        self.init(glyphs: r.glyphs.map {
            TextGlyphInfo(run: Int($0.run), cluster: Int($0.cluster), x: Double($0.x), y: Double($0.y),
                          advance: Double($0.advance), angle: Double($0.angle), rtl: $0.rtl)
        }, lines: r.lines.map {
            TextLineInfo(source: Int($0.sourceStart)..<Int(max($0.sourceStart, $0.sourceEnd)),
                         glyphs: Int($0.glyphStart)..<Int(max($0.glyphStart, $0.glyphEnd)), x: Double($0.x),
                         baseline: Double($0.baseline), width: Double($0.width),
                         availableWidth: $0.availableWidth.map(Double.init), ascent: Double($0.ascent),
                         descent: Double($0.descent))
        }, overflow: r.overflow, textLength: Int(r.textLen))
    }
}

extension AffineTransform2D {
    init(_ m: TransformMatrix) { self.init(a: m.a, b: m.b, c: m.c, d: m.d, e: m.e, f: m.f) }
}

public enum TextBridge {
    /// Encodes runs for `edit_text_runs`.
    static func runsJSON(_ runs: [TextRunModel]) -> String {
        let e = JSONEncoder()
        e.outputFormatting = [.sortedKeys]
        return (try? String(decoding: e.encode(runs), as: UTF8.self)) ?? "[]"
    }

    public static func fonts() -> [TextFontFamilyInfo] {
        TesseraFFI.availableTextFonts().map { f in
            TextFontFamilyInfo(family: f.family, faces: f.faces.map {
                TextFontFaceInfo(postScriptName: $0.postScriptName, weight: $0.weight, italic: $0.italic)
            })
        }
    }

    public static func layout(_ model: TextSourceModel) throws -> TextLayoutInfo {
        try bridged { TextLayoutInfo(try layoutText(modelJson: model.json)) }
    }
}

extension EngineDocumentBackend: DocumentTextBackend {
    public func textLayer(id: DocLayerID) throws -> TextLayerSource {
        let r = try bridged { try session.textLayer(layer: id) }
        guard let m = TextSourceModel(json: r.modelJson) else { throw DocumentError.invalid("text layer \(id): unreadable model") }
        return TextLayerSource(id: r.id, model: m, transform: AffineTransform2D(r.transform), revision: r.revision,
                               draftPending: r.draftPending, caretEditable: r.caretEditable, limitations: r.limitations)
    }

    public func availableTextFonts() -> [TextFontFamilyInfo] { TextBridge.fonts() }

    public func layoutText(_ model: TextSourceModel) throws -> TextLayoutInfo { try TextBridge.layout(model) }

    public func addTextLayer(name: String, parent: DocLayerID?, index: UInt32?, model: TextSourceModel,
                             transform: AffineTransform2D, interactive: Bool) throws -> DocumentChange {
        try change {
            try session.addTextLayer(name: name, parent: parent, index: index, modelJson: model.json, transform: transform.ffi,
                                     interactive: interactive)
        }
    }

    public func setTextLayer(id: DocLayerID, model: TextSourceModel, transform: AffineTransform2D, interactive: Bool,
                             expectedRevision: UInt64?) throws -> DocumentChange {
        try change {
            try session.setTextLayer(layer: id, modelJson: model.json, transform: transform.ffi, interactive: interactive,
                                     expectedRevision: expectedRevision)
        }
    }

    public func editTextRuns(id: DocLayerID, runs: Range<Int>, with replacement: [TextRunModel],
                             expectedRevision: UInt64?) throws -> DocumentChange {
        try change {
            try session.editTextRuns(layer: id, startRun: UInt32(runs.lowerBound), endRun: UInt32(runs.upperBound),
                                     runsJson: TextBridge.runsJSON(replacement), expectedRevision: expectedRevision)
        }
    }

    public func cancelSourcePreview() throws -> DocumentChange { try change { try session.cancelSourcePreview() } }

    public func convertToPixels(id: DocLayerID) throws -> DocumentChange { try change { try session.convertToPixels(layer: id) } }
}
