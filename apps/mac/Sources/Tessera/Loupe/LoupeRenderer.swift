import Metal
import QuartzCore
import simd

/// Minimal Metal presenter: aspect-fits one frame into the drawable, applying the EXIF orientation
/// and the frame's valid sub-rectangle. The shader is compiled from source at startup so the
/// SwiftPM build needs no metallib step. All develop math happens in the engine; this only samples.
@MainActor
final class LoupeRenderer {
    let device: MTLDevice
    private let queue: MTLCommandQueue
    private let pipeline: MTLRenderPipelineState
    private let sampler: MTLSamplerState

    struct Uniforms {
        var scale: SIMD2<Float>
        var uvScale: SIMD2<Float>
        /// Source texels per drawable pixel (> 1 when minifying).
        var ratio: Float
        var orientation: Int32
        var pad: SIMD2<Float> = .zero
    }

    private static let source = """
    #include <metal_stdlib>
    using namespace metal;

    struct Uniforms { float2 scale; float2 uvScale; float ratio; int orientation; float2 pad; };
    struct VOut { float4 position [[position]]; float2 uv; };

    vertex VOut loupe_vertex(uint vid [[vertex_id]], constant Uniforms &u [[buffer(0)]]) {
        float2 corners[4] = { float2(-1, -1), float2(1, -1), float2(-1, 1), float2(1, 1) };
        float2 p = corners[vid];
        VOut o;
        o.position = float4(p * u.scale, 0, 1);
        o.uv = float2((p.x + 1) * 0.5, 1 - (p.y + 1) * 0.5);
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
        float2 size = float2(tex.get_width(), tex.get_height());
        float2 limit = u.uvScale - 0.5 / size;
        float2 st = orient(in.uv, u.orientation) * u.uvScale;
        if (u.ratio <= 1.0) {
            return half4(tex.sample(s, min(st, limit)).rgb, 1.0h);
        }
        // Minifying by up to 2x: four bilinear taps cover the pixel's footprint.
        float2 o = 0.25 * min(u.ratio, 2.0) / size;
        half3 c = tex.sample(s, min(st + float2(-o.x, -o.y), limit)).rgb
                + tex.sample(s, min(st + float2( o.x, -o.y), limit)).rgb
                + tex.sample(s, min(st + float2(-o.x,  o.y), limit)).rgb
                + tex.sample(s, min(st + float2( o.x,  o.y), limit)).rgb;
        return half4(c * 0.25h, 1.0h);
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

    /// Encodes and presents one frame. Returns the CPU encode time in seconds.
    @discardableResult
    func draw(in layer: CAMetalLayer, texture: MTLTexture?, frame: LoupeFrame?, background: Float) -> Double {
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
            // Aspect-fit the full-resolution geometry with a margin, never upscaling past 1:1.
            let (dw, dh) = frame.displaySize
            let margin = 0.96
            let fit = min(size.width * margin / Double(dw), size.height * margin / Double(dh), 1.0)
            let drawnWidth = Double(dw) * fit
            let contentOnScreen = Double(frame.orientation >= 5 ? frame.contentHeight : frame.contentWidth)
            var u = Uniforms(scale: SIMD2(Float(drawnWidth / size.width), Float(Double(dh) * fit / size.height)),
                             uvScale: SIMD2(Float(frame.contentWidth) / Float(frame.width),
                                            Float(frame.contentHeight) / Float(frame.height)),
                             ratio: Float(contentOnScreen / max(drawnWidth, 1)),
                             orientation: Int32(frame.orientation))
            enc.setRenderPipelineState(pipeline)
            enc.setVertexBytes(&u, length: MemoryLayout<Uniforms>.stride, index: 0)
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
