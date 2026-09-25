import Foundation

/// A screen's extended-dynamic-range capability: `NSScreen` in the app, mocked in tests.
public protocol EDRScreen {
    /// `maximumExtendedDynamicRangeColorComponentValue`: the headroom available right now
    /// (1 = none). It follows the display's brightness and whether EDR content is on screen.
    var currentEDRHeadroom: Double { get }
    /// `maximumPotentialExtendedDynamicRangeColorComponentValue`: the display's ceiling
    /// (1 on SDR displays, 16 on a Pro Display XDR / Liquid Retina XDR).
    var potentialEDRHeadroom: Double { get }
}

/// A plain snapshot of an `EDRScreen`.
public struct EDRScreenValues: EDRScreen, Equatable, Sendable {
    public var currentEDRHeadroom: Double
    public var potentialEDRHeadroom: Double
    public init(current: Double, potential: Double) {
        currentEDRHeadroom = current
        potentialEDRHeadroom = potential
    }
    public init(_ screen: EDRScreen) {
        self.init(current: screen.currentEDRHeadroom, potential: screen.potentialEDRHeadroom)
    }
    /// An SDR display.
    public static let sdr = EDRScreenValues(current: 1, potential: 1)
}

/// The recipe's HDR controls (engine `output.hdr` / `output.hdr_headroom_stops`).
public enum HDRControls {
    public static let enabledPath = ["output", "hdr"]
    public static let stopsPath = ["output", "hdr_headroom_stops"]
    /// Recipe range of the headroom (`crs:HDRMaxValue`).
    public static let maxRecipeStops = 16.0

    /// The headroom slider: 0 stops (SDR tone-mapped) up to the display's potential.
    public static func headroom(maxStops: Double) -> DevelopControl {
        DevelopControl("Headroom", stopsPath, 0...max(maxStops, 0.1), step: 0.1, format: "%.1f EV",
                       history: "HDR Headroom")
    }
}

/// How the loupe presents engine frames on one screen (docs/07 §6, M2-22).
///
/// EDR-capable screen and the recipe's HDR toggle on: an RGBA16F ring of display-linear
/// extended sRGB, tone-mapped by the engine for `min(2^stops, current headroom)`. Everything
/// else (SDR screens, HDR off) keeps the RGBA8 display-encoded sRGB ring and today's SDR path.
public struct EDRPresentation: Equatable, Sendable {
    /// Allocate the RGBA16F (`'RGhA'`) ring and present it as linear half floats.
    public var floatSurfaces: Bool
    /// Headroom to report to the engine now (linear multiple of SDR white, ≥ 1).
    public var displayHeadroom: Double
    /// The screen's EDR ceiling (≥ 1).
    public var potentialHeadroom: Double

    /// Anything above this counts as EDR capable (screens report exactly 1.0 otherwise).
    public static let capabilityThreshold = 1.01

    public static let sdr = EDRPresentation(floatSurfaces: false, displayHeadroom: 1, potentialHeadroom: 1)

    /// Whether the screen can show values above SDR white at all.
    public var isEDRCapable: Bool { potentialHeadroom > Self.capabilityThreshold }

    /// Upper end of the headroom slider, in stops above SDR white (0 on SDR screens).
    public var maxStops: Double {
        isEDRCapable ? (log2(potentialHeadroom) * 10).rounded(.down) / 10 : 0
    }

    /// Resolves the presentation for `screen` (nil: no window yet → SDR).
    public static func resolve(screen: EDRScreen?, hdrEnabled: Bool) -> EDRPresentation {
        guard let screen else { return .sdr }
        let sane = { (v: Double) in v.isFinite ? max(v, 1) : 1 }
        let potential = sane(screen.potentialEDRHeadroom)
        // The current headroom never exceeds the potential; before EDR content is on screen
        // macOS may still report 1 (the engine then tone-maps for SDR white until it rises).
        let current = min(sane(screen.currentEDRHeadroom), potential)
        let capable = potential > capabilityThreshold
        return EDRPresentation(floatSurfaces: capable && hdrEnabled,
                               displayHeadroom: capable ? current : 1,
                               potentialHeadroom: potential)
    }

    /// The peak the engine tone-maps float frames for, given the recipe's headroom in stops
    /// (mirrors `tessera_ffi::presentation`): 0 for the SDR (RGBA8) path.
    public func effectiveHeadroom(stops: Double) -> Double {
        guard floatSurfaces else { return 0 }
        let s = stops.isFinite ? min(max(stops, 0), HDRControls.maxRecipeStops) : 0
        return min(pow(2, s), displayHeadroom)
    }

    /// Stops to use when the HDR toggle is switched on with no headroom stored: all of it.
    public var defaultStops: Double { maxStops }

    /// Whether a new current headroom differs enough to re-render (> ~5 %): brightness
    /// changes move it continuously.
    public static func headroomChanged(_ a: Double, _ b: Double) -> Bool {
        abs(log2(max(a, 1)) - log2(max(b, 1))) > 0.07
    }

    /// Inspector readout, e.g. "EDR 8.0× now, 16.0× max".
    public var readout: String {
        guard isEDRCapable else { return "SDR display" }
        return String(format: "EDR %.1f× now, %.1f× max", displayHeadroom, potentialHeadroom)
    }
}
