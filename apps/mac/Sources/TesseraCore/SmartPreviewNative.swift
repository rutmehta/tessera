import TesseraFFI

// Integration prerequisite: A must generate bindings for the frozen Smart Preview
// API. Do not hand-edit TesseraFFI.swift or replace these calls with successful stubs.
extension SmartPreviewAPI {
    public static func live(engine: Engine) -> SmartPreviewAPI {
        .init(info: { id in
            try await Task.detached(priority: .utility) {
                SmartPreviewSnapshot(try engine.smartPreviewInfo(imageId: id))
            }.value
        }, build: { id in
            try await Task.detached(priority: .utility) {
                SmartPreviewSnapshot(try engine.buildSmartPreview(imageId: id))
            }.value
        }, discard: { id in
            try await Task.detached(priority: .utility) {
                try engine.discardSmartPreview(imageId: id)
            }.value
        }, synchronize: { id in
            try await Task.detached(priority: .utility) {
                SmartPreviewSnapshot(try engine.synchronizeSmartPreview(imageId: id))
            }.value
        })
    }
}

extension SmartPreviewSnapshot {
    init(_ info: TesseraFFI.SmartPreviewInfo) {
        let state: State = switch info.state {
        case .missing: .missing
        case .ready: .ready
        case .originalOffline: .originalOffline
        case .dirty: .dirty
        case .stale: .stale
        case .failed: .failed
        case .conflict: .conflict
        }
        self.init(imageID: info.imageId, state: state, originalAvailable: info.originalAvailable,
                  dirty: info.dirty, width: info.width, height: info.height, message: info.message)
    }
}
