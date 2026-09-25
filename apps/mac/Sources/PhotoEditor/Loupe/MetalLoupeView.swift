import AppKit
import Metal
import QuartzCore

/// Loupe viewport: an NSView backed by a CAMetalLayer configured for EDR (RGBA16F,
/// `wantsExtendedDynamicRangeContent`) whose colour space follows the window's screen.
/// Swift owns presentation; the engine will only supply IOSurfaces (see `LoupeFrame`).
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

    /// Linear exposure gain in stops, applied in the shader (stub for the Basic panel).
    var exposure: Float = 0 {
        didSet { if exposure != oldValue { render() } }
    }

    /// Called with a human-readable description of the colour setup whenever the screen changes.
    var onColorInfoChange: ((String) -> Void)?
    /// Last CPU encode time, for the inspector's latency readout.
    private(set) var lastEncodeTime: Double = 0

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layerContentsRedrawPolicy = .duringViewResize
        setAccessibilityElement(true)
        setAccessibilityRole(.image)
        setAccessibilityLabel("Loupe")
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
        guard let metalLayer else { return }
        let screen = window?.screen ?? NSScreen.main
        let screenSpace = screen?.colorSpace?.cgColorSpace ?? CGColorSpace(name: CGColorSpace.displayP3)!
        let working = CGColorSpaceCreateExtendedLinearized(screenSpace)
            ?? CGColorSpace(name: CGColorSpace.extendedLinearDisplayP3)!
        let changed = working != workingColorSpace
        workingColorSpace = working
        metalLayer.colorspace = working

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

    /// Engine path: present a surface rendered elsewhere, already in `workingColorSpace`.
    func present(frame: LoupeFrame) {
        rasterGeneration += 1
        sourceImage = nil
        currentFrame = frame
        texture = renderer.flatMap { frame.makeTexture(device: $0.device) }
        render()
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
        lastEncodeTime = renderer.draw(in: metalLayer, texture: texture, exposure: exposure,
                                       background: Theme.loupeBackgroundLinear)
    }
}
