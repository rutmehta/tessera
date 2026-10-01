import CoreGraphics
import Foundation
import IOSurface

/// The backend supplies document-encoded RGBA8. Surface/CGImage tags describe those bytes;
/// the Metal layer describes the values *after* texture decoding and compositing.
public struct DocumentDisplayColor: @unchecked Sendable {
    /// Original encoded profile: used by IOSurface metadata and all four detail panes.
    public let space: CGColorSpace
    /// Untagged / engine built-in sRGB (including the existing extended-linear EDR configuration).
    public let isSRGB: Bool
    /// Whether Metal may use `rgba8Unorm_srgb`: all three TRCs must match sRGB.
    public let decodesSRGB: Bool
    /// The document's linearized twin when decoding, otherwise its original encoding.
    public let layerSpace: CGColorSpace
    public let diagnostic: String?

    public static let srgb = DocumentDisplayColor(space: CGColorSpace(name: CGColorSpace.sRGB)!, isSRGB: true,
                                                  decodesSRGB: true,
                                                  layerSpace: CGColorSpace(name: CGColorSpace.extendedLinearSRGB)!,
                                                  diagnostic: nil)

    /// `icc`: `DocumentBackend.displayProfileICC()` (nil = sRGB); `name`: diagnostic label only.
    /// Linearization can fail for non-matrix profiles. Preserve their encoded path, with a diagnostic.
    public static func resolve(icc: Data?, name: String?,
                               linearize: (CGColorSpace) -> CGColorSpace? = { CGColorSpaceCreateLinearized($0) }) -> DocumentDisplayColor {
        guard let icc else { return .srgb }
        let label = name ?? "embedded"
        guard let space = CGColorSpace(iccData: icc as CFData) else {
            return fallback("the colour profile “\(label)” cannot be read by macOS")
        }
        guard space.model == .rgb, space.numberOfComponents == 3 else {
            return fallback("the colour profile “\(label)” is not an RGB profile")
        }
        guard let twin = linearize(space) else {
            return DocumentDisplayColor(space: space, isSRGB: false, decodesSRGB: false, layerSpace: space,
                diagnostic: "Display: macOS cannot linearize the colour profile “\(label)”; interpolation and transparency use its encoded space")
        }
        let decode = matchesSRGBCurve(space, linearized: twin)
        // Non-sRGB TRCs deliberately retain B5-30's encoded interpolation/blending limitation.
        return DocumentDisplayColor(space: space, isSRGB: false, decodesSRGB: decode,
                                    layerSpace: decode ? twin : space, diagnostic: nil)
    }

    /// Sample a grey ramp into the same primaries with linear TRCs. Checking every RGB component
    /// covers per-channel curves, whether the ICC uses parametric tags or sampled tables (Apple/HP).
    /// Maximum absolute linear-light error: 0.001 at each of 256 samples. Gamma 2.2 differs by >0.008
    /// and fails this check; profile descriptions, dates and tag layout play no role.
    private static func matchesSRGBCurve(_ space: CGColorSpace, linearized: CGColorSpace) -> Bool {
        for i in 0...255 {
            let x = CGFloat(i) / 255
            let expected = x <= 0.04045 ? x / 12.92 : pow((x + 0.055) / 1.055, 2.4)
            guard let actual = CGColor(colorSpace: space, components: [x, x, x, 1])?
                .converted(to: linearized, intent: .relativeColorimetric, options: nil)?.components,
                  actual.count >= 3,
                  actual.prefix(3).allSatisfy({ $0.isFinite && abs($0 - expected) <= 0.001 }) else { return false }
        }
        return true
    }

    private static func fallback(_ why: String) -> DocumentDisplayColor {
        DocumentDisplayColor(space: srgb.space, isSRGB: true, decodesSRGB: true, layerSpace: srgb.layerSpace,
                             diagnostic: "Display: \(why); the canvas shows it as sRGB")
    }

    /// The ICC bytes of the encoded document space.
    public var iccData: Data? { space.copyICCData() as Data? }

    /// Tags the original samples, not the decoded Metal texture view (metadata only).
    public func tag(_ surface: IOSurfaceRef) {
        guard let icc = space.copyICCData() else { return }
        IOSurfaceSetValue(surface, kIOSurfaceColorSpace, icc)
    }
}
