import CoreGraphics
import CoreVideo
import IOSurface
import Metal

/// What the loupe presents: an IOSurface plus how to map it to the screen.
///
/// Two producers meet here. The engine (`DevelopController`) renders RGBA8 display-encoded sRGB
/// into surfaces it shares with us and hands over only the surface id, so pixels never cross
/// UniFFI (docs/11 §1.2). Before the first engine frame (and for JPEGs), a CGImage (embedded
/// preview) is colour-converted into a half-float surface in the screen's working space.
struct LoupeFrame: @unchecked Sendable {
    let surface: IOSurfaceRef
    /// Surface size in texels (sensor orientation for engine frames).
    let width: Int
    let height: Int
    /// Valid region, anchored top-left. Coarse progressive levels fill only part of the surface.
    let contentWidth: Int
    let contentHeight: Int
    /// EXIF orientation applied when sampling (1 = as stored).
    let orientation: Int
    let pixelFormat: MTLPixelFormat
    /// Colour space the sampled (linearised) values are in; the layer adopts it.
    let colorSpace: CGColorSpace

    /// Engine frame: RGBA8 sRGB-encoded, sampled through `.rgba8Unorm_srgb` as linear sRGB.
    init(engineSurface surface: IOSurfaceRef, contentWidth: Int, contentHeight: Int, orientation: Int) {
        self.surface = surface
        width = IOSurfaceGetWidth(surface)
        height = IOSurfaceGetHeight(surface)
        self.contentWidth = min(contentWidth, width)
        self.contentHeight = min(contentHeight, height)
        self.orientation = (1...8).contains(orientation) ? orientation : 1
        pixelFormat = .rgba8Unorm_srgb
        colorSpace = CGColorSpace(name: CGColorSpace.extendedLinearSRGB)!
    }

    private init(rasterized surface: IOSurfaceRef, colorSpace: CGColorSpace) {
        self.surface = surface
        width = IOSurfaceGetWidth(surface)
        height = IOSurfaceGetHeight(surface)
        contentWidth = width
        contentHeight = height
        orientation = 1
        pixelFormat = .rgba16Float
        self.colorSpace = colorSpace
    }

    /// Full-resolution size on screen after orientation (drives the aspect fit, so coarse
    /// levels are magnified rather than shown smaller).
    var displaySize: (width: Int, height: Int) { orientation >= 5 ? (height, width) : (width, height) }

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
