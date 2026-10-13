import Foundation

/// Click-to-heal (UX-13): one entry of the recipe's `locals.retouch` list per click. The develop
/// pipeline's registered retouch renderer (`brush::render_retouch`) draws additive explicit-source
/// heal and clone spots with zero operation feather, so each click is a round brush spot healed
/// from a source beside it. Points are mask-space fractions; the radius is a fraction of the width.
public enum RetouchSpots {
    public struct Spot: Equatable, Sendable {
        public let id: UInt32
        public let center: (x: Double, y: Double)
        public let radius: Double
        public let enabled: Bool

        public static func == (a: Spot, b: Spot) -> Bool {
            a.id == b.id && a.center == b.center && a.radius == b.radius && a.enabled == b.enabled
        }
    }

    /// Default spot radius as a fraction of the image width (the demo's 26 pt spot on a 1000 pt photo).
    public static let defaultRadius = 0.014

    /// Where the heal samples from: beside the spot, inside the frame, as a normalised `[dx, dy]`.
    public static func sourceOffset(center: (x: Double, y: Double), radius: Double) -> (dx: Double, dy: Double) {
        let gap = radius * 2.6
        if center.x + gap + radius <= 1 { return (gap, 0) }
        if center.x - gap - radius >= 0 { return (-gap, 0) }
        return (0, center.y + gap + radius <= 1 ? gap : -gap)
    }

    /// The heal operation (recipe JSON object) for a spot at `center`.
    public static func healOperation(id: UInt32, center: (x: Double, y: Double), radius: Double = defaultRadius) -> [String: Any] {
        let source = sourceOffset(center: center, radius: radius)
        return [
            "id": id,
            "kind": ["kind": "heal", "source_offset": [source.dx, source.dy]],
            "target": ["kind": "area", "components": [[
                "kind": "brush",
                "strokes": [["points": [[center.x, center.y, 1.0]], "radius": radius, "feather": 50.0, "flow": 100.0, "erase": false]],
            ]]],
            "opacity": 100.0,
            "feather": 0.0,
            "enabled": true,
        ]
    }

    /// The retouch list of `settings` (`locals.retouch`), as stored.
    public static func operations(in settings: [String: Any]) -> [[String: Any]] {
        ((settings["locals"] as? [String: Any])?["retouch"] as? [[String: Any]]) ?? []
    }

    /// The heal and clone spots of `settings`, for drawing (one per brush point).
    public static func spots(in settings: [String: Any]) -> [Spot] {
        var out: [Spot] = []
        for op in operations(in: settings) {
            guard let id = (op["id"] as? NSNumber)?.uint32Value,
                  let target = op["target"] as? [String: Any], target["kind"] as? String == "area",
                  let components = target["components"] as? [[String: Any]] else { continue }
            let enabled = (op["enabled"] as? NSNumber)?.boolValue ?? true
            for c in components where c["kind"] as? String == "brush" {
                for s in (c["strokes"] as? [[String: Any]]) ?? [] {
                    let radius = (s["radius"] as? NSNumber)?.doubleValue ?? 0.02
                    for p in (s["points"] as? [[NSNumber]]) ?? [] where p.count >= 2 {
                        out.append(Spot(id: id, center: (p[0].doubleValue, p[1].doubleValue), radius: radius, enabled: enabled))
                    }
                }
            }
        }
        return out
    }

    /// The first id no operation of `settings` uses.
    public static func nextID(in settings: [String: Any]) -> UInt32 {
        (operations(in: settings).compactMap { ($0["id"] as? NSNumber)?.uint32Value }.max()).map { $0 + 1 } ?? 0
    }

    /// The engine patch that appends `operation` (arrays replace wholesale in a merge patch).
    public static func appending(_ operation: [String: Any], to settings: [String: Any]) -> [String: Any] {
        ["locals": ["retouch": operations(in: settings) + [operation]]]
    }
}
