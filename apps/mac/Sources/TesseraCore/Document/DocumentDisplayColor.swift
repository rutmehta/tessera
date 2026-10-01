import CoreGraphics
import Foundation
import IOSurface

/// B5-30: the colour space the document canvas tags its 8-bit surfaces with. The backend writes the
/// document's own ENCODED samples (no conversion, no per-pixel work); tagging them with the document
/// profile lets macOS colour-manage them to the display. `space` is sRGB for untagged documents and
/// the built-in sRGB profile (the canvas keeps its sRGB path), the document profile otherwise, and
/// sRGB again, with a `diagnostic`, for a profile CoreGraphics cannot use.
public struct DocumentDisplayColor: @unchecked Sendable {
    public let space: CGColorSpace
    /// The sRGB path: `rgba8Unorm_srgb` into an extended linear sRGB layer (as before B5-30).
    public let isSRGB: Bool
    /// Why a document profile fell back to sRGB (status bar), nil otherwise.
    public let diagnostic: String?

    public static let srgb = DocumentDisplayColor(space: CGColorSpace(name: CGColorSpace.sRGB)!, isSRGB: true,
                                                  diagnostic: nil)

    /// `icc`: `DocumentBackend.displayProfileICC()` (nil = sRGB); `name`: the profile's description.
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
        return DocumentDisplayColor(space: space, isSRGB: false, diagnostic: nil)
    }

    private static func fallback(_ why: String) -> DocumentDisplayColor {
        DocumentDisplayColor(space: srgb.space, isSRGB: true, diagnostic: "Display: \(why); the canvas shows it as sRGB")
    }

    /// The ICC bytes of `space`.
    public var iccData: Data? { space.copyICCData() as Data? }

    /// Tags `surface` with `space` (`kIOSurfaceColorSpace`): metadata only, the samples are untouched.
    public func tag(_ surface: IOSurfaceRef) {
        guard let icc = space.copyICCData() else { return }
        IOSurfaceSetValue(surface, kIOSurfaceColorSpace, icc)
    }
}
