import Foundation

/// OkLab / OkLCh helpers matching the engine's colour operators, for the HSL targeted
/// adjustment and for drawing hue swatches and grading wheels in the engine's own hue space.
public enum OkLab {
    /// sRGB OETF inverse.
    public static func linear(_ c: Double) -> Double {
        c <= 0.04045 ? c / 12.92 : pow((c + 0.055) / 1.055, 2.4)
    }

    public static func encode(_ c: Double) -> Double {
        let c = min(max(c, 0), 1)
        return c <= 0.0031308 ? 12.92 * c : 1.055 * pow(c, 1 / 2.4) - 0.055
    }

    /// Linear sRGB → OkLab (Björn Ottosson's published matrices).
    public static func fromLinearSRGB(_ r: Double, _ g: Double, _ b: Double) -> (l: Double, a: Double, b: Double) {
        let l = cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b)
        let m = cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b)
        let s = cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b)
        return (0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
                1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
                0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s)
    }

    public static func toLinearSRGB(_ L: Double, _ a: Double, _ b: Double) -> (r: Double, g: Double, b: Double) {
        let l = pow(L + 0.3963377774 * a + 0.2158037573 * b, 3)
        let m = pow(L - 0.1055613458 * a - 0.0638541728 * b, 3)
        let s = pow(L - 0.0894841775 * a - 1.2914855480 * b, 3)
        return (4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
                -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
                -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s)
    }

    /// Hue (degrees, [0, 360)) and chroma of display-encoded sRGB bytes.
    public static func lch(r: UInt8, g: UInt8, b: UInt8) -> (l: Double, c: Double, h: Double) {
        let lab = fromLinearSRGB(linear(Double(r) / 255), linear(Double(g) / 255), linear(Double(b) / 255))
        var h = atan2(lab.b, lab.a) * 180 / .pi
        if h < 0 { h += 360 }
        return (lab.l, hypot(lab.a, lab.b), h)
    }

    /// Display sRGB (0…1 components) of an OkLCh colour, chroma reduced until it fits the gamut.
    public static func srgb(l: Double, c: Double, hue: Double) -> (r: Double, g: Double, b: Double) {
        var c = c
        let h = hue * .pi / 180
        for _ in 0..<24 {
            let rgb = toLinearSRGB(l, c * cos(h), c * sin(h))
            if [rgb.r, rgb.g, rgb.b].allSatisfy({ $0 >= -0.0005 && $0 <= 1.0005 }) {
                return (encode(rgb.r), encode(rgb.g), encode(rgb.b))
            }
            c *= 0.85
        }
        let rgb = toLinearSRGB(l, 0, 0)
        return (encode(rgb.r), encode(rgb.g), encode(rgb.b))
    }
}

/// The HSL mixer's band membership: raised-cosine weights between adjacent band centres,
/// wrapping at 360° (pipeline-cpu `hue_weights`). Weights sum to one.
public enum HueBandWeights {
    public static func weights(hue: Double) -> [HueBand: Double] {
        let centers = HueBand.allCases.map(\.center) + [385]
        let h = (hue - 25).truncatingRemainder(dividingBy: 360)
        let x = (h < 0 ? h + 360 : h) + 25
        var out: [HueBand: Double] = [:]
        for i in 0..<8 where x >= centers[i] && x <= centers[i + 1] {
            let t = (x - centers[i]) / (centers[i + 1] - centers[i])
            let w = 0.5 - 0.5 * cos(.pi * t)
            out[HueBand.allCases[i], default: 0] += 1 - w
            out[HueBand.allCases[(i + 1) % 8], default: 0] += w
            break
        }
        return out.filter { $0.value > 0 }
    }
}

/// Targeted adjustment for the HSL panel: the colour under the cursor picks the bands (by the
/// engine's own weights) and a vertical drag moves each of them in proportion.
public struct TargetedHSLAdjustment: Sendable {
    public let property: HSLProperty
    /// Band → (weight, value at drag start).
    public let bands: [HueBand: (weight: Double, start: Double)]
    /// Neutral samples carry no hue; the engine skips HSL membership for them too.
    public var isEmpty: Bool { bands.isEmpty }

    /// `start` reads the current slider values. Chroma below `minChroma` is treated as neutral.
    public init(property: HSLProperty, sample: (r: UInt8, g: UInt8, b: UInt8),
                start: (HueBand) -> Double, minChroma: Double = 0.02) {
        self.property = property
        let lch = OkLab.lch(r: sample.r, g: sample.g, b: sample.b)
        guard lch.c >= minChroma else { bands = [:]; return }
        var b: [HueBand: (weight: Double, start: Double)] = [:]
        for (band, w) in HueBandWeights.weights(hue: lch.h) { b[band] = (w, start(band)) }
        bands = b
    }

    /// Merge patch for a drag of `delta` slider units (up = positive) on the dominant band.
    public func patch(delta: Double) -> [String: Any] {
        let top = bands.values.map(\.weight).max() ?? 1
        var values: [String: Any] = [:]
        for (band, e) in bands {
            let v = min(max(e.start + delta * e.weight / top, -100), 100)
            values[band.rawValue] = (v * 10).rounded() / 10
        }
        return ["color": ["hsl": [property.rawValue: values]]]
    }

    /// The band with the largest weight (for the history label).
    public var dominant: HueBand? { bands.max { $0.value.weight < $1.value.weight }?.key }
}
