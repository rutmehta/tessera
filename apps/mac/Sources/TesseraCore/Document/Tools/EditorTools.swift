import Foundation

// UI-free parts of the layered editor's tools (WP M5-11): tool titles, keys and groups, the tool key
// map, brush HUD maths, selection modifier mapping, the pressure curve and per-frame stroke
// coalescing. Everything here is unit tested (DocumentToolsTests).

extension DocumentTool {
    public var title: String {
        switch self {
        case .move: "Move"
        case .marquee: "Rectangular Marquee"
        case .ellipseMarquee: "Elliptical Marquee"
        case .lasso: "Lasso"
        case .polygonLasso: "Polygonal Lasso"
        case .magneticLasso: "Magnetic Lasso"
        case .quickSelect: "Quick Selection"
        case .wand: "Magic Wand"
        case .objectSelect: "Object Selection"
        case .brush: "Brush"
        case .eraser: "Eraser"
        case .cloneStamp: "Clone Stamp"
        case .heal: "Healing Brush"
        case .gradient: "Gradient"
        case .crop: "Crop"
        case .type: "Type"
        case .eyedropper: "Eyedropper"
        case .hand: "Hand"
        case .zoom: "Zoom"
        }
    }

    public var key: String {
        switch self {
        case .move: "V"
        case .marquee, .ellipseMarquee: "M"
        case .lasso, .polygonLasso, .magneticLasso: "L"
        case .quickSelect, .wand, .objectSelect: "W"
        case .brush: "B"
        case .eraser: "E"
        case .cloneStamp: "S"
        case .heal: "J"
        case .gradient: "G"
        case .crop: "C"
        case .type: "T"
        case .eyedropper: "I"
        case .hand: "H"
        case .zoom: "Z"
        }
    }

    public var symbol: String {
        switch self {
        case .move: "arrow.up.and.down.and.arrow.left.and.right"
        case .marquee: "rectangle.dashed"
        case .ellipseMarquee: "circle.dashed"
        case .lasso: "lasso"
        case .polygonLasso: "skew"
        case .magneticLasso: "lasso.badge.sparkles"
        case .quickSelect: "paintbrush.pointed"
        case .wand: "wand.and.stars"
        case .objectSelect: "square.dashed.inset.filled"
        case .brush: "paintbrush"
        case .eraser: "eraser"
        case .cloneStamp: "seal"
        case .heal: "bandage"
        case .gradient: "square.bottomhalf.filled"
        case .crop: "crop"
        case .type: "textformat"
        case .eyedropper: "eyedropper"
        case .hand: "hand.raised"
        case .zoom: "magnifyingglass"
        }
    }

    /// Tools that share a key and a palette slot; ⇧ + the key cycles through them.
    public var group: [DocumentTool] {
        switch self {
        case .marquee, .ellipseMarquee: [.marquee, .ellipseMarquee]
        case .lasso, .polygonLasso, .magneticLasso: [.lasso, .polygonLasso, .magneticLasso]
        case .quickSelect, .wand, .objectSelect: [.quickSelect, .wand, .objectSelect]
        default: [self]
        }
    }

    /// One tool per palette slot, in Photoshop's order.
    public static let paletteSlots: [DocumentTool] = [
        .move, .marquee, .lasso, .quickSelect, .crop, .eyedropper, .heal, .brush, .cloneStamp, .eraser, .gradient,
        .type, .hand, .zoom,
    ]

    /// Painting tools (brush options, cursor outline, HUD).
    public var paints: Bool { [.brush, .eraser, .cloneStamp, .heal].contains(self) }
    /// Selection tools (⇧ / ⌥ / ⇧⌥ modifiers, marching ants).
    public var selects: Bool {
        [.marquee, .ellipseMarquee, .lasso, .polygonLasso, .magneticLasso, .quickSelect, .wand, .objectSelect].contains(self)
    }
    /// Placeholders that explain themselves on click.
    public var isPlaceholder: Bool { [.crop, .type].contains(self) }

    public var strokeKind: BrushToolKind? {
        switch self {
        case .brush: .brush
        case .eraser: .eraser
        case .cloneStamp: .clone
        case .heal: .heal
        default: nil
        }
    }
}

/// What a plain (non-⌘) key does to the document tools.
public enum ToolKeyAction: Equatable, Sendable {
    case tool(DocumentTool)
    /// `[` / `]`.
    case brushSize(larger: Bool)
    /// `⇧[` / `⇧]`.
    case brushHardness(harder: Bool)
    /// 0–9: opacity (of the brush, or of the layer with a non-painting tool).
    case opacityDigit(Int)
    /// X
    case swapColors
    /// D
    case defaultColors
    /// Return / Enter.
    case commit
    /// Esc.
    case cancel
}

public enum ToolKeyMap {
    /// `keyCode` is the hardware key (36 Return, 76 Enter, 53 Esc); `characters` ignores modifiers.
    public static func action(keyCode: UInt16, characters: String, mods: DocumentKeyMap.Mods,
                              current: DocumentTool) -> ToolKeyAction? {
        if mods.contains(.command) || mods.contains(.control) || mods.contains(.option) { return nil }
        switch keyCode {
        case 36, 76: return .commit
        case 53: return .cancel
        default: break
        }
        let shift = mods.contains(.shift)
        let ch = characters.lowercased()
        switch ch {
        case "[", "{": return shift ? .brushHardness(harder: false) : .brushSize(larger: false)
        case "]", "}": return shift ? .brushHardness(harder: true) : .brushSize(larger: true)
        case "x" where !shift: return .swapColors
        case "d" where !shift: return .defaultColors
        default: break
        }
        if !shift, ch.count == 1, let d = Int(ch) { return .opacityDigit(d) }
        guard let first = DocumentTool.allCases.first(where: { $0.key.lowercased() == ch }) else { return nil }
        let group = first.group
        if let i = group.firstIndex(of: current) {
            return .tool(shift ? group[(i + 1) % group.count] : current)
        }
        return .tool(first)
    }
}

/// The brush HUD and bracket keys (spec 02 §4): sizes, hardness, opacity digits.
public enum BrushHUDMath {
    public static let sizeRange: ClosedRange<Float> = 1...5000

    /// Photoshop-like bracket steps: finer for small brushes.
    public static func sizeStep(_ size: Float) -> Float {
        switch size {
        case ..<10: 1
        case ..<50: 5
        case ..<100: 10
        case ..<200: 25
        case ..<500: 50
        case ..<1000: 100
        default: 250
        }
    }

    public static func bracket(size: Float, larger: Bool) -> Float {
        if larger { return min(size + sizeStep(size), sizeRange.upperBound) }
        // Step down by the step that steps back up to `size`, so up and down retrace each other.
        let candidates: [Float] = [250, 100, 50, 25, 10, 5, 1]
        var step = sizeStep(size)
        for s in candidates where size - s >= 1 && sizeStep(size - s) == s {
            step = s
            break
        }
        return max(size - step, sizeRange.lowerBound)
    }

    /// `⇧[` / `⇧]`: hardness in quarters.
    public static func bracket(hardness: Float, harder: Bool) -> Float {
        let q = (hardness * 4).rounded() + (harder ? 1 : -1)
        return min(max(q / 4, 0), 1)
    }

    /// ⌃⌥-drag (or ⌥-right-drag): horizontal changes the diameter so the outline follows the pointer
    /// (2 × the distance, in canvas pixels at `zoom` screen points per pixel); vertical changes
    /// hardness (up = harder, 200 pt for the full range).
    public static func drag(startSize: Float, startHardness: Float, dx: Double, dy: Double, zoom: Double)
        -> (size: Float, hardness: Float) {
        let z = max(zoom, 1e-6)
        let size = Float(Double(startSize) + 2 * dx / z)
        let hardness = startHardness - Float(dy / 200)
        return (min(max(size.rounded(), sizeRange.lowerBound), sizeRange.upperBound), min(max(hardness, 0), 1))
    }

    /// Number keys set opacity: 1 = 10 %, …, 0 = 100 %; two digits typed quickly set the exact
    /// value (4 then 5 = 45 %, 0 then 0 = 0 %).
    public struct OpacityKeys: Sendable {
        public var window: TimeInterval = 0.6
        private var last: (digit: Int, time: TimeInterval)?
        public init() {}

        public mutating func press(_ digit: Int, at time: TimeInterval) -> Float {
            if let l = last, time - l.time <= window {
                last = nil
                return Float(l.digit * 10 + digit) / 100
            }
            last = (digit, time)
            return digit == 0 ? 1 : Float(digit) / 10
        }
    }
}

/// ⇧ add, ⌥ subtract, ⇧⌥ intersect; otherwise the options bar's mode.
public enum SelectionModifiers {
    public static func combine(shift: Bool, option: Bool, optionsBar: SelectionCombine = .replace) -> SelectionCombine {
        switch (shift, option) {
        case (true, true): .intersect
        case (true, false): .add
        case (false, true): .subtract
        case (false, false): optionsBar
        }
    }

    /// Quick Selection adds by default (⌥ subtracts), as in Photoshop.
    public static func quickSelect(option: Bool, firstStroke: Bool) -> SelectionCombine {
        option ? .subtract : (firstStroke ? .replace : .add)
    }

    /// Marquee geometry: ⇧ held after the drag starts constrains to a square / circle, ⌥ draws from
    /// the centre. Returns the canvas rectangle between `start` and `current`.
    public static func marqueeRect(start: CGPoint, current: CGPoint, square: Bool, fromCenter: Bool) -> CGRect {
        var dx = current.x - start.x, dy = current.y - start.y
        if square {
            let m = max(abs(dx), abs(dy))
            dx = dx < 0 ? -m : m
            dy = dy < 0 ? -m : m
        }
        if fromCenter {
            return CGRect(x: start.x - abs(dx), y: start.y - abs(dy), width: 2 * abs(dx), height: 2 * abs(dy))
        }
        return CGRect(x: min(start.x, start.x + dx), y: min(start.y, start.y + dy), width: abs(dx), height: abs(dy))
    }
}

/// Maps raw tablet pressure to the value the brush uses: `minimum + (1 − minimum) · p^gamma`
/// (gamma < 1 is a soft curve, > 1 firm). Mouse events report pressure 1 while the button is down.
public struct PressureCurve: Equatable, Sendable {
    public var gamma: Double = 1
    public var minimum: Double = 0
    public init(gamma: Double = 1, minimum: Double = 0) { self.gamma = gamma; self.minimum = minimum }

    public func map(_ raw: Double) -> Float {
        let p = min(max(raw.isFinite ? raw : 1, 0), 1)
        let m = min(max(minimum, 0), 1)
        return Float(m + (1 - m) * pow(p, max(gamma, 0.05)))
    }
}

/// Collects pointer samples between engine calls: every sample that arrives while a
/// `stroke_points` call is in flight joins the next batch, so the engine gets one call per
/// presented frame however fast the tablet reports. Samples closer than `minSpacing` canvas pixels
/// to the previous one are dropped (the brush's own spacing places dabs).
public struct FrameStrokeCoalescer: Sendable {
    public var minSpacing: Float
    public private(set) var pending: [PenSample] = []
    public private(set) var inFlight = false
    private var last: PenSample?
    /// Calls sent and samples accepted (diagnostics).
    public private(set) var batches = 0
    public private(set) var accepted = 0

    public init(minSpacing: Float = 0.5) { self.minSpacing = minSpacing }

    public mutating func add(_ s: PenSample) {
        if let l = last, hypotf(s.x - l.x, s.y - l.y) < minSpacing, s.pressure == l.pressure { return }
        last = s
        pending.append(s)
        accepted += 1
    }

    /// The next batch, unless one is in flight or nothing is pending.
    public mutating func nextBatch() -> [PenSample]? {
        guard !inFlight, !pending.isEmpty else { return nil }
        inFlight = true
        batches += 1
        defer { pending.removeAll(keepingCapacity: true) }
        return pending
    }

    /// The in-flight batch finished.
    public mutating func batchDone() { inFlight = false }

    /// Everything still pending (stroke end), regardless of the in-flight batch.
    public mutating func drain() -> [PenSample] {
        defer { pending.removeAll() }
        return pending
    }
}

/// Foreground / background colours (X swaps, D resets to black / white).
public struct ToolColors: Equatable, Sendable {
    public var foreground: ToolColor = .black
    public var background: ToolColor = .white
    public init() {}
    public mutating func swap() { (foreground, background) = (background, foreground) }
    public mutating func reset() { foreground = .black; background = .white }
}
