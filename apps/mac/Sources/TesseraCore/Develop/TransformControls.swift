import Foundation

// MARK: - Upright (M2-48)

/// The engine's `UprightMode` (`geometry.upright.mode`), in Lightroom's button order.
public enum UprightMode: String, CaseIterable, Sendable, Identifiable {
    case off, auto, guided, level, vertical, full
    public var id: String { rawValue }
    public var title: String { rawValue.capitalized }
    /// SF Symbol for the compact inspector bar (the name is the help tag and accessibility label).
    public var symbol: String {
        switch self {
        case .off: "circle.slash"
        case .auto: "wand.and.stars"
        case .guided: "pencil.and.ruler"
        case .level: "level"
        case .vertical: "arrow.up.and.down"
        case .full: "perspective"
        }
    }
    public var help: String {
        switch self {
        case .off: "Off: no automatic perspective correction"
        case .auto: "Auto: balanced level, aspect and perspective"
        case .guided: "Guided: draw two to four guides along lines that should be vertical or horizontal"
        case .level: "Level: straighten to the horizon only"
        case .vertical: "Vertical: level and fix converging verticals"
        case .full: "Full: level, vertical and horizontal perspective"
        }
    }
}

/// One Guided Upright guide in sensor-normalised coordinates (`0…1`, origin top-left), the
/// engine's `GuideLine`.
public struct UprightGuide: Equatable, Sendable {
    public var start: (x: Double, y: Double)
    public var end: (x: Double, y: Double)

    public init(start: (x: Double, y: Double), end: (x: Double, y: Double)) {
        self.start = (Self.unit(start.x), Self.unit(start.y))
        self.end = (Self.unit(end.x), Self.unit(end.y))
    }

    public static func == (a: UprightGuide, b: UprightGuide) -> Bool {
        a.start == b.start && a.end == b.end
    }

    /// Sensor-normalised length (guides shorter than `UprightGuides.minimumLength` are dropped).
    public var length: Double { hypot(end.x - start.x, end.y - start.y) }

    public var json: [String: Any] { ["start": [start.x, start.y], "end": [end.x, end.y]] }

    public init?(json: Any?) {
        guard let o = json as? [String: Any], let s = o["start"] as? [NSNumber], let e = o["end"] as? [NSNumber],
              s.count == 2, e.count == 2 else { return nil }
        self.init(start: (s[0].doubleValue, s[1].doubleValue), end: (e[0].doubleValue, e[1].doubleValue))
    }

    static func unit(_ v: Double) -> Double { v.isFinite ? min(max(v, 0), 1) : 0 }
}

/// The Guided Upright editor model: up to four guides, endpoint drags, removal. The engine accepts
/// the Guided mode only with two to four guides, so `patch` writes Guided then and Off (no guides)
/// otherwise; a recipe can never hold a Guided mode the renderer rejects.
public struct UprightGuides: Equatable, Sendable {
    public static let maximum = 4
    public static let minimum = 2
    /// Shorter guides (sensor-normalised) are treated as clicks and dropped.
    public static let minimumLength = 0.01

    public private(set) var guides: [UprightGuide]
    public var selected: Int?

    public init(_ guides: [UprightGuide] = []) {
        self.guides = Array(guides.filter { $0.length >= Self.minimumLength }.prefix(Self.maximum))
    }

    /// Guides stored in a settings document (any mode).
    public init(settings: [String: Any]) {
        let raw = DevelopController.value(in: settings, at: UprightControls.guidesPath) as? [Any] ?? []
        self.init(raw.compactMap(UprightGuide.init(json:)))
    }

    public var isFull: Bool { guides.count >= Self.maximum }
    /// Enough guides for the engine's Guided mode.
    public var isComplete: Bool { guides.count >= Self.minimum }

    /// Adds a guide; returns its index, or nil when four exist or it is too short.
    @discardableResult
    public mutating func add(_ g: UprightGuide) -> Int? {
        guard !isFull, g.length >= Self.minimumLength else { return nil }
        guides.append(g)
        selected = guides.count - 1
        return selected
    }

    /// Moves one end (`end == false`: the start) of guide `index`, clamped to the image.
    public mutating func move(_ index: Int, end: Bool, to p: (x: Double, y: Double)) {
        guard guides.indices.contains(index) else { return }
        let g = guides[index]
        guides[index] = end ? UprightGuide(start: g.start, end: p) : UprightGuide(start: p, end: g.end)
        selected = index
    }

    public mutating func remove(_ index: Int) {
        guard guides.indices.contains(index) else { return }
        guides.remove(at: index)
        selected = nil
    }

    public mutating func removeAll() {
        guides.removeAll()
        selected = nil
    }

    /// The recipe patch for the current guides (Guided with 2–4, else Off without guides).
    public var patch: [String: Any] {
        isComplete
            ? UprightControls.patch(mode: .guided, guides: guides)
            : UprightControls.patch(mode: .off)
    }

    public var historyLabel: String {
        isComplete ? "Upright: Guided (\(guides.count) guides)" : "Upright: Off"
    }
}

public enum UprightControls {
    public static let modePath = ["geometry", "upright", "mode"]
    public static let guidesPath = ["geometry", "upright", "guides"]
    public static let constrainCropPath = ["geometry", "constrain_crop"]

    /// Upright mode (non-Guided modes clear the guides: the engine rejects guides outside Guided).
    public static func patch(mode: UprightMode, guides: [UprightGuide] = []) -> [String: Any] {
        let kept = mode == .guided ? guides : []
        return ["geometry": ["upright": ["mode": mode.rawValue, "guides": kept.map(\.json), "homography": NSNull(), "homography_mode": NSNull()]]]
    }

    public static func mode(in settings: [String: Any]) -> UprightMode {
        (DevelopController.value(in: settings, at: modePath) as? String).flatMap(UprightMode.init(rawValue:)) ?? .off
    }

    /// The Upright group's reset (Off, no guides).
    public static var resetPatch: [String: Any] { patch(mode: .off) }

    public static func constrainCropPatch(_ on: Bool) -> [String: Any] { DevelopController.patch(constrainCropPath, on) }

    public static func historyLabel(_ mode: UprightMode) -> String { "Upright: \(mode.title)" }
}

// MARK: - Manual transform

/// The Transform panel's manual sliders (`geometry.transform`, ranges from `crs:Perspective*`).
public enum TransformControls {
    public static let vertical = DevelopControl("Vertical", ["geometry", "transform", "vertical"], -100...100,
                                                history: "Transform Vertical")
    public static let horizontal = DevelopControl("Horizontal", ["geometry", "transform", "horizontal"], -100...100,
                                                  history: "Transform Horizontal")
    public static let rotate = DevelopControl("Rotate", ["geometry", "transform", "rotate"], -10...10, step: 0.1,
                                              format: "%+.1f°", history: "Transform Rotate")
    public static let aspect = DevelopControl("Aspect", ["geometry", "transform", "aspect"], -100...100,
                                              history: "Transform Aspect")
    public static let scale = DevelopControl("Scale", ["geometry", "transform", "scale"], 50...150, default: 100,
                                             format: "%.0f%%", history: "Transform Scale")
    public static let offsetX = DevelopControl("Offset X", ["geometry", "transform", "offset_x"], -100...100, step: 0.1,
                                               format: "%+.1f", history: "Transform Offset X")
    public static let offsetY = DevelopControl("Offset Y", ["geometry", "transform", "offset_y"], -100...100, step: 0.1,
                                               format: "%+.1f", history: "Transform Offset Y")
    public static let all = [vertical, horizontal, rotate, aspect, scale, offsetX, offsetY]

    /// The Transform group's reset: every manual slider back to neutral in one patch.
    public static var resetPatch: [String: Any] {
        var obj: [String: Any] = [:]
        for c in all { obj[c.path.last!] = c.defaultValue }
        return ["geometry": ["transform": obj]]
    }
}

extension DevelopController {
    /// Value at a member path of a settings document.
    nonisolated public static func value(in settings: [String: Any], at path: [String]) -> Any? {
        var node: Any? = settings
        for key in path { node = (node as? [String: Any])?[key] }
        return node
    }
}
