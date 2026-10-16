import TesseraFFI

// Integration prerequisite: A must generate bindings for the frozen Smart Preview
// API. Do not hand-edit TesseraFFI.swift or replace these calls with successful stubs.
extension SmartPreviewAPI {
    public static func live(engine: Engine) -> SmartPreviewAPI {
        // OF-2: the offline calls go through `OfflineEngineMethods`; nil until OF-1's bindings
        // land and the wiring extension below is uncommented. Until then they report
        // `offlineUnavailable` and the builder stays idle.
        let offline = engine as? any OfflineEngineMethods
        func offlineEngine() throws -> any OfflineEngineMethods {
            guard let offline else { throw SmartPreviewUIError.unavailable(offlineUnavailable) }
            return offline
        }
        return .init(info: { id in
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
        }, coverage: { folder in
            let engine = try offlineEngine()
            return try await EngineCall.detached(priority: .utility) { try engine.offlineCoverageRecord(folder: folder) }
        }, candidates: { folder, limit in
            let engine = try offlineEngine()
            return try await EngineCall.detached(priority: .utility) { try engine.offlineCandidateIDs(folder: folder, limit: limit) }
        }, ensure: { id in
            let engine = try offlineEngine()
            // One build at a time, at background priority, never on the main actor (SPEC OF-2).
            return try await EngineCall.detached(priority: .background) { try engine.ensureSmartPreviewSnapshot(imageID: id) }
        }, pendingSync: {
            let engine = try offlineEngine()
            return try await EngineCall.detached(priority: .utility) { try engine.pendingSyncSmartPreviewIDs() }
        }, storage: {
            let engine = try offlineEngine()
            return try await EngineCall.detached(priority: .utility) { try engine.smartPreviewStorageBytes() }
        })
    }
}

// OF-1 wiring: uncomment once `TesseraFFI.swift` carries `offlineCoverage`, `offlineCandidates`,
// `ensureSmartPreview`, `pendingSyncSmartPreviews` and `smartPreviewStorage` (SPEC "OF-1: engine
// contract"). Nothing else changes: `live(engine:)` finds the conformance at runtime.
//
// extension Engine: OfflineEngineMethods {
//     public func offlineCoverageRecord(folder: String) throws -> OfflineCoverage { OfflineCoverage(try offlineCoverage(folder: folder)) }
//     public func offlineCandidateIDs(folder: String, limit: UInt32) throws -> [String] { try offlineCandidates(folder: folder, limit: limit) }
//     public func ensureSmartPreviewSnapshot(imageID: String) throws -> SmartPreviewSnapshot { SmartPreviewSnapshot(try ensureSmartPreview(imageId: imageID)) }
//     public func pendingSyncSmartPreviewIDs() throws -> [String] { try pendingSyncSmartPreviews() }
//     public func smartPreviewStorageBytes() throws -> UInt64 { try smartPreviewStorage() }
// }
//
// extension OfflineCoverage {
//     init(_ record: TesseraFFI.OfflineCoverage) {
//         self.init(folder: record.folder, raws: record.raws, ready: record.ready,
//                   pendingSync: record.pendingSync, needsAttention: record.needsAttention, bytes: record.bytes)
//     }
// }

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
