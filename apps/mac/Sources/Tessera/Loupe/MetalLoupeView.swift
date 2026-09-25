import AppKit
import IOSurface
import Metal
import TesseraFFI
import QuartzCore
import TesseraCore

/// Loupe viewport: an NSView backed by a CAMetalLayer configured for EDR (RGBA16F,
/// `wantsExtendedDynamicRangeContent`) whose colour space follows the window's screen.
/// Swift owns presentation; the engine only supplies IOSurfaces (see `LoupeFrame`). While a
/// develop session is attached, a display link sends coalesced slider changes once per frame.
@MainActor
final class MetalLoupeView: NSView {
    private let renderer = LoupeRenderer()
    private var metalLayer: CAMetalLayer? { layer as? CAMetalLayer }

    /// Source kept so the frame can be re-rasterised when the screen (and so the working space) changes.
    private var sourceImage: CGImage?
    private var sourceIsFinal = false
    private var currentFrame: LoupeFrame?
    private var texture: MTLTexture?
    private var rasterGeneration = 0
    private(set) var workingColorSpace: CGColorSpace = CGColorSpace(name: CGColorSpace.extendedLinearDisplayP3)!
    private var observedWindow: NSWindow?
    private var notificationTokens: [NSObjectProtocol] = []

    /// The engine session for the image on screen; its frames replace the preview.
    private(set) weak var develop: DevelopController?
    private var flushLink: CADisplayLink?

    /// Called with a human-readable description of the colour setup whenever the screen changes.
    var onColorInfoChange: ((String) -> Void)?
    /// Last CPU encode time, for the inspector's latency readout.
    private(set) var lastEncodeTime: Double = 0
    /// Crop tool, targeted adjustment and detail-target interaction on top of the image.
    let toolOverlay = LoupeToolOverlay(frame: .zero)
    /// Crop-tool presentation (image rotated about the crop, box axis-aligned), in view points.
    var cropView: CropView? { didSet { render() } }
    private var lastDisplayShape: (Int, Int)?
    /// The selected mask's overlay plane (masking mode), shown over engine frames.
    private var maskOverlay: LoupeRenderer.MaskOverlay?

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layerContentsRedrawPolicy = .duringViewResize
        setAccessibilityElement(true)
        setAccessibilityRole(.image)
        setAccessibilityLabel("Loupe")
        toolOverlay.frame = bounds
        toolOverlay.autoresizingMask = [.width, .height]
        toolOverlay.loupe = self
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
        l.colorspace = workingColorSpace
        l.backgroundColor = CGColor(gray: 0.1, alpha: 1)
        return l
    }

    override var wantsUpdateLayer: Bool { true }
    override func updateLayer() { render() }
    override var isOpaque: Bool { true }

    // MARK: Geometry / screen tracking

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        updateDrawableSize()
        planSurfaces()
        render()
    }

    override func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        updateDrawableSize()
        updateColorSpace()
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        guard observedWindow !== window else { return }
        notificationTokens.forEach(NotificationCenter.default.removeObserver)
        notificationTokens.removeAll()
        observedWindow = window
        guard let window else { return }
        let nc = NotificationCenter.default
        let handler: @Sendable (Notification) -> Void = { [weak self] _ in
            MainActor.assumeIsolated { self?.screenDidChange() }
        }
        for name in [NSWindow.didChangeScreenNotification, NSWindow.didChangeScreenProfileNotification,
                     NSWindow.didChangeBackingPropertiesNotification] {
            notificationTokens.append(nc.addObserver(forName: name, object: window, queue: .main, using: handler))
        }
        notificationTokens.append(nc.addObserver(forName: NSApplication.didChangeScreenParametersNotification,
                                                 object: nil, queue: .main, using: handler))
        screenDidChange()
    }

    private func screenDidChange() {
        updateDrawableSize()
        updateColorSpace()
        planSurfaces()
        render()
    }

    private func updateDrawableSize() {
        guard let metalLayer else { return }
        let scale = window?.backingScaleFactor ?? NSScreen.main?.backingScaleFactor ?? 2
        metalLayer.contentsScale = scale
        let size = CGSize(width: max(bounds.width * scale, 1), height: max(bounds.height * scale, 1))
        if metalLayer.drawableSize != size { metalLayer.drawableSize = size }
    }

    /// Working space = the screen's colour space, linearised and extended (values > 1 are EDR).
    private func updateColorSpace() {
        guard metalLayer != nil else { return }
        let screen = window?.screen ?? NSScreen.main
        let screenSpace = screen?.colorSpace?.cgColorSpace ?? CGColorSpace(name: CGColorSpace.displayP3)!
        let working = CGColorSpaceCreateExtendedLinearized(screenSpace)
            ?? CGColorSpace(name: CGColorSpace.extendedLinearDisplayP3)!
        let changed = working != workingColorSpace
        workingColorSpace = working

        let name = screen?.colorSpace?.localizedName ?? (screenSpace.name as String? ?? "Unknown")
        let potential = screen?.maximumPotentialExtendedDynamicRangeColorComponentValue ?? 1
        let current = screen?.maximumExtendedDynamicRangeColorComponentValue ?? 1
        onColorInfoChange?(String(format: "%@ · linear extended · RGBA16F · EDR headroom %.1f× (max %.1f×)", name, current, potential))

        if changed, let img = sourceImage { rasterize(img, isFinal: sourceIsFinal) }
    }

    // MARK: Content

    /// Stub path: present a CGImage (embedded preview). Rasterised off the main thread into an IOSurface.
    func present(image: CGImage?, isFinal: Bool) {
        sourceImage = image
        sourceIsFinal = isFinal
        guard let image else {
            rasterGeneration += 1
            currentFrame = nil
            texture = nil
            render()
            return
        }
        rasterize(image, isFinal: isFinal)
    }

    /// Engine path: present a surface rendered elsewhere, in the frame's colour space.
    func present(frame: LoupeFrame) {
        rasterGeneration += 1
        sourceImage = nil
        currentFrame = frame
        texture = renderer.flatMap { frame.makeTexture(device: $0.device) }
        render()
    }

    // MARK: Develop session

    /// Attaches (or detaches, with nil) the session for the image on screen. Its frames are
    /// presented as they arrive; the preview stays up until the first one.
    func attach(develop controller: DevelopController?) {
        guard controller !== develop else { return }
        develop?.onNeedsFlush = nil
        develop = controller
        maskOverlay = nil
        guard let controller else {
            flushLink?.isPaused = true
            return
        }
        if flushLink == nil {
            let link = displayLink(target: self, selector: #selector(flushTick(_:)))
            link.add(to: .main, forMode: .common)
            flushLink = link
        }
        flushLink?.isPaused = true
        controller.onNeedsFlush = { [weak self] in self?.flushLink?.isPaused = false }
        planSurfaces()
    }

    /// Shows an engine frame if it belongs to the attached session.
    func present(developFrame f: DevelopFrame, from controller: DevelopController) {
        guard controller === develop, let surface = controller.surface(f.surfaceID) else { return }
        present(frame: LoupeFrame(engineSurface: surface, contentWidth: f.width, contentHeight: f.height,
                                  fullWidth: f.displayWidth, fullHeight: f.displayHeight,
                                  orientation: Int(controller.info.orientation)))
        toolOverlay.frameDidArrive()
        // A crop (or its undo) changes the picture's aspect: re-plan the surface level.
        let shape = (f.displayWidth, f.displayHeight)
        if !f.isOverlay, let last = lastDisplayShape, last != shape { planSurfaces() }
        if !f.isOverlay { lastDisplayShape = shape }
    }

    @objc private func flushTick(_ link: CADisplayLink) {
        if develop?.flushPending() != true { link.isPaused = true }
    }

    /// Surface size follows the drawn image size in device pixels (the session picks the level).
    private func planSurfaces() {
        guard let develop, let metalLayer, window != nil else { return }
        let size = metalLayer.drawableSize
        let info = develop.info
        let swap = info.orientation >= 5
        let (iw, ih) = (Double(swap ? info.height : info.width), Double(swap ? info.width : info.height))
        // A crop is magnified to fill the view: plan the level for the cropped fraction.
        let (fw, fh) = cropFraction(develop)
        let fit = min(size.width * 0.96 / (iw * fw), size.height * 0.96 / (ih * fh), 1.0)
        do {
            try develop.attachSurfaces(viewWidth: Int((iw * fit).rounded(.up)), viewHeight: Int((ih * fit).rounded(.up)))
        } catch {
            develop.onFailure?(error.localizedDescription)
        }
    }

    private func rasterize(_ image: CGImage, isFinal: Bool) {
        rasterGeneration += 1
        let generation = rasterGeneration
        let space = workingColorSpace
        let work = { @Sendable in LoupeFrame.rasterize(image, into: space) }
        let apply: @MainActor (LoupeFrame?) -> Void = { [weak self] frame in
            guard let self, generation == self.rasterGeneration, let frame else { return }
            self.currentFrame = frame
            self.texture = self.renderer.flatMap { frame.makeTexture(device: $0.device) }
            self.render()
        }
        if !isFinal, image.width * image.height <= 512 * 512 {
            apply(work())   // small thumbnail: synchronous first paint
        } else {
            DispatchQueue.global(qos: .userInteractive).async {
                let frame = work()
                DispatchQueue.main.async { MainActor.assumeIsolated { apply(frame) } }
            }
        }
    }

    func render() {
        guard let renderer, let metalLayer, window != nil else { return }
        let space = currentFrame?.colorSpace ?? workingColorSpace
        if metalLayer.colorspace != space { metalLayer.colorspace = space }
        let scale = metalLayer.contentsScale
        let placement = cropView.map { $0.placement(scale: scale) }
        var overlay = cropView == nil && currentFrame?.pixelFormat == .rgba8Unorm_srgb ? maskOverlay : nil
        if overlay != nil {
            let tools = MaskTools.shared
            let c = tools.overlayColor.rgb
            // Engine frames are sampled as linear sRGB: linearise the tint.
            let lin = { (v: Float) in v <= 0.04045 ? v / 12.92 : powf((v + 0.055) / 1.055, 2.4) }
            overlay?.tint = SIMD4(lin(c.r), lin(c.g), lin(c.b), Float(tools.overlayOpacity))
        }
        lastEncodeTime = renderer.draw(in: metalLayer, texture: texture, frame: currentFrame, placement: placement,
                                       background: Theme.loupeBackgroundLinear, overlay: overlay)
    }

    /// Shows (or clears) the selected mask's overlay plane from the attached session.
    func present(maskOverlay f: MaskOverlayFrame?) {
        guard let f, let develop, let surface = develop.maskOverlaySurface(f.surfaceId), let renderer else {
            if maskOverlay != nil { maskOverlay = nil; render() }
            return
        }
        let (w, h) = (IOSurfaceGetWidth(surface), IOSurfaceGetHeight(surface))
        let desc = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .r8Unorm, width: w, height: h, mipmapped: false)
        desc.usage = .shaderRead
        desc.storageMode = renderer.device.hasUnifiedMemory ? .shared : .managed
        guard let texture = renderer.device.makeTexture(descriptor: desc, iosurface: surface, plane: 0) else { return }
        maskOverlay = LoupeRenderer.MaskOverlay(texture: texture,
                                                scale: SIMD2(Float(f.width) / Float(w), Float(f.height) / Float(h)),
                                                tint: .zero)
        render()
    }

    /// Displayed-picture uv under a view point (y-down points), unclamped; nil without a frame or
    /// in the crop tool.
    func displayUV(_ p: CGPoint) -> (u: Double, v: Double)? {
        guard cropView == nil, let frame = currentFrame, let metalLayer else { return nil }
        let scale = metalLayer.contentsScale
        let place = LoupePlacement.fit(display: frame.displaySize, drawable: metalLayer.drawableSize)
        return place.uv(p.x * scale, p.y * scale)
    }

    /// View point (y-down points) of displayed-picture uv; the inverse of `displayUV`.
    func viewPoint(u: Double, v: Double) -> CGPoint? {
        guard cropView == nil, let frame = currentFrame, let metalLayer else { return nil }
        let scale = metalLayer.contentsScale
        let place = LoupePlacement.fit(display: frame.displaySize, drawable: metalLayer.drawableSize)
        let x = (u - place.row0.z) / place.row0.x, y = (v - place.row1.z) / place.row1.y
        return CGPoint(x: x / scale, y: y / scale)
    }

    /// Width of the displayed picture in view points.
    var pictureWidthPoints: Double? {
        guard let a = viewPoint(u: 0, v: 0), let b = viewPoint(u: 1, v: 0) else { return nil }
        return Double(b.x - a.x)
    }

    // MARK: Tool geometry

    /// Displayed (cropped) picture fraction of the full frame, per axis, for surface planning.
    private func cropFraction(_ d: DevelopController) -> (Double, Double) {
        if DevelopTools.shared.cropActive { return (1, 1) }
        guard let r = d.value(at: CropControls.rectPath) as? [String: Any] else { return (1, 1) }
        let n = { (k: String, def: Double) in (r[k] as? NSNumber)?.doubleValue ?? def }
        var (w, h) = (n("right", 1) - n("left", 0), n("bottom", 1) - n("top", 0))
        if d.info.orientation >= 5 { swap(&w, &h) }
        return (min(max(w, 0.05), 1), min(max(h, 0.05), 1))
    }

    /// Normalised coordinates of the displayed picture under a view point (y-down points), with
    /// the frame texel there, when the image is shown normally (not in the crop tool).
    func pictureLocation(_ p: CGPoint) -> (u: Double, v: Double, texel: (x: Int, y: Int))? {
        guard cropView == nil, let frame = currentFrame, let metalLayer else { return nil }
        let scale = metalLayer.contentsScale
        let place = LoupePlacement.fit(display: frame.displaySize, drawable: metalLayer.drawableSize)
        let d = place.uv(p.x * scale, p.y * scale)
        guard (0...1).contains(d.u), (0...1).contains(d.v) else { return nil }
        let st = CropGeometry.orient(d.u, d.v, frame.orientation)
        return (d.u, d.v, (Int(st.0 * Double(frame.contentWidth)), Int(st.1 * Double(frame.contentHeight))))
    }

    /// Display RGB under a view point (engine frames only).
    func sampleColor(at p: CGPoint) -> (r: UInt8, g: UInt8, b: UInt8)? {
        guard let loc = pictureLocation(p), let frame = currentFrame else { return nil }
        return frame.sampleRGB8(x: loc.texel.x, y: loc.texel.y)
    }

    /// Re-plans surfaces after a crop change (the cropped picture needs a finer level).
    func cropDidChange() { planSurfaces() }
}
