import Foundation
import TesseraFFI

// Masking (M2-14): the UI-free parts of the Masks panel and the loupe mask tools. Mask-space
// coordinates are the engine's: normalised to the full, uncropped image in sensor orientation.

/// Maps between the loupe's displayed picture (cropped, EXIF-oriented, `[0, 1]` uv) and mask space.
/// Follows the engine geometry through `CropGeometry` (displayed orientation, pixel metric).
public struct MaskSpace: Sendable {
    public let orientation: Int
    /// The drawn crop (nil or identity: the whole frame is shown, e.g. in the crop tool).
    public let crop: CropGeometry?

    public init(orientation: Int, crop: CropGeometry?) {
        self.orientation = (1...8).contains(orientation) ? orientation : 1
        self.crop = crop.flatMap { $0.isIdentity ? nil : $0 }
    }

    /// Displayed-picture uv → mask space.
    public func toMask(_ u: Double, _ v: Double) -> (x: Double, y: Double) {
        let full: (u: Double, v: Double) = crop.map { $0.imageUV(fromCropUV: u, v) } ?? (u, v)
        let s = CropGeometry.orient(full.u, full.v, orientation)
        return (s.0, s.1)
    }

    /// Mask space → displayed-picture uv (may fall outside `[0, 1]` when cropped away).
    public func fromMask(_ x: Double, _ y: Double) -> (u: Double, v: Double) {
        let d = Self.unorient(x, y, orientation)
        guard let g = crop else { return d }
        let q = g.cropPoint(x: d.u * g.width, y: d.v * g.height)
        return (q.qx / g.cropWidth + 0.5, q.qy / g.cropHeight + 0.5)
    }

    /// Stored (sensor) uv → displayed uv: the inverse of `CropGeometry.orient`.
    public static func unorient(_ x: Double, _ y: Double, _ o: Int) -> (u: Double, v: Double) {
        let r = CropGeometry.unorient(x, y, o)
        return (r.0, r.1)
    }

    /// Brush radius in mask units (fraction of the sensor-orientation image width) for a radius of
    /// `displayedFraction` of the displayed picture's width.
    public func maskRadius(displayedFraction f: Double, imageWidth: Double, imageHeight: Double) -> Double {
        // Displayed picture width in full-image pixels, then in sensor-width units.
        let displayedWidth = crop?.cropWidth ?? (orientation >= 5 ? imageHeight : imageWidth)
        return f * displayedWidth / imageWidth
    }
}

/// Collects brush samples between display frames: the engine receives at most one batch per frame.
/// Samples closer than `spacing` (mask units) to the last kept one are dropped, except the last
/// sample of a stroke, which `finish` always keeps.
public struct StrokeCoalescer: Sendable {
    public var spacing: Double
    private var pending: [BrushPoint] = []
    private var last: BrushPoint?
    private var skipped: BrushPoint?
    public private(set) var sentCount = 0

    public init(spacing: Double) { self.spacing = spacing }

    public var isEmpty: Bool { pending.isEmpty }
    public var pendingCount: Int { pending.count }

    /// Records a sample; returns true when it will be sent.
    @discardableResult
    public mutating func add(x: Double, y: Double, pressure: Double = 1) -> Bool {
        let p = BrushPoint(x: Float(x), y: Float(y), pressure: Float(min(max(pressure, 0), 1)))
        if let l = last, hypot(Double(p.x - l.x), Double(p.y - l.y)) < spacing {
            skipped = p
            return false
        }
        pending.append(p)
        last = p
        skipped = nil
        return true
    }

    /// Samples to send now (one engine call per display frame).
    public mutating func take() -> [BrushPoint] {
        defer { sentCount += pending.count; pending.removeAll(keepingCapacity: true) }
        return pending
    }

    /// End of stroke: the final (possibly too-close) sample is kept so the stroke ends under the cursor.
    public mutating func finish() -> [BrushPoint] {
        if let s = skipped { pending.append(s); skipped = nil }
        let out = take()
        last = nil
        return out
    }
}

/// Loupe mask tools.
public enum MaskTool: String, CaseIterable, Sendable {
    case brush, linear, radial, colorRange, luminanceRange, object, person

    public var title: String {
        switch self {
        case .brush: "Brush"
        case .linear: "Linear Gradient"
        case .radial: "Radial Gradient"
        case .colorRange: "Color Range"
        case .luminanceRange: "Luminance Range"
        case .object: "Objects"
        case .person: "People"
        }
    }

    public var symbol: String {
        switch self {
        case .brush: "paintbrush.pointed"
        case .linear: "square.bottomhalf.filled"
        case .radial: "circle.dashed.inset.filled"
        case .colorRange: "eyedropper"
        case .luminanceRange: "sun.max"
        case .object: "cursorarrow.rays"
        case .person: "person.crop.rectangle"
        }
    }

    public var hint: String {
        switch self {
        case .brush: "Paint on the photo · ⌥ erases · [ ] size · ⇧[ ] feather"
        case .linear: "Drag from full effect to none · ⌥ subtracts from the mask"
        case .radial: "Drag from the centre outwards · ⌥ subtracts"
        case .colorRange: "Click a colour · ⇧-click adds a sample · ⌥ subtracts"
        case .luminanceRange: "Click a tone to select its brightness range · ⌥ subtracts"
        case .object: "Click an object or drag a box around it · ⌥ subtracts"
        case .person: "Drag a box around a face"
        }
    }
}

/// Mask overlay colours (O toggles, the toolbar picks).
public enum MaskOverlayColor: String, CaseIterable, Sendable {
    case red, green, blue, white, black

    public var rgb: (r: Float, g: Float, b: Float) {
        switch self {
        case .red: (0.95, 0.18, 0.18)
        case .green: (0.2, 0.85, 0.3)
        case .blue: (0.25, 0.45, 1.0)
        case .white: (1, 1, 1)
        case .black: (0, 0, 0)
        }
    }
}

/// One local slider of a mask group (engine `LocalParams` member).
public struct LocalParam: Identifiable, Hashable, Sendable {
    public let name: String
    public let title: String
    public let range: ClosedRange<Double>
    public let format: String
    public let step: Double
    public var id: String { name }

    public func historyLabel(_ v: Double) -> String { "\(title) " + String(format: format, v) }

    public static let sections: [(String, [LocalParam])] = [
        ("White Balance", [.init("temperature", "Temp"), .init("tint", "Tint")]),
        ("Light", [.init("exposure", "Exposure", -4...4, "%+.2f", 0.01), .init("contrast", "Contrast"),
                   .init("highlights", "Highlights"), .init("shadows", "Shadows"),
                   .init("whites", "Whites"), .init("blacks", "Blacks")]),
        ("Presence", [.init("texture", "Texture"), .init("clarity", "Clarity"), .init("dehaze", "Dehaze"),
                      .init("hue", "Hue", -180...180, "%+.0f°", 1), .init("saturation", "Saturation")]),
        ("Detail", [.init("sharpness", "Sharpness"), .init("noise", "Noise"), .init("moire", "Moiré")]),
    ]

    public static var all: [LocalParam] { sections.flatMap(\.1) }

    init(_ name: String, _ title: String, _ range: ClosedRange<Double> = -100...100,
         _ format: String = "%+.0f", _ step: Double = 1) {
        self.name = name; self.title = title; self.range = range; self.format = format; self.step = step
    }
}

/// The Masks panel's list: groups from the engine, the selection and AI progress. Pure state; the
/// app applies it to the session.
public struct MaskListState: Sendable {
    public private(set) var groups: [MaskGroupInfo] = []
    public private(set) var selectedID: UInt32?
    /// AI progress by raster key (`MaskComponentInfo.aiKey`).
    public private(set) var progress: [String: (fraction: Float, message: String)] = [:]

    public init() {}

    public var selected: MaskGroupInfo? { groups.first { $0.id == selectedID } }

    /// New engine list. Keeps the selection when it still exists; a newly added group becomes
    /// selected; otherwise the neighbour of a deleted selection is chosen.
    public mutating func update(_ next: [MaskGroupInfo]) {
        let before = Set(groups.map(\.id))
        let added = next.filter { !before.contains($0.id) }
        if let id = selectedID, !next.contains(where: { $0.id == id }) {
            let index = groups.firstIndex { $0.id == id } ?? 0
            selectedID = next.isEmpty ? nil : next[min(index, next.count - 1)].id
        }
        if !groups.isEmpty || selectedID == nil, let newest = added.last { selectedID = newest.id }
        groups = next
        for g in next {
            for c in g.components {
                if let key = c.aiKey, case .pending(let f, let m) = c.ai { progress[key] = (f, m) }
                else if let key = c.aiKey { progress.removeValue(forKey: key) }
            }
        }
    }

    public mutating func select(_ id: UInt32?) {
        selectedID = id.flatMap { i in groups.contains { $0.id == i } ? i : nil }
    }

    /// Cycles the selection (⌥↑/⌥↓-style navigation; wraps).
    public mutating func selectNext(_ step: Int) {
        guard !groups.isEmpty else { return }
        let i = groups.firstIndex { $0.id == selectedID } ?? (step > 0 ? -1 : 0)
        selectedID = groups[((i + step) % groups.count + groups.count) % groups.count].id
    }

    public mutating func progressUpdate(_ u: MaskJobUpdate) {
        if u.done { progress.removeValue(forKey: u.key) } else { progress[u.key] = (u.fraction, u.message) }
    }

    /// Any AI raster still computing.
    public var busy: Bool { !progress.isEmpty }

    /// The value of a local slider of the selected group.
    public func param(_ name: String) -> Double {
        Double(selected?.params.first { $0.name == name }?.value ?? 0)
    }

    /// Optimistic local update while a slider drags (the engine list refreshes on release).
    public mutating func setParam(_ name: String, _ value: Double) {
        guard let gi = groups.firstIndex(where: { $0.id == selectedID }),
              let pi = groups[gi].params.firstIndex(where: { $0.name == name }) else { return }
        groups[gi].params[pi].value = Float(value)
    }

    public mutating func setAmount(_ value: Double) {
        guard let gi = groups.firstIndex(where: { $0.id == selectedID }) else { return }
        groups[gi].amount = Float(value)
    }

    /// The brush component (paint target) of the selected group, if any.
    public var selectedBrushIndex: Int? {
        selected?.components.lastIndex { $0.kind == .brush && $0.combine == .add && !$0.invert }
    }
}

extension MaskComponentType {
    public var symbol: String {
        switch self {
        case .subject: "person.and.background.dotted"
        case .sky: "cloud.sun"
        case .background: "photo.on.rectangle"
        case .person: "person.crop.rectangle"
        case .object: "cube"
        case .landscape: "mountain.2"
        case .depth: "square.3.layers.3d"
        case .linear: "square.bottomhalf.filled"
        case .radial: "circle.dashed.inset.filled"
        case .brush: "paintbrush.pointed"
        case .luminanceRange: "sun.max"
        case .colorRange: "eyedropper"
        }
    }

    public var isAI: Bool { [.subject, .sky, .background, .person, .object, .landscape, .depth].contains(self) }
}

extension MaskCombineMode {
    public var title: String {
        switch self {
        case .add: "Add"
        case .subtract: "Subtract"
        case .intersect: "Intersect"
        }
    }
    public var sign: String {
        switch self {
        case .add: "+"
        case .subtract: "−"
        case .intersect: "∩"
        }
    }
}

// MARK: - Gradient geometry (the loupe handles)

/// A linear gradient: full effect at `start`, none at `end` (mask space).
public struct LinearGradientShape: Equatable, Sendable {
    public var start: (x: Double, y: Double)
    public var end: (x: Double, y: Double)

    public init(start: (x: Double, y: Double), end: (x: Double, y: Double)) { self.start = start; self.end = end }

    public static func == (a: Self, b: Self) -> Bool { a.start == b.start && a.end == b.end }

    public init?(json: String) {
        guard let o = Self.object(json), o["kind"] as? String == "linear",
              let s = o["start"] as? [NSNumber], let e = o["end"] as? [NSNumber], s.count == 2, e.count == 2 else { return nil }
        start = (s[0].doubleValue, s[1].doubleValue)
        end = (e[0].doubleValue, e[1].doubleValue)
    }

    public var json: String {
        DevelopController.encode(["kind": "linear", "start": [start.x, start.y], "end": [end.x, end.y]]) ?? "{}"
    }

    static func object(_ json: String) -> [String: Any]? {
        (try? JSONSerialization.jsonObject(with: Data(json.utf8))) as? [String: Any]
    }
}

/// An elliptical radial gradient (radii normalised to width/height, angle in degrees).
public struct RadialGradientShape: Equatable, Sendable {
    public var center: (x: Double, y: Double)
    public var radii: (x: Double, y: Double)
    public var angle: Double
    public var feather: Double

    public init(center: (x: Double, y: Double), radii: (x: Double, y: Double), angle: Double = 0, feather: Double = 50) {
        self.center = center; self.radii = radii; self.angle = angle; self.feather = feather
    }

    public static func == (a: Self, b: Self) -> Bool {
        a.center == b.center && a.radii == b.radii && a.angle == b.angle && a.feather == b.feather
    }

    public init?(json: String) {
        guard let o = LinearGradientShape.object(json), o["kind"] as? String == "radial",
              let c = o["center"] as? [NSNumber], let r = o["radii"] as? [NSNumber], c.count == 2, r.count == 2 else { return nil }
        center = (c[0].doubleValue, c[1].doubleValue)
        radii = (r[0].doubleValue, r[1].doubleValue)
        angle = (o["angle"] as? NSNumber)?.doubleValue ?? 0
        feather = (o["feather"] as? NSNumber)?.doubleValue ?? 50
    }

    public var json: String {
        DevelopController.encode(["kind": "radial", "center": [center.x, center.y], "radii": [radii.x, radii.y],
                                  "angle": angle, "feather": feather]) ?? "{}"
    }

    /// Outline points in mask space (for drawing through `MaskSpace`).
    public func outline(scale: Double = 1, segments: Int = 72) -> [(x: Double, y: Double)] {
        let a = angle * .pi / 180, (c, s) = (cos(a), sin(a))
        return (0...segments).map { i in
            let t = Double(i) / Double(segments) * 2 * .pi
            let (ex, ey) = (cos(t) * radii.x * scale, sin(t) * radii.y * scale)
            // The operator rotates in normalised space: d = R(p - c) / radii, so p = c + Rᵀ(e).
            return (center.x + c * ex - s * ey, center.y + s * ex + c * ey)
        }
    }
}
