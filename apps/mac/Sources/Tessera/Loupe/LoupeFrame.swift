import CoreGraphics
import CoreVideo
import IOSurface
import Metal

/// What the loupe presents. Today: a CGImage (embedded preview) rasterised into an IOSurface.
/// Later: the Rust engine renders into an IOSurface it shares with us (docs/11 §1.2, wgpu
/// `texture_from_raw`) and hands over only the handle, so pixels never cross UniFFI as bytes.
///
/// Both paths meet at `LoupeFrame.surface`, so the Metal presentation code is already the one
/// the engine will use.
struct LoupeFrame: @unchecked Sendable {
    let surface: IOSurfaceRef
    let width: Int
    let height: Int
    let pixelFormat: MTLPixelFormat
    /// Colour space the surface's pixel values are encoded in (linear-extended for our half-float frames).
    let colorSpace: CGColorSpace

    /// Engine entry point: wrap a surface produced elsewhere (e.g. by the Rust renderer).
    init(surface: IOSurfaceRef, pixelFormat: MTLPixelFormat, colorSpace: CGColorSpace) {
        self.surface = surface
        width = IOSurfaceGetWidth(surface)
        height = IOSurfaceGetHeight(surface)
        self.pixelFormat = pixelFormat
        self.colorSpace = colorSpace
    }

    /// Stub path: colour-convert `image` into `colorSpace` (the loupe's working space derived from the
    /// screen) as RGBA half-float in a new IOSurface. CoreGraphics performs the ICC conversion.
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
        return LoupeFrame(surface: surface, pixelFormat: .rgba16Float, colorSpace: colorSpace)
    }

    func makeTexture(device: MTLDevice) -> MTLTexture? {
        let desc = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: pixelFormat, width: width, height: height, mipmapped: false)
        desc.usage = .shaderRead
        desc.storageMode = device.hasUnifiedMemory ? .shared : .managed
        return device.makeTexture(descriptor: desc, iosurface: surface, plane: 0)
    }
}
