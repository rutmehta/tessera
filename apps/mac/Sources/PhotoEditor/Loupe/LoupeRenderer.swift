import Metal
import QuartzCore
import simd

/// Minimal Metal presenter: aspect-fits one texture into the drawable and applies the stub
/// exposure uniform. The shader is compiled from source at startup so the SwiftPM build needs no
/// metallib step; the real pipeline (wgpu / MSL) replaces this with its output surface.
@MainActor
final class LoupeRenderer {
    let device: MTLDevice
    private let queue: MTLCommandQueue
    private let pipeline: MTLRenderPipelineState
    private let sampler: MTLSamplerState

    struct Uniforms {
        var scale: SIMD2<Float>
        var exposure: Float
        var pad: Float = 0
    }

    private static let source = """
    #include <metal_stdlib>
    using namespace metal;

    struct Uniforms { float2 scale; float exposure; float pad; };
    struct VOut { float4 position [[position]]; float2 uv; };

    vertex VOut loupe_vertex(uint vid [[vertex_id]], constant Uniforms &u [[buffer(0)]]) {
        float2 corners[4] = { float2(-1, -1), float2(1, -1), float2(-1, 1), float2(1, 1) };
        float2 p = corners[vid];
        VOut o;
        o.position = float4(p * u.scale, 0, 1);
        o.uv = float2((p.x + 1) * 0.5, 1 - (p.y + 1) * 0.5);
        return o;
    }

    // Input is linear-extended in the screen's primaries; exposure is a linear gain, so EDR
    // headroom above 1.0 is preserved all the way to the CAMetalLayer.
    fragment half4 loupe_fragment(VOut in [[stage_in]], texture2d<half> tex [[texture(0)]],
                                  sampler s [[sampler(0)]], constant Uniforms &u [[buffer(0)]]) {
        half4 c = tex.sample(s, in.uv);
        c.rgb *= half(exp2(u.exposure));
        return half4(c.rgb, 1.0h);
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
    func draw(in layer: CAMetalLayer, texture: MTLTexture?, exposure: Float, background: Float) -> Double {
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
        if let texture {
            // Aspect-fit with a margin, never upscaling past 1:1 device pixels.
            let margin = 0.96
            let fit = min(size.width * margin / Double(texture.width), size.height * margin / Double(texture.height), 1.0)
            var u = Uniforms(scale: SIMD2(Float(Double(texture.width) * fit / size.width),
                                          Float(Double(texture.height) * fit / size.height)),
                             exposure: exposure)
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
