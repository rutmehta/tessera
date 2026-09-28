import AppKit
import Darwin

/// Value-only, opt-in diagnostics. No trace decision participates in shortcut ownership.
protocol InspectorFocusTraceSink: AnyObject {
    func append(_ data: Data) throws
    func close()
}

@MainActor
protocol InspectorFocusTraceNode: AnyObject {
    var identity: ObjectIdentifier { get }
    var role: String? { get }
    var identifier: String? { get }
    var focused: Bool? { get }
    var windowIdentity: ObjectIdentifier? { get }
    var parent: (any InspectorFocusTraceNode)? { get }
    func children(limit: Int) -> InspectorFocusTraceChildren
}

struct InspectorFocusTraceChildren {
    let nodes: [any InspectorFocusTraceNode]
    let truncated: Bool
}

/// All operational entry points are MainActor; nonisolated deinit only closes the owned sink.
/// Neither the sink nor any serialized record retains a responder, window or AX element.
final class InspectorFocusTrace {
    struct Input: Codable, Sendable {
        var keyDown: Bool
        var keyCode: UInt16
        var modifiers: UInt
        var timestamp: Double
        var document: Bool
        var ownedKeyWindow: Bool
        var blockedWindow: Bool
        var fullKeyboardAccess: Bool
        var windowIdentity: String = ""

        var eligible: Bool {
            keyDown && [48, 49, 36, 76].contains(keyCode) && document && ownedKeyWindow && !blockedWindow
                && NSEvent.ModifierFlags(rawValue: modifiers).intersection([.command, .control, .option]).isEmpty
        }
    }

    struct Semantic: Codable, Sendable {
        let identity: String
        let role: String?
        let identifier: String?
    }

    struct Snapshot: Codable, Sendable {
        let status: String
        let nativeType: String
        let nativeIdentity: String
        let semantics: [Semantic]
        let incomplete: Bool
        var primary: Semantic? = nil
        var primaryFocused: Bool? = nil
        var primaryMembership: String = "unavailable"
        static let unknown = Snapshot(status: "unknown", nativeType: "", nativeIdentity: "",
                                      semantics: [], incomplete: true)
    }

    private struct Record: Encodable {
        let version = 1
        let sequence: Int
        let input: Input
        let before: Snapshot
        let handled: Bool
        let reentrantEventsSkipped: Int
    }

    private let sink: any InspectorFocusTraceSink
    private let byteLimit: Int
    private var bytes = 0
    private var events = 0
    private var closed = false
    private var inFlight = false
    private var reentrantEventsSkipped = 0

    @MainActor
    init(sink: any InspectorFocusTraceSink, byteLimit: Int = 64 * 1024) {
        self.sink = sink
        self.byteLimit = max(0, min(byteLimit, 64 * 1024))
    }

    deinit { if !closed { sink.close() } }

    @MainActor
    static func configured(arguments: [String],
                           open: (String) -> (any InspectorFocusTraceSink)? = { ExclusiveFocusTraceFile(path: $0) })
        -> InspectorFocusTrace? {
        let positions = arguments.indices.filter { arguments[$0] == "--inspector-focus-trace" }
        guard positions.count == 1, let index = positions.first, index + 1 < arguments.count,
              !arguments[index + 1].isEmpty, !arguments[index + 1].hasPrefix("--"),
              let sink = open(arguments[index + 1]) else { return nil }
        return InspectorFocusTrace(sink: sink)
    }

    /// The handler is invoked exactly once, even if capture/output is disabled or exhausted.
    @MainActor
    static func route(_ trace: InspectorFocusTrace?, input: Input, capture: () -> Snapshot,
                      handler: () -> Bool) -> Bool {
        guard let trace, !trace.closed, input.eligible else { return handler() }
        guard !trace.inFlight else {
            trace.reentrantEventsSkipped += 1
            return handler()
        }
        trace.inFlight = true
        defer { trace.inFlight = false }
        let before = capture()
        let handled = handler()
        trace.record(input: input, before: before, handled: handled)
        return handled
    }

    @MainActor
    private func record(input: Input, before: Snapshot, handled: Bool) {
        guard !closed else { return }
        events += 1
        do {
            let encoder = JSONEncoder()
            encoder.outputFormatting = [.sortedKeys]
            var data = try encoder.encode(Record(sequence: events, input: input, before: before, handled: handled,
                                                reentrantEventsSkipped: reentrantEventsSkipped))
            data.append(0x0a)
            // Reserve space for a final bounded marker. No oversized record is partly emitted.
            let marker = Data("{\"traceEnd\":\"limit\"}\n".utf8)
            guard bytes + data.count + marker.count <= byteLimit else {
                if bytes + marker.count <= byteLimit { try sink.append(marker); bytes += marker.count }
                stop()
                return
            }
            try sink.append(data)
            bytes += data.count
            if events >= 32 {
                try sink.append(marker)
                bytes += marker.count
                stop()
            }
        } catch {
            // Output errors only disable diagnostics; never change the event result.
            stop()
        }
    }

    @MainActor
    private func stop() {
        guard !closed else { return }
        closed = true
        sink.close()
    }

    /// Called at the existing local monitor boundary. No native/AX capture on the nil path.
    @MainActor
    static func routeEvent(_ trace: InspectorFocusTrace?, event: NSEvent, document: Bool, ownedWindow: NSWindow?,
                           handler: () -> Bool) -> Bool {
        guard let trace, !trace.closed else { return handler() }
        let window = event.window
        let input = Input(keyDown: event.type == .keyDown, keyCode: event.keyCode,
            modifiers: event.modifierFlags.intersection(.deviceIndependentFlagsMask).rawValue,
            // NSEvent.eventNumber is mouse-only. Sequence + timestamp identify keyboard records.
            timestamp: event.timestamp, document: document,
            ownedKeyWindow: window != nil && window === NSApp.keyWindow && window === ownedWindow,
            blockedWindow: window == nil || window is NSPanel || window?.attachedSheet != nil
                || window?.sheetParent != nil || NSApp.modalWindow != nil,
            fullKeyboardAccess: NSApp.isFullKeyboardAccessEnabled,
            windowIdentity: window.map { String(describing: ObjectIdentifier($0)) } ?? "")
        return route(trace, input: input, capture: {
            guard let window else { return .unknown }
            let responder = window.firstResponder
            let focus = AppKitFocusNode.wrap(NSApp.accessibilityApplicationFocusedUIElement())
            return snapshot(focused: focus, root: AppKitFocusNode(window), window: ObjectIdentifier(window),
                nativeType: responder.map { String(reflecting: type(of: $0)) } ?? "nil",
                nativeIdentity: responder.map { String(describing: ObjectIdentifier($0)) } ?? "")
        }, handler: handler)
    }

    /// The budget is shared by primary membership and fallback traversal. Missing evidence
    /// is unknown, not proof that viewport shortcuts own the event.
    @MainActor
    static func snapshot(focused: (any InspectorFocusTraceNode)?, root: (any InspectorFocusTraceNode)?,
                         window: ObjectIdentifier, nativeType: String, nativeIdentity: String) -> Snapshot {
        var inspected = Set<ObjectIdentifier>()
        var incomplete = false
        func admit(_ node: any InspectorFocusTraceNode) -> Bool {
            if inspected.contains(node.identity) { return true }
            guard inspected.count < 64 else { incomplete = true; return false }
            inspected.insert(node.identity)
            return true
        }
        func belongs(_ node: any InspectorFocusTraceNode) -> Bool {
            var current: (any InspectorFocusTraceNode)? = node
            var path = Set<ObjectIdentifier>()
            for _ in 0..<8 {
                guard let value = current else { return false }
                guard admit(value), path.insert(value.identity).inserted else { incomplete = true; return false }
                if value.identity == window { return true }
                if let owner = value.windowIdentity { return owner == window }
                current = value.parent
            }
            incomplete = true
            return false
        }
        var candidates: [any InspectorFocusTraceNode] = []
        let primaryFocused = focused?.focused
        let primaryMember = focused.map { belongs($0) }
        if let focused, primaryFocused == true, primaryMember == true {
            candidates = [focused]
        } else {
            // A conflicting/incomplete application focus must not be upgraded by
            // finding a convenient focused descendant elsewhere in the tree.
            if focused != nil { incomplete = true }
        }
        if candidates.isEmpty, let root {
            var queue: [(any InspectorFocusTraceNode, Int)] = [(root, 0)]
            var visited = Set<ObjectIdentifier>()
            var index = 0
            while index < queue.count {
                let (node, depth) = queue[index]
                index += 1
                guard depth < 8, admit(node), visited.insert(node.identity).inserted else {
                    incomplete = true
                    continue
                }
                if node.focused == true, belongs(node) { candidates.append(node) }
                let capacity = max(0, 64 - queue.count)
                let children = node.children(limit: capacity)
                if children.truncated || children.nodes.count > capacity { incomplete = true }
                for child in children.nodes.prefix(capacity) { queue.append((child, depth + 1)) }
            }
        }
        let status = incomplete ? "unknown" : candidates.count > 1 ? "ambiguous"
            : candidates.count == 1 ? "focused" : "unknown"
        let semantic = candidates.prefix(64).map {
            Semantic(identity: String(describing: $0.identity), role: safeRole($0.role),
                     identifier: safeIdentifier($0.identifier))
        }
        let primary = focused.map {
            Semantic(identity: String(describing: $0.identity), role: safeRole($0.role),
                     identifier: safeIdentifier($0.identifier))
        }
        return Snapshot(status: status, nativeType: String(nativeType.prefix(160)),
                        nativeIdentity: String(nativeIdentity.prefix(80)), semantics: semantic, incomplete: incomplete,
                        primary: primary, primaryFocused: primaryFocused,
                        primaryMembership: primaryMember == true ? "matched" : focused == nil ? "unavailable" : "unproven")
    }

    private static func safeRole(_ value: String?) -> String? {
        let roles: Set<String> = ["AXButton", "AXCheckBox", "AXRadioButton", "AXTextField", "AXTextArea",
            "AXSlider", "AXPopUpButton", "AXMenuButton", "AXComboBox", "AXGroup", "AXScrollArea",
            "AXOutline", "AXRow", "AXWindow", "AXSplitGroup", "AXTabGroup", "AXStaticText", "AXUnknown"]
        return value.flatMap { roles.contains($0) ? $0 : nil }
    }

    private static func safeIdentifier(_ value: String?) -> String? {
        let identifiers: Set<String> = ["document.properties.name", "document.properties.colorLookup.load",
            "document.properties.colorLookup.reset", "document.properties.colorLookup.dither",
            "document.history.height.decrease", "document.history.height.increase",
            "document.history.height.reset", "document.history.height.value"]
        return value.flatMap { identifiers.contains($0) ? $0 : nil }
    }
}

/// Public semantic objects may be virtual SwiftUI accessibility elements, not NSViews.
@MainActor
private final class AppKitFocusNode: InspectorFocusTraceNode {
    private let object: AnyObject
    init(_ object: AnyObject) { self.object = object }
    static func wrap(_ value: Any?) -> AppKitFocusNode? { value.map { AppKitFocusNode($0 as AnyObject) } }
    private var ax: (any NSAccessibilityProtocol)? { object as? any NSAccessibilityProtocol }
    var identity: ObjectIdentifier { ObjectIdentifier(object) }
    var role: String? { ax?.accessibilityRole()?.rawValue }
    var identifier: String? { ax?.accessibilityIdentifier() }
    var focused: Bool? { ax?.isAccessibilityFocused() }
    var windowIdentity: ObjectIdentifier? {
        ax?.accessibilityWindow().map { ObjectIdentifier($0 as AnyObject) }
    }
    var parent: (any InspectorFocusTraceNode)? { Self.wrap(ax?.accessibilityParent()) }
    func children(limit: Int) -> InspectorFocusTraceChildren {
        guard let ax else { return .init(nodes: [], truncated: true) }
        // Getter materialization cost is framework-controlled; iteration and retention are bounded.
        if let ordered = ax.accessibilityChildrenInNavigationOrder() {
            return .init(nodes: ordered.prefix(limit).map { AppKitFocusNode($0 as AnyObject) },
                         truncated: ordered.count > limit)
        }
        let raw = ax.accessibilityChildren() ?? []
        return .init(nodes: raw.prefix(limit).map { AppKitFocusNode($0 as AnyObject) },
                     truncated: raw.count > limit)
    }
}

private final class ExclusiveFocusTraceFile: InspectorFocusTraceSink {
    private var file: FileHandle?
    init?(path: String) {
        let descriptor = Darwin.open(path, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC | O_NOFOLLOW, S_IRUSR | S_IWUSR)
        guard descriptor >= 0 else { return nil }
        file = FileHandle(fileDescriptor: descriptor, closeOnDealloc: true)
    }
    func append(_ data: Data) throws {
        guard let file else { throw CocoaError(.fileWriteUnknown) }
        try file.write(contentsOf: data)
    }
    func close() {
        try? file?.close()
        file = nil
    }
    deinit { close() }
}
