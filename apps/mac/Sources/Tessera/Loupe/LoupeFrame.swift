import CoreGraphics
import CoreVideo
import IOSurface
import Metal

/// What the loupe presents: an IOSurface plus how to map it to the screen.
///
/// Two producers meet here. The engine (`DevelopController`) renders into surfaces it shares with
/// us and hands over only the surface id, so pixels never cross UniFFI (docs/11 §1.2): RGBA8
/// display-encoded sRGB, or on an EDR screen with HDR on, RGBA16F display-linear extended sRGB
/// whose values above 1.0 are EDR headroom (M2-22). Before the first engine frame (and for
/// JPEGs), a CGImage (embedded preview) is colour-converted into a half-float surface in the
/// screen's working space.
struct LoupeFrame: @unchecked Sendable {
    let surface: IOSurfaceRef
    /// Surface size in texels (sensor orientation for engine frames).
    let width: Int
    let height: Int
    /// Valid region, anchored top-left. Coarse progressive levels fill only part of the surface.
    let contentWidth: Int
    let contentHeight: Int
    /// The whole picture at the surface's level (sensor orientation): the surface size, or the
    /// cropped extent when the engine applied a crop. Drives the aspect fit.
    let fullWidth: Int
    let fullHeight: Int
    /// EXIF orientation applied when sampling (1 = as stored).
    let orientation: Int
    let pixelFormat: MTLPixelFormat
    /// Colour space the sampled (linearised) values are in; the layer adopts it.
    let colorSpace: CGColorSpace
    /// Rendered by the engine (the sRGB viewport contract; soft proof and mask overlays apply).
    let isEngineFrame: Bool

    /// Engine frame: RGBA8 sRGB-encoded, sampled through `.rgba8Unorm_srgb` as linear sRGB; or
    /// (`'RGhA'` surfaces) RGBA16F linear extended sRGB sampled as `.rgba16Float`.
    init(engineSurface surface: IOSurfaceRef, contentWidth: Int, contentHeight: Int,
         fullWidth: Int? = nil, fullHeight: Int? = nil, orientation: Int) {
        self.surface = surface
        width = IOSurfaceGetWidth(surface)
        height = IOSurfaceGetHeight(surface)
        self.contentWidth = min(contentWidth, width)
        self.contentHeight = min(contentHeight, height)
        self.fullWidth = max(1, min(fullWidth ?? width, width))
        self.fullHeight = max(1, min(fullHeight ?? height, height))
        self.orientation = (1...8).contains(orientation) ? orientation : 1
        pixelFormat = IOSurfaceGetPixelFormat(surface) == 0x5247_6841 ? .rgba16Float : .rgba8Unorm_srgb
        colorSpace = CGColorSpace(name: CGColorSpace.extendedLinearSRGB)!
        isEngineFrame = true
    }

    /// An EDR engine frame (values above 1.0 are headroom).
    var isEDR: Bool { isEngineFrame && pixelFormat == .rgba16Float }

    private init(rasterized surface: IOSurfaceRef, colorSpace: CGColorSpace) {
        self.surface = surface
        width = IOSurfaceGetWidth(surface)
        height = IOSurfaceGetHeight(surface)
        contentWidth = width
        contentHeight = height
        fullWidth = width
        fullHeight = height
        orientation = 1
        pixelFormat = .rgba16Float
        self.colorSpace = colorSpace
        isEngineFrame = false
    }

    /// Full-resolution size on screen after orientation (drives the aspect fit, so coarse
    /// levels are magnified rather than shown smaller).
    var displaySize: (width: Int, height: Int) { orientation >= 5 ? (fullHeight, fullWidth) : (fullWidth, fullHeight) }

    /// Reads the display-encoded RGB of an engine frame around stored-orientation texel `(x, y)`
    /// (3×3 mean). EDR frames report their SDR-clipped encoding. Nil for preview frames.
    func sampleRGB8(x: Int, y: Int) -> (r: UInt8, g: UInt8, b: UInt8)? {
        guard isEngineFrame, contentWidth > 0, contentHeight > 0 else { return nil }
        IOSurfaceLock(surface, .readOnly, nil)
        defer { IOSurfaceUnlock(surface, .readOnly, nil) }
        if pixelFormat == .rgba16Float {
            let base = IOSurfaceGetBaseAddress(surface).assumingMemoryBound(to: Float16.self)
            let stride = IOSurfaceGetBytesPerRow(surface) / 2
            var sum: (Float, Float, Float) = (0, 0, 0), n: Float = 0
            for dy in -1...1 {
                for dx in -1...1 {
                    let px = min(max(x + dx, 0), contentWidth - 1), py = min(max(y + dy, 0), contentHeight - 1)
                    let p = base + py * stride + px * 4
                    sum.0 += Float(p[0]); sum.1 += Float(p[1]); sum.2 += Float(p[2]); n += 1
                }
            }
            let enc = { (v: Float) -> UInt8 in
                let l = min(max(v / n, 0), 1)
                let e = l <= 0.0031308 ? 12.92 * l : 1.055 * powf(l, 1 / 2.4) - 0.055
                return UInt8((e * 255).rounded())
            }
            return (enc(sum.0), enc(sum.1), enc(sum.2))
        }
        let base = IOSurfaceGetBaseAddress(surface).assumingMemoryBound(to: UInt8.self)
        let stride = IOSurfaceGetBytesPerRow(surface)
        var sum = (0, 0, 0), n = 0
        for dy in -1...1 {
            for dx in -1...1 {
                let px = min(max(x + dx, 0), contentWidth - 1), py = min(max(y + dy, 0), contentHeight - 1)
                let p = base + py * stride + px * 4
                sum.0 += Int(p[0]); sum.1 += Int(p[1]); sum.2 += Int(p[2]); n += 1
            }
        }
        return (UInt8(sum.0 / n), UInt8(sum.1 / n), UInt8(sum.2 / n))
    }

    /// Preview path: colour-convert `image` into `colorSpace` (the loupe's working space derived from
    /// the screen) as RGBA half-float in a new IOSurface. CoreGraphics performs the ICC conversion.
    static func rasterize(_ image: CGImage, into colorSpace: CGColorSpace) -> LoupeFrame? {
        let w = image.width, h = image.height
        guard w > 0, h > 0 else { return nil }
        let bytesPerElement = 8   // RGBA16F
        let props: [CFString: Any] = [
            kIOSurfaceWidth: w,
            kIOSurfaceHeight: h,
            kIOSurfaceBytesPerElement: bytesPerElement,
            kIOSurfaceBytesPerRow: IOSurfaceAlignProperty(kIOSurfaceBytesPerRow, w * bytesPerElement),
            kIOSurfacePixelFormat: kCVPixelFormatType_64RGBAHalf,
        ]
        guard let surface = IOSurfaceCreate(props as CFDictionary) else { return nil }
        IOSurfaceLock(surface, [], nil)
        defer { IOSurfaceUnlock(surface, [], nil) }
        let info = CGImageAlphaInfo.premultipliedLast.rawValue
            | CGBitmapInfo.floatComponents.rawValue
            | CGBitmapInfo.byteOrder16Little.rawValue
        guard let ctx = CGContext(data: IOSurfaceGetBaseAddress(surface), width: w, height: h,
                                  bitsPerComponent: 16, bytesPerRow: IOSurfaceGetBytesPerRow(surface),
                                  space: colorSpace, bitmapInfo: info)
        else { return nil }
        ctx.interpolationQuality = .none
        // CG bitmap memory is top row first, which matches Metal's top-left texture origin: no flip.
        ctx.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
        return LoupeFrame(rasterized: surface, colorSpace: colorSpace)
    }

    func makeTexture(device: MTLDevice) -> MTLTexture? {
        let desc = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: pixelFormat, width: width, height: height, mipmapped: false)
        desc.usage = .shaderRead
        desc.storageMode = device.hasUnifiedMemory ? .shared : .managed
        return device.makeTexture(descriptor: desc, iosurface: surface, plane: 0)
    }
}
