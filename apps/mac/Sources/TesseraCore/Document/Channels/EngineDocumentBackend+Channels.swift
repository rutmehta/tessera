import Foundation
import TesseraFFI

// `EngineDocumentBackend` as a `DocumentChannelsBackend` (WP B5-08): each call is the session call of
// the same name (crates/tessera-ffi/src/document/channels.rs), records converted field by field.

extension SavedChannel {
    init(_ r: ChannelRecord) {
        self.init(id: r.id, kind: r.kind == .spot ? .spot : .alpha, name: r.name, color: ToolColor(r.color),
                  opacity: r.opacity, selectedAreas: r.selectedAreas, visible: r.visible, index: r.index, revision: r.revision)
    }
}

extension EngineDocumentBackend: DocumentChannelsBackend {
    private func channelChange(_ body: () throws -> ChannelUpdate) throws -> SavedChannelChange {
        var id: UInt64 = 0
        let c = try change {
            let u = try body()
            id = u.channelId
            return u.update
        }
        return SavedChannelChange(channelID: id, change: c)
    }

    public func documentChannels() throws -> [SavedChannel] {
        try bridged { try session.documentChannels() }.map(SavedChannel.init)
    }

    public func saveSelectionChannel(name: String, target: UInt64?, op: SelectionCombine) throws -> SavedChannelChange {
        try channelChange { try session.saveSelectionChannel(name: name, target: target, op: op.ffi) }
    }

    public func loadSelectionChannel(id: UInt64, op: SelectionCombine, invert: Bool) throws -> DocumentChange {
        try change { try session.loadSelectionChannel(id: id, op: op.ffi, invert: invert) }
    }

    public func newAlphaChannel(name: String, selected: Bool) throws -> SavedChannelChange {
        try channelChange { try session.newAlphaChannel(name: name, selected: selected) }
    }

    public func renameDocumentChannel(id: UInt64, name: String) throws -> DocumentChange {
        try change { try session.renameDocumentChannel(id: id, name: name) }
    }

    public func deleteDocumentChannel(id: UInt64) throws -> DocumentChange {
        try change { try session.deleteDocumentChannel(id: id) }
    }

    public func duplicateDocumentChannel(id: UInt64) throws -> SavedChannelChange {
        try channelChange { try session.duplicateDocumentChannel(id: id) }
    }

    public func setSpotChannel(id: UInt64, color: ToolColor, solidity: Float) throws -> DocumentChange {
        try change { try session.setSpotChannel(id: id, color: color.ffi, solidity: solidity) }
    }

    public func setAlphaChannelDisplay(id: UInt64, color: ToolColor, opacity: Float, selectedAreas: Bool) throws
        -> DocumentChange {
        try change {
            try session.setAlphaChannelDisplay(id: id, color: color.ffi, opacity: opacity, selectedAreas: selectedAreas)
        }
    }

    public func newSpotChannel(name: String, color: ToolColor, solidity: Float, fromSelection: Bool) throws
        -> SavedChannelChange {
        try channelChange {
            try session.newSpotChannel(name: name, color: color.ffi, solidity: solidity, fromSelection: fromSelection)
        }
    }

    public func channelThumbnail(id: UInt64, maxPx: UInt32) throws -> UInt32 {
        try bridged { try session.channelThumbnail(id: id, maxPx: maxPx) }
    }

    public func setChannelVisible(id: UInt64, visible: Bool) throws {
        try bridged { try session.setChannelVisible(id: id, visible: visible) }
    }
}
