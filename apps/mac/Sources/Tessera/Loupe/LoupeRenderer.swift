import Metal
import QuartzCore
import simd

/// Where the image sits in the loupe: an affine map from drawable pixels (top-left origin) to
/// displayed-image uv (after EXIF orientation, [0, 1] across the whole picture). The normal view is
/// an aspect fit; the crop tool rotates the uncropped image about the crop so the crop box is
/// axis-aligned on screen and dims everything outside it.
struct LoupePlacement {
    /// uv = (row0 · (x, y, 1), row1 · (x, y, 1)) for drawable pixel (x, y).
    var row0: SIMD3<Double>
    var row1: SIMD3<Double>
    /// Drawable pixels per displayed-image uv unit along x (for the minification ratio).
    var pixelsPerImageWidth: Double
    /// Drawable-pixel rectangle kept bright; everything else is dimmed (crop tool).
    var keep: CGRect?

    /// Aspect fit with a margin, never upscaling past 1:1 of `display` texels.
    static func fit(display: (width: Int, height: Int), drawable: CGSize, margin: Double = 0.96) -> LoupePlacement {
        let fit = min(drawable.width * margin / Double(display.width), drawable.height * margin / Double(display.height), 1.0)
        let dw = Double(display.width) * fit, dh = Double(display.height) * fit
        let ox = (drawable.width - dw) / 2, oy = (drawable.height - dh) / 2
        return LoupePlacement(row0: SIMD3(1 / dw, 0, -ox / dw), row1: SIMD3(0, 1 / dh, -oy / dh),
                              pixelsPerImageWidth: dw, keep: nil)
    }

    /// Drawable pixel → displayed uv.
    func uv(_ x: Double, _ y: Double) -> (u: Double, v: Double) {
        (row0.x * x + row0.y * y + row0.z, row1.x * x + row1.y * y + row1.z)
    }
}

/// Minimal Metal presenter: maps one frame onto the drawable through a `LoupePlacement`, applying
/// the EXIF orientation and the frame's valid sub-rectangle. The shader is compiled from source at
/// startup so the SwiftPM build needs no metallib step. All develop math happens in the engine;
/// this only samples.
@MainActor
final class LoupeRenderer {
    let device: MTLDevice
    private let queue: MTLCommandQueue
    private let pipeline: MTLRenderPipelineState
    private let sampler: MTLSamplerState

    struct Uniforms {
        var row0: SIMD4<Float>
        var row1: SIMD4<Float>
        var uvScale: SIMD2<Float>
        /// Source texels per drawable pixel (> 1 when minifying).
        var ratio: Float
        var orientation: Int32
        /// Kept rectangle (x0, y0, x1, y1) in drawable pixels; x1 <= x0 disables dimming.
        var keep: SIMD4<Float>
        /// (dim factor, background, 0, 0).
        var misc: SIMD4<Float>
    }

    private static let source = """
    #include <metal_stdlib>
    using namespace metal;

    struct Uniforms { float4 row0; float4 row1; float2 uvScale; float ratio; int orientation; float4 keep; float4 misc; };
    struct VOut { float4 position [[position]]; };

    vertex VOut loupe_vertex(uint vid [[vertex_id]]) {
        float2 corners[4] = { float2(-1, -1), float2(1, -1), float2(-1, 1), float2(1, 1) };
        VOut o;
        o.position = float4(corners[vid], 0, 1);
        return o;
    }

    // Displayed uv -> stored uv for EXIF orientations 1-8.
    static float2 orient(float2 d, int o) {
        switch (o) {
            case 2: return float2(1 - d.x, d.y);
            case 3: return float2(1 - d.x, 1 - d.y);
            case 4: return float2(d.x, 1 - d.y);
            case 5: return float2(d.y, d.x);
            case 6: return float2(d.y, 1 - d.x);
            case 7: return float2(1 - d.y, 1 - d.x);
            case 8: return float2(1 - d.y, d.x);
            default: return d;
        }
    }

    // Input is linear (sRGB-decoding texture or linear half float) in the layer's colour space;
    // values above 1.0 in half-float frames are EDR headroom and pass through untouched.
    fragment half4 loupe_fragment(VOut in [[stage_in]], texture2d<half> tex [[texture(0)]],
                                  sampler s [[sampler(0)]], constant Uniforms &u [[buffer(0)]]) {
        float3 p = float3(in.position.xy, 1);
        float2 d = float2(dot(u.row0.xyz, p), dot(u.row1.xyz, p));
        half3 bg = half3(u.misc.y);
        if (any(d < 0.0) || any(d > 1.0)) { return half4(bg, 1.0h); }
        float2 size = float2(tex.get_width(), tex.get_height());
        float2 limit = u.uvScale - 0.5 / size;
        float2 st = orient(d, u.orientation) * u.uvScale;
        half3 c;
        if (u.ratio <= 1.0) {
            c = tex.sample(s, min(st, limit)).rgb;
        } else {
            // Minifying by up to 2x: four bilinear taps cover the pixel's footprint.
            float2 o = 0.25 * min(u.ratio, 2.0) / size;
            c = (tex.sample(s, min(max(st + float2(-o.x, -o.y), 0.0), limit)).rgb
               + tex.sample(s, min(max(st + float2( o.x, -o.y), 0.0), limit)).rgb
               + tex.sample(s, min(max(st + float2(-o.x,  o.y), 0.0), limit)).rgb
               + tex.sample(s, min(max(st + float2( o.x,  o.y), 0.0), limit)).rgb) * 0.25h;
        }
        if (u.keep.z > u.keep.x && (p.x < u.keep.x || p.y < u.keep.y || p.x > u.keep.z || p.y > u.keep.w)) {
            c = mix(bg, c, half(u.misc.x));
        }
        return half4(c, 1.0h);
    }
    """

    init?() {
        guard let device = MTLCreateSystemDefaultDevice(), let queue = device.makeCommandQueue() else { return nil }
        self.device = device
        self.queue = queue
        do {
            let library = try device.makeLibrary(source: Self.source, options: nil)
            let desc = MTLRenderPipelineDescriptor()
            desc.label = "loupe"
            desc.vertexFunction = library.makeFunction(name: "loupe_vertex")
            desc.fragmentFunction = library.makeFunction(name: "loupe_fragment")
            desc.colorAttachments[0].pixelFormat = .rgba16Float
            pipeline = try device.makeRenderPipelineState(descriptor: desc)
        } catch {
            NSLog("Loupe shader compile failed: \(error)")
            return nil
        }
        let sd = MTLSamplerDescriptor()
        sd.minFilter = .linear
        sd.magFilter = .linear
        sd.mipFilter = .notMipmapped
        sd.sAddressMode = .clampToEdge
        sd.tAddressMode = .clampToEdge
        guard let sampler = device.makeSamplerState(descriptor: sd) else { return nil }
        self.sampler = sampler
    }

    /// Encodes and presents one frame. `placement` nil = aspect fit. Returns the CPU encode time.
    @discardableResult
    func draw(in layer: CAMetalLayer, texture: MTLTexture?, frame: LoupeFrame?, placement: LoupePlacement?,
              background: Float, dim: Float = 0.35) -> Double {
        let t0 = CACurrentMediaTime()
        let size = layer.drawableSize
        guard size.width >= 1, size.height >= 1, let drawable = layer.nextDrawable(),
              let cmd = queue.makeCommandBuffer() else { return 0 }
        let pass = MTLRenderPassDescriptor()
        pass.colorAttachments[0].texture = drawable.texture
        pass.colorAttachments[0].loadAction = .clear
        pass.colorAttachments[0].storeAction = .store
        let bg = Double(background)
        pass.colorAttachments[0].clearColor = MTLClearColor(red: bg, green: bg, blue: bg, alpha: 1)
        guard let enc = cmd.makeRenderCommandEncoder(descriptor: pass) else { return 0 }
        if let texture, let frame {
            let place = placement ?? .fit(display: frame.displaySize, drawable: size)
            let contentOnScreen = Double(frame.orientation >= 5 ? frame.contentHeight : frame.contentWidth)
            let keep = place.keep ?? .zero
            var u = Uniforms(row0: SIMD4(Float(place.row0.x), Float(place.row0.y), Float(place.row0.z), 0),
                             row1: SIMD4(Float(place.row1.x), Float(place.row1.y), Float(place.row1.z), 0),
                             uvScale: SIMD2(Float(frame.contentWidth) / Float(frame.width),
                                            Float(frame.contentHeight) / Float(frame.height)),
                             ratio: Float(contentOnScreen / max(place.pixelsPerImageWidth, 1)),
                             orientation: Int32(frame.orientation),
                             keep: SIMD4(Float(keep.minX), Float(keep.minY), Float(keep.maxX), Float(keep.maxY)),
                             misc: SIMD4(dim, background, 0, 0))
            enc.setRenderPipelineState(pipeline)
            enc.setFragmentBytes(&u, length: MemoryLayout<Uniforms>.stride, index: 0)
            enc.setFragmentTexture(texture, index: 0)
            enc.setFragmentSamplerState(sampler, index: 0)
            enc.drawPrimitives(type: .triangleStrip, vertexStart: 0, vertexCount: 4)
        }
        enc.endEncoding()
        cmd.present(drawable)
        cmd.commit()
        return CACurrentMediaTime() - t0
    }
}
