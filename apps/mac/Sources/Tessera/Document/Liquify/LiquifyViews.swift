import AppKit
import SwiftUI
import TesseraCore

/// Filter ▸ Liquify… (⇧⌘X).
struct LiquifyMenuItem: View {
    let doc: DocumentController?

    var body: some View {
        // The menu bar is built at launch: one of the places `--liquify-selftest` starts.
        let _ = LiquifySelfTest.startIfRequested()
        let liquify = DocumentLiquify.shared
        Button("Liquify…") { if let doc { liquify.open(doc) } }
            .keyboardShortcut("x", modifiers: [.command, .shift])
            .disabled(!LiquifyMenuState.enabled(layerKind: doc?.primary?.kind, busy: liquify.opening || liquify.workspace != nil))
    }
}

/// The Liquify workspace sheet on the document view.
struct LiquifySheets: ViewModifier {
    @Bindable var liquify: DocumentLiquify

    func body(content: Content) -> some View {
        // Also the self-test's start in a background launch, where the menu bar may never be built.
        let _ = LiquifySelfTest.startIfRequested()
        return content.sheet(item: $liquify.workspace) { LiquifyWorkspaceSheet(model: $0) }
    }
}

/// The workspace: tools on the left, the canvas, brush / mesh / reconstruct / view / output controls on the right.
/// Footer: status on the left (brush latency, busy, errors), Cancel and Apply (primary) on the right.
struct LiquifyWorkspaceSheet: View {
    @Bindable var model: LiquifyWorkspaceModel

    private let panelWidth: CGFloat = 312

    var body: some View {
        SheetScaffold(title: model.title, subtitle: model.subtitle) {
            if model.info.selectionFrozen {
                Chip(text: "Outside the selection frozen", color: Theme.textSecondary, style: .outlined)
            }
        } content: {
            HStack(spacing: 0) {
                toolColumn
                Hairline(vertical: true)
                LiquifyCanvas(model: model, revision: model.revision)
                    .frame(minWidth: 480, maxWidth: .infinity, minHeight: 420, maxHeight: .infinity)
                    .accessibilityIdentifier("document.liquify.canvas")
                Hairline(vertical: true)
                ScrollView { controls.padding(Theme.Space.l) }
                    .frame(width: panelWidth)
            }
        } leading: {
            footerStatus
        } actions: {
            Button("Cancel") { model.cancel() }
                .buttonStyle(.theme(.bordered, height: Theme.Height.large))
                .keyboardShortcut(.cancelAction)
                .accessibilityIdentifier("document.liquify.cancel")
            Button("Apply") { model.apply() }
                .buttonStyle(.theme(.primary, height: Theme.Height.large))
                .keyboardShortcut(.defaultAction)
                .disabled(model.busy != nil || model.outputs.first { $0.0 == model.output }?.1 != nil)
                .accessibilityIdentifier("document.liquify.apply")
        }
        .frame(minWidth: 1040, idealWidth: 1240, minHeight: 640, idealHeight: 820)
    }

    // MARK: Tools

    private var toolColumn: some View {
        VStack(spacing: Theme.Space.xxs) {
            ForEach(Array(LiquifyToolKind.allCases.enumerated()), id: \.element) { i, t in
                if i == 1 || i == 3 || i == 8 {
                    Hairline().frame(width: Theme.Height.large).padding(.vertical, Theme.Space.xxs)
                }
                IconButton(symbol: t.symbol, help: t.title + (t.key.map { " (\(String($0).uppercased()))" } ?? " (⌥ with Twirl)"),
                           on: model.tool == t, size: Theme.Height.large) { model.tool = t }
                    .accessibilityIdentifier("document.liquify.tool.\(t.rawValue)")
            }
            Spacer(minLength: 0)
        }
        .padding(Theme.Space.s)
    }

    // MARK: Controls

    @ViewBuilder private var controls: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            Text(model.tool.title).font(Theme.Fonts.labelMedium).foregroundStyle(Theme.textPrimary)
            SubHeader("Brush")
            slider("Size", model.brush.size, LiquifyBrushSettings.sizeRange, 100, "%.0f px", id: "size") { model.brush.setSize($0) }
            slider("Density", model.brush.density, 0...100, 50, "%.0f %%", id: "density") { model.brush.setDensity($0) }
            slider("Pressure", model.brush.pressure, 1...100, 100, "%.0f %%", id: "pressure") { model.brush.setPressure($0) }
            slider("Rate", model.brush.rate, 0...100, 80, "%.0f %%", id: "rate", enabled: model.tool.usesRate) { model.brush.setRate($0) }
            if !model.tool.usesRate { Hint("Rate applies to tools that act while the pointer rests") }

            SubHeader("Mesh and mask")
            Toggle("Show mesh", isOn: $model.showMesh).toggleStyle(.checkbox).font(Theme.Fonts.caption)
                .accessibilityIdentifier("document.liquify.showMesh")
            SegmentedPicker(selection: $model.meshSize, segments: LiquifyWorkspaceModel.MeshSize.allCases.map {
                .init(value: $0, title: $0.title)
            }, height: Theme.Height.small)
            .disabled(!model.showMesh)
            Toggle("Show mask", isOn: $model.showMask).toggleStyle(.checkbox).font(Theme.Fonts.caption)
                .accessibilityIdentifier("document.liquify.showMask")
            HStack(spacing: Theme.Space.s) {
                Button("Freeze All") { model.freezeAll() }.buttonStyle(.theme(.bordered, height: Theme.Height.small))
                Button("Thaw All") { model.thawAll() }.buttonStyle(.theme(.bordered, height: Theme.Height.small))
                    .accessibilityIdentifier("document.liquify.thawAll")
            }

            SubHeader("Reconstruct")
            slider("Amount", model.reconstructAmount, 0...100, 100, "%.0f %%", id: "reconstructAmount") { model.reconstructAmount = $0 }
            HStack(spacing: Theme.Space.s) {
                Button("Reconstruct") { model.reconstructAll() }.buttonStyle(.theme(.bordered, height: Theme.Height.small))
                    .help("Moves every unfrozen area back towards the original by the amount")
                    .accessibilityIdentifier("document.liquify.reconstruct")
                Button("Restore All") { model.restoreAll() }.buttonStyle(.theme(.bordered, height: Theme.Height.small))
                    .help("Removes all distortion; the freeze mask stays")
                    .accessibilityIdentifier("document.liquify.restoreAll")
            }

            SubHeader("View")
            Toggle("Show original (P)", isOn: $model.showOriginal).toggleStyle(.checkbox).font(Theme.Fonts.caption)
                .accessibilityIdentifier("document.liquify.showOriginal")
            Hint(model.previewNote)

            SubHeader("Output")
            let outs = model.outputs
            SegmentedPicker(selection: $model.output, segments: outs.filter { $0.1 == nil }.map {
                .init(value: $0.0, title: $0.0.title)
            }, height: Theme.Height.small)
            .accessibilityIdentifier("document.liquify.output")
            ForEach(outs.filter { $0.1 != nil }, id: \.0) { o in
                Text("\(o.0.title): \(o.1 ?? "")").font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Hint("Face-Aware Liquify is not available in this build.")
        }
    }

    private func slider(_ title: String, _ value: Double, _ range: ClosedRange<Double>, _ def: Double, _ format: String,
                        id: String, enabled: Bool = true, _ set: @escaping (Double) -> Void) -> some View {
        DocSlider(title: title, value: value, range: range, defaultValue: def, format: format, enabled: enabled,
                  identifier: "document.liquify.\(id)", revision: model.revision) { v, _ in set(v) }
            .frame(height: Theme.Height.slider)
    }

    @ViewBuilder private var footerStatus: some View {
        if let busy = model.busy {
            ProgressView().controlSize(.small)
            TimelineView(.periodic(from: .now, by: 0.5)) { _ in
                Text(String(format: "%@ %.0f s · Esc cancels", busy, model.jobs.seconds)).font(Theme.Fonts.captionNumeric).fixedSize()
            }
        } else if let e = model.error {
            StatusLine(text: e, kind: .error).lineLimit(1).truncationMode(.tail)
                .accessibilityIdentifier("document.liquify.error")
        } else if let l = model.latencyText {
            Text(l).font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textTertiary)
                .accessibilityIdentifier("document.liquify.latency")
        } else {
            Text("W warp · R reconstruct · E smooth · C twirl (⌥ reverses) · S pucker · B bloat · O push left · F freeze · D thaw · [ ] size")
                .font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary).lineLimit(1).truncationMode(.tail)
        }
    }
}

// MARK: - Canvas

struct LiquifyCanvas: NSViewRepresentable {
    let model: LiquifyWorkspaceModel
    /// The model's overlay revision (read by the parent body, so a change redraws the canvas).
    let revision: Int

    func makeNSView(context: Context) -> LiquifyCanvasView {
        let v = LiquifyCanvasView()
        v.model = model
        return v
    }

    func updateNSView(_ v: LiquifyCanvasView, context: Context) {
        v.model = model
        v.needsDisplay = true
    }
}

/// The workspace canvas: the proxy preview on the checkerboard, the freeze mask (`OnImage.reject` at 45 %), the warped
/// mesh (`OnImage.guideFaint` over `OnImage.shadow`) and the brush outline (`OnImage.guide` over `OnImage.shadow`).
/// Drag paints; scroll pans; pinch or ⌘-scroll zooms around the pointer; double-click with ⌥ fits.
@MainActor
final class LiquifyCanvasView: NSView {
    var model: LiquifyWorkspaceModel?
    private var pointer: CGPoint?
    private var tracking: NSTrackingArea?

    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }

    override init(frame: NSRect) {
        super.init(frame: frame)
        // A zoomed image must never draw over the tool column or the controls.
        clipsToBounds = true
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        window?.makeFirstResponder(self)
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let t = tracking { removeTrackingArea(t) }
        let t = NSTrackingArea(rect: bounds, options: [.mouseMoved, .mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect],
                               owner: self, userInfo: nil)
        addTrackingArea(t)
        tracking = t
    }

    override func layout() {
        super.layout()
        model?.fitIfNeeded(in: bounds.size)
    }

    // MARK: Drawing

    override func draw(_ dirtyRect: NSRect) {
        guard let m = model, let ctx = NSGraphicsContext.current?.cgContext else { return }
        m.fitIfNeeded(in: bounds.size)
        Theme.Palette.canvas.setFill()
        bounds.fill()
        let rect = m.view.viewRect(source: m.info.sourceSize)
        drawChecker(rect, ctx)
        if let img = m.image {
            ctx.saveGState()
            ctx.interpolationQuality = m.view.scale * Double(m.info.previewFactor) >= 2 ? .none : .medium
            // Flipped view: draw the image upright.
            ctx.translateBy(x: rect.minX, y: rect.maxY)
            ctx.scaleBy(x: 1, y: -1)
            ctx.draw(img, in: CGRect(origin: .zero, size: rect.size))
            ctx.restoreGState()
        }
        if m.showMask, let mesh = m.mesh, mesh.hasFreeze { drawFreeze(mesh, m, ctx) }
        if m.showMesh, let mesh = m.mesh { drawMesh(mesh, m) }
        drawBrush(m)
    }

    private func drawChecker(_ rect: CGRect, _ ctx: CGContext) {
        let clip = rect.intersection(bounds)
        guard !clip.isEmpty else { return }
        Theme.Palette.checkerLight.setFill()
        clip.fill()
        Theme.Palette.checkerDark.setFill()
        let s = Theme.Space.s
        var y = floor((clip.minY - rect.minY) / s) * s + rect.minY
        while y < clip.maxY {
            var x = floor((clip.minX - rect.minX) / s) * s + rect.minX
            while x < clip.maxX {
                if (Int(((x - rect.minX) / s).rounded()) + Int(((y - rect.minY) / s).rounded())) % 2 == 1 {
                    CGRect(x: x, y: y, width: s, height: s).intersection(clip).fill()
                }
                x += s
            }
            y += s
        }
    }

    private func drawFreeze(_ mesh: LiquifyMeshData, _ m: LiquifyWorkspaceModel, _ ctx: CGContext) {
        var bytes = mesh.freezeBytes
        guard let provider = CGDataProvider(data: Data(bytes: &bytes, count: bytes.count) as CFData),
              let mask = CGImage(width: mesh.columns, height: mesh.rows, bitsPerComponent: 8, bitsPerPixel: 8, bytesPerRow: mesh.columns,
                                 space: CGColorSpaceCreateDeviceGray(), bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.none.rawValue),
                                 provider: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent) else { return }
        // Node (c, r) sits at (c·cell, r·cell): each mask pixel is centred on its node.
        let cell = Double(mesh.cellSize)
        let a = m.view.viewPoint(source: CGPoint(x: -cell / 2, y: -cell / 2))
        let size = CGSize(width: Double(mesh.columns) * cell * m.view.scale, height: Double(mesh.rows) * cell * m.view.scale)
        ctx.saveGState()
        ctx.clip(to: m.view.viewRect(source: m.info.sourceSize))
        ctx.translateBy(x: a.x, y: a.y + size.height)
        ctx.scaleBy(x: 1, y: -1)
        ctx.clip(to: CGRect(origin: .zero, size: size), mask: mask)
        ctx.setFillColor(Theme.Palette.OnImage.reject.withAlphaComponent(0.45).cgColor(for: self))
        ctx.fill(CGRect(origin: .zero, size: size))
        ctx.restoreGState()
    }

    private func drawMesh(_ mesh: LiquifyMeshData, _ m: LiquifyWorkspaceModel) {
        let step = mesh.lineStep(scale: m.view.scale, minSpacing: m.meshSize.spacing)
        let path = NSBezierPath()
        for line in mesh.gridLines(step: step) {
            guard let first = line.first else { continue }
            path.move(to: m.view.viewPoint(source: first))
            for p in line.dropFirst() { path.line(to: m.view.viewPoint(source: p)) }
        }
        path.lineWidth = 2
        Theme.Palette.OnImage.shadow.setStroke()
        path.stroke()
        path.lineWidth = 1
        Theme.Palette.OnImage.guideFaint.setStroke()
        path.stroke()
    }

    private func drawBrush(_ m: LiquifyWorkspaceModel) {
        guard let p = pointer else { return }
        let r = max(m.view.viewLength(m.brush.size) / 2, 2)
        let circle = NSBezierPath(ovalIn: CGRect(x: p.x - r, y: p.y - r, width: 2 * r, height: 2 * r))
        circle.lineWidth = 3
        Theme.Palette.OnImage.shadow.setStroke()
        circle.stroke()
        circle.lineWidth = 1
        Theme.Palette.OnImage.guide.setStroke()
        circle.stroke()
        // The hard core (density).
        let c = r * m.brush.density / 100
        if c > 2 {
            let core = NSBezierPath(ovalIn: CGRect(x: p.x - c, y: p.y - c, width: 2 * c, height: 2 * c))
            core.lineWidth = 1
            Theme.Palette.OnImage.guideFaint.setStroke()
            core.stroke()
        }
    }

    // MARK: Input

    private func local(_ e: NSEvent) -> CGPoint { convert(e.locationInWindow, from: nil) }

    private func pressure(_ e: NSEvent) -> Float { e.subtype == .tabletPoint ? max(e.pressure, 0.05) : 1 }

    override func mouseDown(with e: NSEvent) {
        window?.makeFirstResponder(self)
        guard let m = model else { return }
        let p = local(e)
        pointer = p
        if e.clickCount == 2, e.modifierFlags.contains(.option) { m.fit(in: bounds.size); return }
        m.pointerDown(m.view.sourcePoint(view: p), pressure: pressure(e), option: e.modifierFlags.contains(.option))
        needsDisplay = true
    }

    override func mouseDragged(with e: NSEvent) {
        guard let m = model else { return }
        let p = local(e)
        pointer = p
        m.pointerDragged(m.view.sourcePoint(view: p), pressure: pressure(e))
        needsDisplay = true
    }

    override func mouseUp(with e: NSEvent) {
        guard let m = model else { return }
        m.pointerUp(m.view.sourcePoint(view: local(e)), pressure: pressure(e))
        needsDisplay = true
    }

    override func mouseMoved(with e: NSEvent) {
        pointer = local(e)
        needsDisplay = true
    }

    override func mouseExited(with e: NSEvent) {
        pointer = nil
        needsDisplay = true
    }

    override func scrollWheel(with e: NSEvent) {
        guard let m = model else { return }
        if e.modifierFlags.contains(.command) {
            m.zoom(by: exp(Double(e.scrollingDeltaY) * (e.hasPreciseScrollingDeltas ? 0.01 : 0.1)), around: local(e))
        } else {
            m.pan(dx: Double(e.scrollingDeltaX), dy: Double(e.scrollingDeltaY))
        }
        needsDisplay = true
    }

    override func magnify(with e: NSEvent) {
        model?.zoom(by: 1 + Double(e.magnification), around: local(e))
        needsDisplay = true
    }

    override func keyDown(with e: NSEvent) {
        if model?.key(e) == true { needsDisplay = true; return }
        if let c = e.charactersIgnoringModifiers, e.modifierFlags.contains(.command), let m = model {
            let centre = CGPoint(x: bounds.midX, y: bounds.midY)
            switch c {
            case "=", "+": m.zoom(by: 2, around: centre); return
            case "-": m.zoom(by: 0.5, around: centre); return
            case "0": m.fit(in: bounds.size); return
            case "1": m.actualPixels(in: bounds.size); return
            default: break
            }
        }
        super.keyDown(with: e)
    }
}
