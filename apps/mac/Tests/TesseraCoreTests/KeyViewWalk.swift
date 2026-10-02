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
    var presses: Int { trail.count - 1 }
    func visited(_ object: AnyObject) -> Bool { trail.contains { $0.object === object } }
    var description: String {
        "\(outcome) after \(presses) presses (\(controlledStops) controlled, \(uncontrolledStops) uncontrolled): "
            + trail.map(\.name).joined(separator: " → ")
    }

    /// Presses until `isTarget`. `budget` is the number of stops allowed before giving up.
    /// `press` returns nil when the key was consumed.
    static func run(from start: Stop, budget: Int, uncontrolledLimit: Int = KeyViewWalk.uncontrolledLimit,
                    isTarget: (Stop) -> Bool, press: () throws -> Stop?) rethrows -> KeyViewWalk {
        var walk = KeyViewWalk(trail: [start])
        // RED (B5-49d): every stop counts against the budget, as in the test that failed on Machine A.
        while walk.presses < budget {
            guard let stop = try press() else { walk.outcome = .consumed; return walk }
            walk.trail.append(stop)
            if stop.controlled { walk.controlledStops += 1 } else { walk.uncontrolledStops += 1 }
            if isTarget(stop) { walk.outcome = .reached; return walk }
        }
        _ = uncontrolledLimit
        return walk
    }
}
