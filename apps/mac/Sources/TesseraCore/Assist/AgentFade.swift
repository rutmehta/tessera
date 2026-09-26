import Foundation

/// The "Agent base edit" amount slider (docs/10 §2 "fine-tune surface"): previews an amount of
/// one history group as a JSON merge patch against the live settings.
///
/// The engine (`DevelopSession.historyGroups`) supplies the settings with the group at 0 % and at
/// 100 %, everything else (later manual edits, step toggles, other groups) as it is. An amount `a`
/// is `without + a · (with − without)` on numbers (rounded when both are integers); any other
/// differing value (a mode, a curve, a flag) takes `with` from 50 %. These are the semantics of
/// `DevelopSession.commitGroupAmount`, which records the final amount on mouse-up, so the drag
/// preview and the recorded step agree.
public enum AgentFade {
    /// The settings value at `amount` between `without` (0) and `with` (1).
    public static func blend(without: Any?, with: Any?, amount: Double) -> Any? {
        let a = min(max(amount, 0), 1)
        switch (without, with) {
        case let (x as NSNumber, y as NSNumber) where !isBool(x) && !isBool(y):
            if a == 0 { return x }
            if a == 1 { return y }
            let value = x.doubleValue + a * (y.doubleValue - x.doubleValue)
            return isFloat(x) || isFloat(y) ? NSNumber(value: value) : NSNumber(value: Int64(value.rounded()))
        case let (x as [String: Any], y as [String: Any]):
            var out: [String: Any] = [:]
            for key in Set(x.keys).union(y.keys) {
                switch (x[key], y[key]) {
                case let (xv?, yv?): out[key] = blend(without: xv, with: yv, amount: a)
                case let (xv?, nil): if a < 0.5 { out[key] = xv }
                case let (nil, yv?): if a >= 0.5 { out[key] = yv }
                case (nil, nil): break
                }
            }
            return out
        default:
            if equal(without, with) { return without }
            return a >= 0.5 ? with : without
        }
    }

    /// RFC 7386 merge patch turning `current` into `target` (nested objects merge, other values
    /// replace, removed members are `NSNull`). Empty when they are equal.
    public static func mergePatch(from current: [String: Any], to target: [String: Any]) -> [String: Any] {
        var patch: [String: Any] = [:]
        for (key, value) in target {
            let old = current[key]
            if let v = value as? [String: Any], let o = old as? [String: Any] {
                let nested = mergePatch(from: o, to: v)
                if !nested.isEmpty { patch[key] = nested }
            } else if !equal(old, value) {
                patch[key] = value
            }
        }
        for key in current.keys where target[key] == nil { patch[key] = NSNull() }
        return patch
    }

    /// The patch that shows `amount` of a group, from the engine's JSON for the group off / on.
    /// Nil when the JSON is not an object.
    public static func patch(current: [String: Any], withoutJSON: String, withJSON: String,
                             amount: Double) -> [String: Any]? {
        guard let without = object(withoutJSON), let with = object(withJSON),
              let target = blend(without: without, with: with, amount: amount) as? [String: Any] else { return nil }
        return mergePatch(from: current, to: target)
    }

    /// "Agent base edit 60 %" style readout.
    public static func percent(_ amount: Double) -> String { "\(Int((min(max(amount, 0), 1) * 100).rounded())) %" }

    static func object(_ json: String) -> [String: Any]? {
        (try? JSONSerialization.jsonObject(with: Data(json.utf8))) as? [String: Any]
    }

    private static func isBool(_ n: NSNumber) -> Bool { CFGetTypeID(n) == CFBooleanGetTypeID() }
    private static func isFloat(_ n: NSNumber) -> Bool { CFNumberIsFloatType(n) }

    static func equal(_ a: Any?, _ b: Any?) -> Bool {
        switch (a, b) {
        case (nil, nil): return true
        case let (x as NSObject, y as NSObject): return x.isEqual(y)
        default: return false
        }
    }
}
