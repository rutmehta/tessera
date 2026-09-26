import AppKit
import IOSurface
import Metal
import QuartzCore
import SwiftUI
import TesseraCore

/// Presents document composites: a `CAMetalLayer` in the EDR configuration of the loupe
/// (RGBA16F, extended linear, `wantsExtendedDynamicRangeContent`) sampling the backend's RGBA8
/// straight-alpha IOSurfaces over a checkerboard drawn here. Frames cover a canvas rectangle at a
/// pyramid level; the shader maps every drawable pixel to the canvas through the current zoom and
/// pan, so a frame keeps lining up while the next one renders.
@MainActor
final class DocumentRenderer {
    let device: MTLDevice
    private let queue: MTLCommandQueue
    private let pipeline: MTLRenderPipelineState
    private let linear: MTLSamplerState
    private let nearest: MTLSamplerState

    struct Uniforms {
        var viewSize: SIMD2<Float>
        var center: SIMD2<Float>
        var canvas: SIMD2<Float>
        var zoom: Float
        var checker: Float
        /// Canvas rectangle of the frame (x, y, w, h); w = 0 without a frame.
        var frameRect: SIMD4<Float>
        /// Valid fraction of the surface.
        var uvScale: SIMD2<Float>
        var nearest: Float
        var pad: Float
        var checkA: SIMD4<Float>
        var checkB: SIMD4<Float>
        var background: SIMD4<Float>
    }

    private static let source = """
    #include <metal_stdlib>
    using namespace metal;
    struct Uniforms { float2 viewSize; float2 center; float2 canvas; float zoom; float checker; float4 frameRect;
                      float2 uvScale; float nearest; float pad; float4 checkA; float4 checkB; float4 background; };
    struct VOut { float4 position [[position]]; };
    vertex VOut doc_vertex(uint vid [[vertex_id]]) {
        float2 corners[4] = { float2(-1, -1), float2(1, -1), float2(-1, 1), float2(1, 1) };
        VOut o; o.position = float4(corners[vid], 0, 1); return o;
    }
    fragment half4 doc_fragment(VOut in [[stage_in]], texture2d<half> tex [[texture(0)]],
                                sampler lin [[sampler(0)]], sampler near [[sampler(1)]],
                                constant Uniforms &u [[buffer(0)]]) {
        float2 p = in.position.xy;
        float2 c = u.center + (p - u.viewSize * 0.5) / u.zoom;
        if (any(c < 0.0) || any(c >= u.canvas)) { return half4(half3(u.background.rgb), 1.0h); }
        int2 k = int2(floor(p / u.checker));
        half3 col = ((k.x + k.y) & 1) ? half3(u.checkA.rgb) : half3(u.checkB.rgb);
        float4 r = u.frameRect;
        if (r.z > 0.0 && c.x >= r.x && c.y >= r.y && c.x < r.x + r.z && c.y < r.y + r.w) {
            float2 size = float2(tex.get_width(), tex.get_height());
            float2 uv = min((c - r.xy) / r.zw * u.uvScale, u.uvScale - 0.5 / size);
            // Straight alpha, sRGB-decoded by the texture format: composite over the checkerboard.
            half4 s = u.nearest > 0.5 ? tex.sample(near, uv) : tex.sample(lin, uv);
            col = mix(col, s.rgb, s.a);
        }
        return half4(col, 1.0h);
    }
    """

    init?() {
        guard let device = MTLCreateSystemDefaultDevice(), let queue = device.makeCommandQueue() else { return nil }
        self.device = device
        self.queue = queue
        do {
            let library = try device.makeLibrary(source: Self.source, options: nil)
            let desc = MTLRenderPipelineDescriptor()
            desc.label = "document"
            desc.vertexFunction = library.makeFunction(name: "doc_vertex")
            desc.fragmentFunction = library.makeFunction(name: "doc_fragment")
            desc.colorAttachments[0].pixelFormat = .rgba16Float
            pipeline = try device.makeRenderPipelineState(descriptor: desc)
        } catch {
            NSLog("Document shader compile failed: \(error)")
            return nil
        }
        func sampler(_ f: MTLSamplerMinMagFilter) -> MTLSamplerState? {
            let d = MTLSamplerDescriptor()
            d.minFilter = f
            d.magFilter = f
            d.sAddressMode = .clampToEdge
            d.tAddressMode = .clampToEdge
            return device.makeSamplerState(descriptor: d)
        }
        guard let l = sampler(.linear), let n = sampler(.nearest) else { return nil }
        linear = l
        nearest = n
    }

    func texture(for surface: IOSurfaceRef) -> MTLTexture? {
        let desc = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .rgba8Unorm_srgb, width: IOSurfaceGetWidth(surface),
                                                            height: IOSurfaceGetHeight(surface), mipmapped: false)
        desc.usage = .shaderRead
        desc.storageMode = device.hasUnifiedMemory ? .shared : .managed
        return device.makeTexture(descriptor: desc, iosurface: surface, plane: 0)
    }

    func draw(in layer: CAMetalLayer, texture: MTLTexture?, uniforms: Uniforms) {
        let size = layer.drawableSize
        guard size.width >= 1, size.height >= 1, let drawable = layer.nextDrawable(),
              let cmd = queue.makeCommandBuffer() else { return }
        let pass = MTLRenderPassDescriptor()
        pass.colorAttachments[0].texture = drawable.texture
        pass.colorAttachments[0].loadAction = .dontCare
        pass.colorAttachments[0].storeAction = .store
        guard let enc = cmd.makeRenderCommandEncoder(descriptor: pass), let tex = texture ?? placeholder else { return }
        var u = uniforms
        enc.setRenderPipelineState(pipeline)
        enc.setFragmentBytes(&u, length: MemoryLayout<Uniforms>.stride, index: 0)
        enc.setFragmentTexture(tex, index: 0)
        enc.setFragmentSamplerState(linear, index: 0)
        enc.setFragmentSamplerState(nearest, index: 1)
        enc.drawPrimitives(type: .triangleStrip, vertexStart: 0, vertexCount: 4)
        enc.endEncoding()
        cmd.present(drawable)
        cmd.commit()
    }

    private lazy var placeholder: MTLTexture? = {
        let d = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .rgba8Unorm_srgb, width: 1, height: 1, mipmapped: false)
        d.usage = .shaderRead
        return device.makeTexture(descriptor: d)
    }()
}

/// `DocumentViewport`: the document canvas. Zoom (⌘+ / ⌘− / ⌘0 / ⌘1, pinch, ⌥-drag scrubby zoom,
/// ⌥-scroll), pan (scroll, Space-drag), the rectangular marquee with marching ants, and the
/// surface ring shared with the backend. Viewport changes call `setViewport(level, rect, zoom)`.
@MainActor
final class DocumentViewportView: NSView {
    private let renderer = DocumentRenderer()
    private var metalLayer: CAMetalLayer? { layer as? CAMetalLayer }
    weak var workspace: DocumentWorkspace?
    private(set) weak var controller: DocumentController?
    private var ring: [UInt32: (surface: IOSurfaceRef, texture: MTLTexture?)] = [:]
    private var ringSize = (width: UInt32(0), height: UInt32(0))
    private var current: (info: DocFrame, texture: MTLTexture)?
    private var math = DocumentViewportMath(canvasWidth: 1, canvasHeight: 1, viewWidth: 1, viewHeight: 1)
    private let ants = MarchingAntsView()
    private var drag: Drag?
    private var lastPushed: (UInt8, UInt32, UInt32, UInt32, UInt32, Double)?

    private enum Drag {
        case pan(last: CGPoint)
        case scrubby(start: CGPoint, zoom: Double)
        case marquee(start: CGPoint)
        /// A layered-editor tool gesture (WP M5-11, `DocumentTools`).
        case tool
    }

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layerContentsRedrawPolicy = .duringViewResize
        setAccessibilityElement(true)
        setAccessibilityRole(.image)
        setAccessibilityLabel("Document canvas")
        setAccessibilityIdentifier("document.viewport")
        ants.frame = bounds
        ants.autoresizingMask = [.width, .height]
        addSubview(ants)
        // WP M5-11: marching ants from the selection outline, brush cursor, guides, transform box.
        toolOverlay.frame = bounds
        toolOverlay.autoresizingMask = [.width, .height]
        toolOverlay.viewport = self
        addSubview(toolOverlay)
    }

    required init?(coder: NSCoder) { fatalError() }

    override func makeBackingLayer() -> CALayer {
        let l = CAMetalLayer()
        l.device = renderer?.device
        l.pixelFormat = .rgba16Float
        l.wantsExtendedDynamicRangeContent = true
        l.framebufferOnly = true
        l.isOpaque = true
        l.maximumDrawableCount = 3
        l.colorspace = CGColorSpace(name: CGColorSpace.extendedLinearSRGB)
        return l
    }

    override var wantsUpdateLayer: Bool { true }
    override func updateLayer() { render() }
    override var isOpaque: Bool { true }
    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    private var scale: CGFloat { window?.backingScaleFactor ?? NSScreen.main?.backingScaleFactor ?? 2 }

    // MARK: Document

    func attach(_ doc: DocumentController?) {
        guard doc !== controller else { return }
        if let old = controller {
            old.viewState = math
            old.onFrame = nil
            if old.viewport === self { old.viewport = nil }
            old.backend.detachSurfaces()
        }
        controller = doc
        ring.removeAll()
        ringSize = (0, 0)
        current = nil
        lastPushed = nil
        guard let doc else { ants.rect = nil; render(); return }
        doc.viewport = self
        doc.onFrame = { [weak self, weak doc] f in
            guard let self, let doc, doc === self.controller else { return }
            self.present(f)
        }
        let size = drawableSize
        math = doc.viewState ?? DocumentViewportMath(canvasWidth: Double(doc.info.width), canvasHeight: Double(doc.info.height),
                                                     viewWidth: size.width, viewHeight: size.height)
        math.resize(viewWidth: size.width, viewHeight: size.height)
        doc.zoomDidChange(math.zoom)
        selectionDidChange()
        pushViewport()
        render()
    }

    private var drawableSize: CGSize { CGSize(width: max(bounds.width * scale, 1), height: max(bounds.height * scale, 1)) }

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        updateDrawable()
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        updateDrawable()
    }

    override func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        updateDrawable()
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        render()
    }

    private func updateDrawable() {
        guard let metalLayer else { return }
        metalLayer.contentsScale = scale
        let size = drawableSize
        if metalLayer.drawableSize != size { metalLayer.drawableSize = size }
        let first = math.viewWidth <= 1
        math.resize(viewWidth: size.width, viewHeight: size.height)
        if first, controller != nil, controller?.viewState == nil { math.fit() }
        controller?.zoomDidChange(math.zoom)
        selectionDidChange()
        pushViewport()
        render()
    }

    /// Sends the visible rectangle to the backend, (re)allocating the surface ring when the frame
    /// it will render no longer fits.
    private func pushViewport() {
        guard let doc = controller, window != nil, bounds.width > 1 else { return }
        let rect = math.visibleCanvasRect
        guard !rect.isEmpty else { return }
        let level = math.level
        let region = DocumentViewportMath.levelRect(rect, level: level)
        // Zoomed viewports allocate surfaces of their own size (the region), rounded to limit churn.
        if ring.isEmpty || ringSize.width < region.width || ringSize.height < region.height
            || UInt64(ringSize.width) * UInt64(ringSize.height) > 4 * UInt64(region.width) * UInt64(region.height) {
            let round = { (v: UInt32) in UInt32((Int(v) + 127) / 128 * 128) }
            let (w, h) = (round(region.width), round(region.height))
            var next: [UInt32: (IOSurfaceRef, MTLTexture?)] = [:]
            for _ in 0..<3 {
                guard let s = DocumentSurfaces.make(width: Int(w), height: Int(h)) else { continue }
                next[IOSurfaceGetID(s)] = (s, renderer?.texture(for: s))
            }
            // Release the old ring first (M5-09: a different size replaces the ring; detaching makes it
            // explicit). The frame on screen keeps its texture until the new ring's first frame arrives.
            if !ring.isEmpty { doc.backend.detachSurfaces() }
            do {
                for id in next.keys { try doc.backend.attachSurface(iosurfaceId: id, width: w, height: h) }
            } catch {
                doc.report?("Viewport: \(error.localizedDescription)")
            }
            ring = next.mapValues { (surface: $0.0, texture: $0.1) }
            ringSize = (w, h)
            lastPushed = nil
        }
        let pushed = (UInt8(level), region.x, region.y, region.width, region.height, math.zoom)
        if let l = lastPushed, l.0 == pushed.0, l.1 == pushed.1, l.2 == pushed.2, l.3 == pushed.3, l.4 == pushed.4, l.5 == pushed.5 {
            return
        }
        lastPushed = pushed
        do {
            try doc.backend.setViewport(level: UInt8(level), x: region.x, y: region.y, width: region.width, height: region.height,
                                        zoom: math.zoom)
        } catch {
            doc.report?("Viewport: \(error.localizedDescription)")
        }
    }

    private func present(_ f: DocFrame) {
        guard let entry = ring[f.surfaceId], let texture = entry.texture else { return }
        current = (f, texture)
        render()
    }

    func render() {
        guard let renderer, let metalLayer, window != nil else { return }
        let appearance = effectiveAppearance
        func linear(_ c: NSColor) -> SIMD4<Float> {
            var out = SIMD4<Float>(0, 0, 0, 1)
            appearance.performAsCurrentDrawingAppearance {
                let s = c.usingColorSpace(.sRGB) ?? c
                let lin = { (v: CGFloat) -> Float in
                    let x = Float(v)
                    return x <= 0.04045 ? x / 12.92 : powf((x + 0.055) / 1.055, 2.4)
                }
                out = SIMD4(lin(s.redComponent), lin(s.greenComponent), lin(s.blueComponent), 1)
            }
            return out
        }
        var frameRect = SIMD4<Float>(0, 0, 0, 0)
        var uvScale = SIMD2<Float>(1, 1)
        if let (info, tex) = current {
            let r = info.canvasRect
            frameRect = SIMD4(Float(r.x), Float(r.y), Float(r.width), Float(r.height))
            uvScale = SIMD2(Float(info.width) / Float(tex.width), Float(info.height) / Float(tex.height))
        }
        let size = metalLayer.drawableSize
        let u = DocumentRenderer.Uniforms(
            viewSize: SIMD2(Float(size.width), Float(size.height)),
            center: SIMD2(Float(math.center.x), Float(math.center.y)),
            canvas: SIMD2(Float(math.canvasWidth), Float(math.canvasHeight)),
            zoom: Float(math.zoom), checker: Float(Theme.Space.s * scale), frameRect: frameRect, uvScale: uvScale,
            nearest: math.zoom >= 2 ? 1 : 0, pad: 0,
            checkA: linear(Theme.Palette.checkerLight), checkB: linear(Theme.Palette.checkerDark),
            background: linear(Theme.Palette.canvas))
        renderer.draw(in: metalLayer, texture: controller == nil ? nil : current?.texture, uniforms: u)
    }

    // MARK: Zoom commands

    private func changed(anchor: CGPoint? = nil) {
        toolOverlay.needsDisplay = true
        controller?.zoomDidChange(math.zoom)
        controller?.viewState = math
        selectionDidChange()
        pushViewport()
        render()
    }

    func zoomIn(at anchor: CGPoint? = nil) { math.setZoom(DocumentViewportMath.zoomIn(from: math.zoom), anchor: anchor); changed() }
    func zoomOut(at anchor: CGPoint? = nil) { math.setZoom(DocumentViewportMath.zoomOut(from: math.zoom), anchor: anchor); changed() }
    func zoomToFit() { math.fit(); changed() }
    func zoomActual() { math.setZoom(1); changed() }
    var zoom: Double { math.zoom }

    // MARK: Events

    /// View point (points, y down) → drawable pixel.
    private func device(_ event: NSEvent) -> CGPoint {
        let p = convert(event.locationInWindow, from: nil)
        return CGPoint(x: p.x * scale, y: p.y * scale)
    }

    override func scrollWheel(with event: NSEvent) {
        guard controller != nil else { return }
        let mods = event.modifierFlags
        if mods.contains(.option) || mods.contains(.command) {
            let dy = event.hasPreciseScrollingDeltas ? event.scrollingDeltaY : event.scrollingDeltaY * 8
            math.setZoom(math.zoom * exp(Double(dy) * 0.01), anchor: device(event))
        } else {
            let k: CGFloat = event.hasPreciseScrollingDeltas ? scale : scale * 12
            math.pan(dx: Double(event.scrollingDeltaX * k), dy: Double(event.scrollingDeltaY * k))
        }
        changed()
    }

    override func magnify(with event: NSEvent) {
        guard controller != nil else { return }
        math.setZoom(math.zoom * (1 + Double(event.magnification)), anchor: device(event))
        changed()
    }

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        guard let doc = controller else { return }
        let p = device(event)
        if workspace?.spaceHeld == true {
            drag = .pan(last: p)
            NSCursor.closedHand.set()
        } else if DocumentTools.shared.mouseDown(event, in: self) {   // WP M5-11
            drag = .tool
        } else if event.modifierFlags.contains(.option) {
            drag = .scrubby(start: p, zoom: math.zoom)
        } else if doc.tool == .marquee {
            drag = .marquee(start: p)
        } else {
            drag = nil
            doc.report?("Move tool: moving layer pixels arrives with transforms. Space-drag pans, ⌥-drag zooms, M selects.")
        }
    }

    override func mouseDragged(with event: NSEvent) {
        let p = device(event)
        switch drag {
        case .pan(let last):
            math.pan(dx: Double(p.x - last.x), dy: Double(p.y - last.y))
            drag = .pan(last: p)
            changed()
        case .scrubby(let start, let z):
            math.setZoom(z * exp(Double(p.x - start.x) / Double(scale) * 0.01), anchor: start)
            changed()
        case .marquee(let start):
            ants.rect = marqueeRect(start, p).map(viewRect)
        case .tool:
            DocumentTools.shared.mouseDragged(event, in: self)
        case nil: break
        }
    }

    override func mouseUp(with event: NSEvent) {
        defer { drag = nil; window?.invalidateCursorRects(for: self) }
        if case .tool = drag { DocumentTools.shared.mouseUp(event, in: self); return }
        guard case .marquee(let start) = drag, let doc = controller else { return }
        doc.setMarquee(marqueeRect(start, device(event)))
    }

    /// Canvas rectangle between two drawable points, clamped to the canvas; nil for a click.
    private func marqueeRect(_ a: CGPoint, _ b: CGPoint) -> CanvasRect? {
        let ca = math.canvasPoint(view: a), cb = math.canvasPoint(view: b)
        let x0 = max(min(ca.x, cb.x).rounded(), 0), y0 = max(min(ca.y, cb.y).rounded(), 0)
        let x1 = min(max(ca.x, cb.x).rounded(), math.canvasWidth), y1 = min(max(ca.y, cb.y).rounded(), math.canvasHeight)
        guard x1 - x0 >= 1, y1 - y0 >= 1, hypot(a.x - b.x, a.y - b.y) > 2 * scale else { return nil }
        return CanvasRect(x: Int64(x0), y: Int64(y0), width: Int64(x1 - x0), height: Int64(y1 - y0))
    }

    /// Canvas rectangle → view rectangle in points.
    private func viewRect(_ r: CanvasRect) -> CGRect {
        let a = math.viewPoint(canvas: CGPoint(x: Double(r.x), y: Double(r.y)))
        let b = math.viewPoint(canvas: CGPoint(x: Double(r.x) + Double(r.width), y: Double(r.y) + Double(r.height)))
        return CGRect(x: a.x / scale, y: a.y / scale, width: (b.x - a.x) / scale, height: (b.y - a.y) / scale)
    }

    func selectionDidChange() {
        // WP M5-11: the marching ants follow the engine's selection outline (ToolOverlayView).
        ants.rect = nil
        DocumentTools.shared.selectionDidChange(in: self)
        toolOverlay.needsDisplay = true
    }

    override func resetCursorRects() {
        let cursor: NSCursor = workspace?.spaceHeld == true ? .openHand : DocumentTools.shared.cursor(for: controller)
        addCursorRect(bounds, cursor: cursor)
    }

    // MARK: Tools (WP M5-11)

    let toolOverlay = ToolOverlayView()

    /// Level-0 canvas pixel under an event.
    func canvasPoint(_ event: NSEvent) -> CGPoint { math.canvasPoint(view: device(event)) }
    /// A canvas point in view points (y down).
    func viewPoint(canvas p: CGPoint) -> CGPoint {
        let d = math.viewPoint(canvas: p)
        return CGPoint(x: d.x / scale, y: d.y / scale)
    }
    /// View points per canvas pixel.
    var pointsPerPixel: Double { math.zoom / Double(scale) }
    /// The pyramid level the viewport shows.
    var viewLevel: Int { math.level }
    /// Pans by view points (Hand tool).
    func panBy(dx: CGFloat, dy: CGFloat) {
        math.pan(dx: Double(dx * scale), dy: Double(dy * scale))
        changed()
    }
    /// Zoom tool: in or out about an event's location.
    func zoomStep(in zoomIn: Bool, at event: NSEvent) {
        let a = device(event)
        zoomIn ? self.zoomIn(at: a) : zoomOut(at: a)
    }

    private var toolTracking: NSTrackingArea?
    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let t = toolTracking { removeTrackingArea(t) }
        let t = NSTrackingArea(rect: bounds, options: [.mouseMoved, .mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect],
                               owner: self, userInfo: nil)
        addTrackingArea(t)
        toolTracking = t
    }
    override func mouseMoved(with event: NSEvent) { DocumentTools.shared.mouseMoved(event, in: self) }
    override func mouseExited(with event: NSEvent) { DocumentTools.shared.mouseExited(in: self) }
    override func rightMouseDown(with event: NSEvent) {
        if !DocumentTools.shared.rightMouseDown(event, in: self) { super.rightMouseDown(with: event) }
    }
    override func rightMouseDragged(with event: NSEvent) { DocumentTools.shared.mouseDragged(event, in: self) }
    override func rightMouseUp(with event: NSEvent) { DocumentTools.shared.mouseUp(event, in: self) }

    func cursorDidChange() { window?.invalidateCursorRects(for: self) }
}

/// The marquee's marching ants: a two-tone dashed rectangle whose phase advances at 30 fps.
@MainActor
final class MarchingAntsView: NSView {
    var rect: CGRect? {
        didSet {
            guard rect != oldValue else { return }
            needsDisplay = true
            if rect != nil, timer == nil {
                timer = Timer.scheduledTimer(withTimeInterval: 1.0 / 30, repeats: true) { [weak self] _ in
                    MainActor.assumeIsolated {
                        guard let self else { return }
                        self.phase = (self.phase + 0.5).truncatingRemainder(dividingBy: 8)
                        self.needsDisplay = true
                    }
                }
            } else if rect == nil {
                timer?.invalidate()
                timer = nil
            }
        }
    }
    private var phase: CGFloat = 0
    private var timer: Timer?

    override var isFlipped: Bool { true }
    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    override func draw(_ dirtyRect: NSRect) {
        guard let r = rect?.integral.insetBy(dx: 0.5, dy: 0.5) else { return }
        let path = NSBezierPath(rect: r)
        path.lineWidth = Theme.Space.hairline
        Theme.Palette.OnImage.text.setStroke()
        path.stroke()
        path.setLineDash([4, 4], count: 2, phase: phase)
        Theme.Palette.OnImage.ink.setStroke()
        path.stroke()
    }
}

/// SwiftUI host of the viewport, following the workspace's current document.
struct DocumentViewportRepresentable: NSViewRepresentable {
    let workspace: DocumentWorkspace
    let document: DocumentController?

    func makeNSView(context: Context) -> DocumentViewportView {
        let v = DocumentViewportView(frame: .zero)
        v.workspace = workspace
        return v
    }

    func updateNSView(_ v: DocumentViewportView, context: Context) {
        v.attach(document)
        _ = document?.tool
        v.cursorDidChange()
    }
}
