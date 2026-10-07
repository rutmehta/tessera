import AppKit
@testable import Tessera

/// A Tab walk judged by identity (B5-49d).
///
/// The in-process keyboard-access pin decides which buttons are Tab stops. It does not decide the
/// stops SwiftUI adds for its own controls (one `KeyViewProxy` view each) or AppKit's other control
/// classes: those follow the machine's real setting. A walk that must reach an identified control
/// therefore may not assume how many presses that takes. This walk presses until the target is the
/// first responder, and stops on its own as soon as it cannot get there.
struct KeyViewWalk: CustomStringConvertible {
    struct Stop {
        /// The first responder after the press; identity is the object itself.
        let object: AnyObject?
        let name: String
        /// False for a stop whose place in the key-view loop the pin does not control.
        let controlled: Bool
    }

    enum Outcome: Equatable {
        /// The target became first responder.
        case reached
        /// The router took the key instead of leaving it to the key-view loop.
        case consumed
        /// A controlled stop came round again: the loop closed without the target.
        case repeated
        /// More controlled stops than the caller allows before the target.
        case overBudget
        /// More uncontrolled stops than any real panel has; a stalled walk ends here.
        case uncontrolledLimit
    }

    /// The start, then one stop per press.
    private(set) var trail: [Stop]
    private(set) var outcome = Outcome.overBudget
    /// Stops after the start that the pin controls, the target included.
    private(set) var controlledStops = 0
    private(set) var uncontrolledStops = 0

    static let uncontrolledLimit = 64

    var reached: Bool { outcome == .reached }
    // Existing destination-only contract, factored for scripted regression coverage.
    func reachesFirst(_ target: AnyObject, before others: [AnyObject]) -> Bool {
        reached && trail.last?.object === target
    }

    @MainActor func staysInSelectedRow(_ row: NSView, outline: NSView) -> Bool {
        trail.allSatisfy { stop in
            stop.object === outline || (stop.object as? NSView)?.isDescendant(of: outline) == true
        } && uncontrolledStops == 0
    }

    var presses: Int { trail.count - 1 }
    func visited(_ object: AnyObject) -> Bool { trail.contains { $0.object === object } }
    var description: String {
        "\(outcome) after \(presses) presses (\(controlledStops) controlled, \(uncontrolledStops) uncontrolled): "
            + trail.map(\.name).joined(separator: " → ")
    }

    /// Presses until `isTarget`. `press` returns nil when the key was consumed.
    ///
    /// `budget` counts only controlled stops after the start, the target included: the stops the
    /// test can predict. Uncontrolled stops are allowed in between, up to `uncontrolledLimit`. They
    /// are not checked for repeats, because one SwiftUI proxy may keep the keyboard for several
    /// presses while focus moves inside SwiftUI. A controlled stop seen twice (the start included)
    /// ends the walk at once. Every press is one or the other, so the walk always terminates within
    /// `budget + uncontrolledLimit + 1` presses.
    static func run(from start: Stop, budget: Int, uncontrolledLimit: Int = KeyViewWalk.uncontrolledLimit,
                    isTarget: (Stop) -> Bool, press: () throws -> Stop?) rethrows -> KeyViewWalk {
        var walk = KeyViewWalk(trail: [start])
        var seen: Set<ObjectIdentifier> = []
        if start.controlled, let object = start.object { seen.insert(ObjectIdentifier(object)) }
        while true {
            guard let stop = try press() else { walk.outcome = .consumed; return walk }
            walk.trail.append(stop)
            if stop.controlled { walk.controlledStops += 1 } else { walk.uncontrolledStops += 1 }
            if isTarget(stop) {
                walk.outcome = walk.controlledStops > budget ? .overBudget : .reached
                return walk
            }
            if stop.controlled {
                if let object = stop.object, !seen.insert(ObjectIdentifier(object)).inserted {
                    walk.outcome = .repeated
                    return walk
                }
                if walk.controlledStops > budget { walk.outcome = .overBudget; return walk }
            } else if walk.uncontrolledStops > uncontrolledLimit {
                walk.outcome = .uncontrolledLimit
                return walk
            }
        }
    }
}

extension KeyViewWalk {
    /// The stop a first responder stands for. `host` gives SwiftUI proxies a frame to be named by.
    @MainActor static func stop(_ responder: NSResponder?, in host: NSView?) -> Stop {
        guard let view = responder as? NSView else {
            return Stop(object: responder, name: responder.map { String(describing: type(of: $0)) } ?? "nil", controlled: true)
        }
        if isSwiftUIProxy(view) { return Stop(object: view, name: proxyName(view, host: host), controlled: false) }
        let identifier = view.accessibilityIdentifier()
        let name = identifier.isEmpty ? String(describing: type(of: view)) : identifier
        return Stop(object: view, name: name, controlled: isControlled(view))
    }

    /// SwiftUI's stand-in view for one focusable SwiftUI control (a private class, matched by name).
    @MainActor static func isSwiftUIProxy(_ view: NSView) -> Bool {
        NSStringFromClass(type(of: view)).hasSuffix("KeyViewProxy")
    }

    /// Buttons follow the pin. Text, lists and the app's own key-owning views are Tab stops in either
    /// mode. Any other AppKit control (slider, pop-up, segmented control, …) follows the real setting.
    @MainActor static func isControlled(_ view: NSView) -> Bool {
        if isSwiftUIProxy(view) { return false }
        guard view is NSControl else { return true }
        return view is NSButton || view is NSTextField || view is NSTableView || view is KeyOwningControl
    }

    /// Names a proxy by the SwiftUI control it stands for: its own accessibility identity, else the
    /// element its hosting view vends at the proxy's centre, else the control's frame in the host.
    /// SwiftUI builds no accessibility tree in a process no assistive client has queried, so in the
    /// background test host the frame is what identifies the control.
    @MainActor static func proxyName(_ view: NSView, host: NSView?) -> String {
        func identity(_ element: (any NSAccessibilityProtocol)?) -> String? {
            guard let element else { return nil }
            if let identifier = element.accessibilityIdentifier(), !identifier.isEmpty { return identifier }
            if let label = element.accessibilityLabel(), !label.isEmpty { return label }
            return nil
        }
        var resolved = identity(view)
        if resolved == nil, let window = view.window, let hosting = view.superview {
            let centre = view.convert(NSPoint(x: view.bounds.midX, y: view.bounds.midY), to: nil)
            if let hit = hosting.accessibilityHitTest(window.convertPoint(toScreen: centre)) as AnyObject?,
               hit !== hosting, hit !== view {
                resolved = identity(hit as? any NSAccessibilityProtocol)
            }
        }
        if let resolved { return "KeyViewProxy(\(resolved))" }
        let frame = view.convert(view.bounds, to: host ?? view.superview)
        return "KeyViewProxy(\(Int(frame.minX)),\(Int(frame.minY)) \(Int(frame.width))×\(Int(frame.height)))"
    }

    /// Presses Tab in `window` through `press` until `isTarget` holds for the first responder.
    @MainActor static func run(in window: NSWindow, budget: Int, isTarget: (NSResponder?) -> Bool,
                               press: () throws -> Bool) rethrows -> KeyViewWalk {
        let host = window.contentView
        return try run(from: stop(window.firstResponder, in: host), budget: budget,
                       isTarget: { _ in isTarget(window.firstResponder) }) {
            try press() ? nil : stop(window.firstResponder, in: host)
        }
    }

    @MainActor static func run(in window: NSWindow, budget: Int, to target: NSResponder,
                               press: () throws -> Bool) rethrows -> KeyViewWalk {
        try run(in: window, budget: budget, isTarget: { $0 === target }, press: press)
    }
}
