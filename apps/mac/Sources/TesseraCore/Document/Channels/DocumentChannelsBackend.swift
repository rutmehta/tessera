import Foundation

// Persistent alpha and spot channels (WP B5-08): a third protocol next to `DocumentBackend` and
// `DocumentToolsBackend`, adopted by `EngineDocumentBackend` over the session's channel calls
// (crates/tessera-ffi/src/document/channels.rs) and by `StubDocumentBackend` with rectangular
// channels. Records carry TesseraCore names distinct from the FFI's: `SavedChannel` ⇄ `ChannelRecord`,
// `SavedChannelKind` ⇄ `DocChannelKind`, `SavedChannelChange` ⇄ `ChannelUpdate`.
//
// The pure models below (panel rows, the Save / Load Selection forms, Quick Mask) hold no UI and are
// unit-tested in DocumentChannelsTests.

public enum SavedChannelKind: String, Sendable, CaseIterable { case alpha, spot }

/// One saved channel (Channels panel row).
public struct SavedChannel: Equatable, Sendable, Identifiable {
    public var id: UInt64
    public var kind: SavedChannelKind
    public var name: String
    /// Spot: ink display colour. Alpha: PSD's default overlay colour (red).
    public var color: ToolColor
    /// Spot: solidity. Alpha: the default overlay opacity (0.5).
    public var opacity: Float
    /// Shown by the preview overlay (session state).
    public var visible: Bool
    public var index: UInt32
    /// Changes when the channel's samples change (thumbnail cache key).
    public var revision: UInt64
    public init(id: UInt64, kind: SavedChannelKind, name: String, color: ToolColor, opacity: Float, visible: Bool,
                index: UInt32, revision: UInt64) {
        self.id = id; self.kind = kind; self.name = name; self.color = color; self.opacity = opacity
        self.visible = visible; self.index = index; self.revision = revision
    }
}

/// A channel edit and the channel it made or changed.
public struct SavedChannelChange: Equatable, Sendable {
    public var channelID: UInt64
    public var change: DocumentChange
    public init(channelID: UInt64, change: DocumentChange) { self.channelID = channelID; self.change = change }
}

/// The session's B5-08 calls, one for one. Every edit is one history node; visibility is not.
public protocol DocumentChannelsBackend: AnyObject, Sendable {
    func documentChannels() throws -> [SavedChannel]
    /// A new alpha channel from the selection (`target` nil, `op` ignored) or the selection combined into
    /// channel `target` by `op`.
    func saveSelectionChannel(name: String, target: UInt64?, op: SelectionCombine) throws -> SavedChannelChange
    func loadSelectionChannel(id: UInt64, op: SelectionCombine, invert: Bool) throws -> DocumentChange
    /// Empty (all masked) or, with `selected`, full (all selected).
    func newAlphaChannel(name: String, selected: Bool) throws -> SavedChannelChange
    func renameDocumentChannel(id: UInt64, name: String) throws -> DocumentChange
    func deleteDocumentChannel(id: UInt64) throws -> DocumentChange
    func duplicateDocumentChannel(id: UInt64) throws -> SavedChannelChange
    /// Spot colour and solidity (preview metadata only; an alpha channel becomes a spot channel).
    func setSpotChannel(id: UInt64, color: ToolColor, solidity: Float) throws -> DocumentChange
    func newSpotChannel(name: String, color: ToolColor, solidity: Float, fromSelection: Bool) throws -> SavedChannelChange
    /// A grey RGBA8 IOSurface id (white = selected / full ink).
    func channelThumbnail(id: UInt64, maxPx: UInt32) throws -> UInt32
    func setChannelVisible(id: UInt64, visible: Bool) throws
}

// MARK: - Panel model

/// How an alpha channel's overlay is drawn (Channel Options; session preference, not saved).
public struct ChannelOverlayStyle: Equatable, Sendable {
    public var color: ToolColor
    public var opacity: Float
    /// Colour marks the selected areas (white) instead of the masked areas (black).
    public var indicatesSelected: Bool
    public init(color: ToolColor = ToolColor(r: 1, g: 0, b: 0), opacity: Float = 0.5, indicatesSelected: Bool = false) {
        self.color = color; self.opacity = opacity; self.indicatesSelected = indicatesSelected
    }
    public static let alphaDefault = ChannelOverlayStyle()
}

/// Which colour components the preview shows (the RGB and Red / Green / Blue eyes).
public struct ComponentVisibility: Equatable, Sendable {
    public var red = true
    public var green = true
    public var blue = true
    public init(red: Bool = true, green: Bool = true, blue: Bool = true) { self.red = red; self.green = green; self.blue = blue }
    public var all: Bool { red && green && blue }
    public var none: Bool { !red && !green && !blue }
    public subscript(_ i: Int) -> Bool {
        get { i == 0 ? red : i == 1 ? green : blue }
        set { if i == 0 { red = newValue } else if i == 1 { green = newValue } else { blue = newValue } }
    }
    /// The composite eye: showing it shows every component, hiding it hides them all.
    public mutating func setComposite(_ on: Bool) { red = on; green = on; blue = on }
}

/// One row of the Channels panel.
public struct ChannelRow: Equatable, Sendable, Identifiable {
    public enum Kind: Equatable, Sendable {
        case composite
        /// 0 red, 1 green, 2 blue.
        case component(Int)
        case alpha
        case spot
    }
    public var id: String
    public var kind: Kind
    public var title: String
    /// Saved channels only.
    public var channelID: UInt64?
    public var visible: Bool
    /// Spot ink or alpha overlay colour.
    public var color: ToolColor?
    public var revision: UInt64
    public var isQuickMask: Bool
    /// Saved channels can be renamed, duplicated, deleted and loaded; the colour rows cannot.
    public var editable: Bool { channelID != nil }
}

public enum ChannelsPanelModel {
    public static let componentTitles = ["Red", "Green", "Blue"]

    /// RGB, Red, Green, Blue, then the saved channels in document order.
    public static func rows(records: [SavedChannel], components: ComponentVisibility, quickMask: UInt64?,
                            styles: [UInt64: ChannelOverlayStyle] = [:]) -> [ChannelRow] {
        var rows = [ChannelRow(id: "rgb", kind: .composite, title: "RGB", channelID: nil, visible: components.all,
                               color: nil, revision: 0, isQuickMask: false)]
        for (i, t) in componentTitles.enumerated() {
            rows.append(ChannelRow(id: "component.\(i)", kind: .component(i), title: t, channelID: nil,
                                   visible: components[i], color: nil, revision: 0, isQuickMask: false))
        }
        for r in records.sorted(by: { $0.index < $1.index }) {
            let color = r.kind == .spot ? r.color : (styles[r.id] ?? .alphaDefault).color
            rows.append(ChannelRow(id: "channel.\(r.id)", kind: r.kind == .spot ? .spot : .alpha, title: r.name,
                                   channelID: r.id, visible: r.visible, color: color, revision: r.revision,
                                   isQuickMask: r.id == quickMask))
        }
        return rows
    }

    /// A name not yet used: "Alpha 1", "Alpha 2", … (or "Spot Color 1", …).
    public static func nextName(_ base: String, existing: [SavedChannel]) -> String {
        let names = Set(existing.map(\.name))
        var n = 1
        while names.contains("\(base) \(n)") { n += 1 }
        return "\(base) \(n)"
    }
}

// MARK: - Save / Load Selection forms

/// Select ▸ Save Selection…: destination channel (nil = a new one), name for a new channel, operation.
public struct SaveSelectionForm: Equatable, Sendable {
    public var destination: UInt64?
    public var name: String
    public var operation: SelectionCombine
    public init(destination: UInt64? = nil, name: String, operation: SelectionCombine = .replace) {
        self.destination = destination; self.name = name; self.operation = operation
    }

    /// A new channel takes only "New Channel"; an existing one Replace / Add / Subtract / Intersect.
    public var operations: [SelectionCombine] { destination == nil ? [.replace] : SelectionCombine.allCases }

    public static func title(_ op: SelectionCombine, newChannel: Bool) -> String {
        switch op {
        case .replace: newChannel ? "New Channel" : "Replace Channel"
        case .add: "Add to Channel"
        case .subtract: "Subtract from Channel"
        case .intersect: "Intersect with Channel"
        }
    }

    /// What to call, or nil when the form is incomplete (a new channel needs a name).
    public var request: (name: String, target: UInt64?, op: SelectionCombine)? {
        let n = name.trimmingCharacters(in: .whitespaces)
        if destination == nil {
            return n.isEmpty ? nil : (n, nil, .replace)
        }
        return (n, destination, operations.contains(operation) ? operation : .replace)
    }
}

/// Select ▸ Load Selection…: source channel, invert, operation (only New Selection without a selection).
public struct LoadSelectionForm: Equatable, Sendable {
    public var channel: UInt64?
    public var invert = false
    public var operation: SelectionCombine = .replace
    public var hasSelection: Bool
    public init(channel: UInt64?, invert: Bool = false, operation: SelectionCombine = .replace, hasSelection: Bool) {
        self.channel = channel; self.invert = invert; self.operation = operation; self.hasSelection = hasSelection
    }

    public var operations: [SelectionCombine] { hasSelection ? SelectionCombine.allCases : [.replace] }

    public var request: (id: UInt64, op: SelectionCombine, invert: Bool)? {
        guard let channel else { return nil }
        return (channel, operations.contains(operation) ? operation : .replace, invert)
    }
}

// MARK: - Quick Mask

/// Quick Mask (Q): entering saves the selection (or, without one, everything) to a temporary alpha
/// channel shown by the overlay; leaving loads that channel as the selection and deletes it.
public enum QuickMask {
    public static let channelName = "Quick Mask"

    /// Returns the temporary channel's id.
    public static func enter(_ b: any DocumentChannelsBackend, hasSelection: Bool) throws -> SavedChannelChange {
        let c = hasSelection ? try b.saveSelectionChannel(name: channelName, target: nil, op: .replace)
            : try b.newAlphaChannel(name: channelName, selected: true)
        try b.setChannelVisible(id: c.channelID, visible: true)
        return c
    }

    /// Makes channel `id` the selection and removes it. A channel deleted meanwhile just ends the mode.
    public static func exit(_ b: any DocumentChannelsBackend, channel id: UInt64) throws -> DocumentChange? {
        guard try b.documentChannels().contains(where: { $0.id == id }) else { return nil }
        _ = try b.loadSelectionChannel(id: id, op: .replace, invert: false)
        return try b.deleteDocumentChannel(id: id)
    }
}
