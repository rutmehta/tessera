import Foundation
import TesseraFFI

// The stub backend (`--stub-library`, unit tests) and layer styles (WP B5-07): styles and the Global
// Light live in a side table per stub document, so the inspector, menus and Layers rows work; the stub
// compositor does not draw effects and style edits record no history.

/// Styles of stub documents, keyed by document.
final class StubStyleStore: @unchecked Sendable {
    static let shared = StubStyleStore()
    private let lock = NSLock()
    private var styles: [ObjectIdentifier: [DocLayerID: String]] = [:]
    private var lights: [ObjectIdentifier: DocGlobalLight] = [:]
    private var clipboard: String?

    func get(_ doc: ObjectIdentifier, _ layer: DocLayerID) -> String? { lock.lock(); defer { lock.unlock() }; return styles[doc]?[layer] }
    func set(_ doc: ObjectIdentifier, _ layer: DocLayerID, _ json: String?) {
        lock.lock(); defer { lock.unlock() }
        styles[doc, default: [:]][layer] = json
    }
    func all(_ doc: ObjectIdentifier) -> [DocLayerID: String] { lock.lock(); defer { lock.unlock() }; return styles[doc] ?? [:] }
    func light(_ doc: ObjectIdentifier) -> DocGlobalLight { lock.lock(); defer { lock.unlock() }; return lights[doc] ?? .default }
    func setLight(_ doc: ObjectIdentifier, _ l: DocGlobalLight) { lock.lock(); defer { lock.unlock() }; lights[doc] = l }
    var copied: String? {
        get { lock.lock(); defer { lock.unlock() }; return clipboard }
        set { lock.lock(); clipboard = newValue; lock.unlock() }
    }
}

extension StubDocumentBackend: DocumentStylesBackend {
    private var styleKey: ObjectIdentifier { ObjectIdentifier(self) }
    private var store: StubStyleStore { .shared }
    private static let emptyStyles = LayerStyleModel().json

    private func styleChange(_ layers: [DocLayerID]) throws -> DocumentChange {
        let i = try info()
        return DocumentChange(layersChanged: layers, created: [], historyHead: i.historyHead,
                              dirtyRect: CanvasRect(x: 0, y: 0, width: Int64(i.width), height: Int64(i.height)),
                              epoch: i.epoch, dirty: i.dirty)
    }

    private func styleable(_ layer: DocLayerID, _ model: LayerStyleModel) throws {
        let n = try self.layer(id: layer)
        if n.locks.all { throw DocumentError.invalid("layer \"\(n.name)\" is locked: unlock it to change its layer style") }
        guard !model.isEmpty else { return }
        if n.kind == .adjustment { throw DocumentError.invalid("adjustment layers cannot have layer styles") }
        if n.kind == .group, n.groupMode == .passThrough {
            throw DocumentError.invalid("pass-through groups cannot have layer styles: set the group's mode to Isolated (Normal) first")
        }
    }

    public func layerStylesJson(layer: DocLayerID) throws -> String {
        _ = try self.layer(id: layer)
        return store.get(styleKey, layer) ?? Self.emptyStyles
    }

    public func setLayerStylesJson(layer: DocLayerID, json: String, interactive: Bool) throws -> DocumentChange {
        guard let model = LayerStyleModel(json: json) else { throw DocumentError.invalid("layer style JSON") }
        try styleable(layer, model)
        store.set(styleKey, layer, model.isEmpty && model.scale == 1 ? nil : model.json)
        return try styleChange([layer])
    }

    public func layerStyleRows() throws -> [LayerStyleRow] {
        let all = store.all(styleKey)
        return try layers().compactMap { n in
            guard let m = LayerStyleModel(json: all[n.id]), !m.isEmpty else { return nil }
            return LayerStyleRow(layer: n.id, model: m)
        }
    }

    public func globalLight() throws -> DocGlobalLight { store.light(styleKey) }

    public func setGlobalLight(_ light: DocGlobalLight, interactive: Bool) throws -> DocumentChange {
        guard light.angle.isFinite, (0...90).contains(light.altitude) else { throw DocumentError.invalid("light altitude outside 0…90") }
        store.setLight(styleKey, light)
        return try styleChange([])
    }

    public func copyLayerStyles(from layer: DocLayerID) throws { store.copied = try layerStylesJson(layer: layer) }
    public func canPasteLayerStyles() -> Bool { store.copied != nil }

    public func pasteLayerStyles(to layers: [DocLayerID]) throws -> DocumentChange {
        guard let json = store.copied, let model = LayerStyleModel(json: json) else {
            throw DocumentError.invalid("no layer style has been copied")
        }
        for l in layers { try styleable(l, model) }
        for l in layers { store.set(styleKey, l, json) }
        return try styleChange(layers)
    }

    public func clearLayerStyles(layer: DocLayerID) throws -> DocumentChange {
        try styleable(layer, LayerStyleModel())
        store.set(styleKey, layer, nil)
        return try styleChange([layer])
    }

    public func styleSchemaJson() -> String { TesseraFFI.styleEffectsSchemaJson() }
}
