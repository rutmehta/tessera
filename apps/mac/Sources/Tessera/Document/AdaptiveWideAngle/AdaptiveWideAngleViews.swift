import AppKit
import SwiftUI
import TesseraCore

/// Filter ▸ Adaptive Wide Angle… (⌥⇧⌘A, Photoshop's shortcut).
struct AdaptiveWideAngleMenuItem: View {
    let doc: DocumentController?

    var body: some View {
        let awa = DocumentAdaptiveWideAngle.shared
        let target = AdaptiveWideAngleFilter.refusal(kind: doc?.primary?.kind) == nil
        Button(AdaptiveWideAngleFilter.menuTitle) { if let doc { awa.open(doc) } }
            .shortcut(doc != nil, "a", [.command, .option, .shift])
            .disabled(doc == nil || !target || awa.opening || awa.workspace != nil)
    }
}

/// The workspace sheet on the document view.
struct AdaptiveWideAngleSheets: ViewModifier {
    @Bindable var awa: DocumentAdaptiveWideAngle

    func body(content: Content) -> some View {
        let _ = AdaptiveWideAngleSelfTest.startIfRequested()
        return content.sheet(item: $awa.workspace) { AdaptiveWideAngleSheet(model: $0) }
    }
}

/// The workspace: the canvas, then Correction / Constraints / View controls. Footer: status, Cancel, OK.
/// Identifiers: `document.awa.canvas`, `.projection`, `.focal`, `.useExif`, `.scale`, `.orientation`, `.removeLine`,
/// `.removeAll`, `.preview`, `.showConstraints`, `.error`, `.cancel`, `.ok`.
struct AdaptiveWideAngleSheet: View {
    @Bindable var model: AdaptiveWideAngleWorkspaceModel
    private let ident = "document.awa"

    var body: some View {
        SheetScaffold(title: model.title, subtitle: model.subtitle) {
            EmptyView()
        } content: {
            HStack(spacing: 0) {
                AdaptiveWideAngleCanvas(model: model, revision: model.revision)
                    .frame(minWidth: 560, maxWidth: .infinity, minHeight: 420, maxHeight: .infinity)
                    .accessibilityIdentifier("\(ident).canvas")
                Hairline(vertical: true)
                ScrollView { controls.padding(Theme.Space.l) }
                    .frame(width: 300)
            }
            .disabled(model.busy != nil)
        } leading: {
            footer
        } actions: {
            Button("Cancel") { model.cancel() }
                .keyboardShortcut(.cancelAction)
                .sheetButton()
                .accessibilityIdentifier("\(ident).cancel")
            Button("OK") { model.ok() }
                .keyboardShortcut(.defaultAction)
                .sheetButton(primary: true)
                .disabled(model.busy != nil)
                .accessibilityIdentifier("\(ident).ok")
        }
        .frame(minWidth: 940, idealWidth: 1120, minHeight: 600, idealHeight: 760)
    }

    @ViewBuilder private var controls: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            SubHeader("Correction")
            SegmentedPicker(selection: $model.projection,
                            segments: AdaptiveProjection.allCases.map { .init(value: $0, title: $0.title) },
                            height: Theme.Height.small)
                .accessibilityIdentifier("\(ident).projection")
            DocSlider(title: "Focal Length", value: model.draft.focal35, range: AdaptiveWideAngleFilter.focalRange,
                      defaultValue: model.info.exifFocal35mm ?? 24, format: "%.1f mm", step: 0.5,
                      identifier: "\(ident).focal", revision: model.revision) { v, _ in model.setFocal(v) }
                .frame(height: Theme.Height.slider)
            if let f = model.info.exifFocal35mm {
                HStack(spacing: Theme.Space.xs) {
                    Hint(String(format: "EXIF: %.0f mm (35 mm equivalent)", f))
                    Button("Use") { model.useExifFocal() }
                        .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                        .accessibilityIdentifier("\(ident).useExif")
                }
            } else {
                Hint("35 mm equivalent. Adjust until the traced lines follow the edges.")
            }
            DocSlider(title: "Scale", value: model.draft.scalePercent, range: AdaptiveWideAngleFilter.scaleRange,
                      defaultValue: 100, format: "%.0f %%", step: 1,
                      identifier: "\(ident).scale", revision: model.revision) { v, _ in model.setScale(v) }
                .frame(height: Theme.Height.slider)

            SubHeader("Constraints")
            Text("\(model.draft.lines.count) line\(model.draft.lines.count == 1 ? "" : "s")")
                .font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
            if let i = model.selected, model.draft.lines.indices.contains(i) {
                SegmentedPicker(selection: Binding(get: { model.draft.lines[i].orientation }, set: { model.setOrientation($0) }),
                                segments: AdaptiveLineOrientation.allCases.map { .init(value: $0, title: $0.title) },
                                height: Theme.Height.small)
                    .accessibilityIdentifier("\(ident).orientation")
                Button("Remove Line") { model.removeSelected() }
                    .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                    .accessibilityIdentifier("\(ident).removeLine")
            } else {
                Hint("Drag on the image along an edge that should be straight; ⇧ makes it horizontal or vertical. Click a line to select it; Delete removes it.")
            }
            Button("Remove All") { model.removeAll() }
                .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                .disabled(model.draft.lines.isEmpty)
                .accessibilityIdentifier("\(ident).removeAll")

            SubHeader("View")
            Toggle("Preview (P)", isOn: $model.preview).toggleStyle(.checkbox).font(Theme.Fonts.caption)
                .accessibilityIdentifier("\(ident).preview")
            Toggle("Show constraints", isOn: $model.showConstraints).toggleStyle(.checkbox).font(Theme.Fonts.caption)
                .accessibilityIdentifier("\(ident).showConstraints")
            Hint(model.previewNote)
            Hint("Lines are drawn on the original; Preview shows the corrected result.")
        }
    }

    @ViewBuilder private var footer: some View {
        if let busy = model.busy {
            ProgressView().controlSize(.small)
            TimelineView(.periodic(from: .now, by: 0.5)) { _ in
                Text(String(format: "%@ %.0f s · Esc cancels", busy, model.jobs.seconds)).font(Theme.Fonts.captionNumeric).fixedSize()
            }
        } else if let e = model.error {
            StatusLine(text: e, kind: .error).lineLimit(2).truncationMode(.tail)
                .accessibilityIdentifier("\(ident).error")
        } else if let ms = model.previewMillis {
            Text(String(format: "Preview %.0f ms", ms)).font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textTertiary)
        }
    }
}

// MARK: - Canvas

struct AdaptiveWideAngleCanvas: NSViewRepresentable {
    let model: AdaptiveWideAngleWorkspaceModel
    /// The model's revision (read by the parent body, so a change redraws the canvas).
    let revision: Int

    func makeNSView(context: Context) -> AdaptiveWideAngleCanvasView {
        let v = AdaptiveWideAngleCanvasView()
        v.model = model
        return v
    }

    func updateNSView(_ v: AdaptiveWideAngleCanvasView, context: Context) {
        v.model = model
        v.needsDisplay = true
    }
}

/// The proxy image on the checkerboard; on the original, the traced constraint curves (`OnImage.guide` over
/// `OnImage.shadow`, the selected one wider) and the line being drawn. Drag draws; click selects; scroll pans;
/// pinch or ⌘-scroll zooms; ⌥ double-click fits.
@MainActor
final class AdaptiveWideAngleCanvasView: NSView {
    var model: AdaptiveWideAngleWorkspaceModel?

    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }

    override init(frame: NSRect) {
        super.init(frame: frame)
        clipsToBounds = true
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        window?.makeFirstResponder(self)
    }

    override func layout() {
        super.layout()
        model?.fitIfNeeded(in: bounds.size)
    }

    override func draw(_ dirtyRect: NSRect) {
        guard let m = model, let ctx = NSGraphicsContext.current?.cgContext else { return }
        m.fitIfNeeded(in: bounds.size)
        Theme.Palette.canvas.setFill()
        bounds.fill()
        let rect = m.view.viewRect(source: m.info.sourceSize)
        Theme.Palette.checkerLight.setFill()
        rect.intersection(bounds).fill()
        if let img = m.image {
            ctx.saveGState()
            ctx.interpolationQuality = .medium
            ctx.translateBy(x: rect.minX, y: rect.maxY)
            ctx.scaleBy(x: 1, y: -1)
            ctx.draw(img, in: CGRect(origin: .zero, size: rect.size))
            ctx.restoreGState()
        }
        guard m.overlayVisible else { return }
        for (i, line) in m.draft.lines.enumerated() {
            stroke(line.points.map { m.view.viewPoint(source: $0) }, width: i == m.selected ? 2.5 : 1.25)
            for p in [line.from, line.to] { handle(m.view.viewPoint(source: p)) }
        }
        if let r = m.rubberBand {
            stroke([m.view.viewPoint(source: r.from), m.view.viewPoint(source: r.to)], width: 1.25, dashed: true)
        }
    }

    private func stroke(_ pts: [CGPoint], width: CGFloat, dashed: Bool = false) {
        guard let first = pts.first else { return }
        let path = NSBezierPath()
        path.move(to: first)
        for p in pts.dropFirst() { path.line(to: p) }
        if dashed { path.setLineDash([4, 3], count: 2, phase: 0) }
        path.lineWidth = width + 1.5
        Theme.Palette.OnImage.shadow.setStroke()
        path.stroke()
        path.lineWidth = width
        Theme.Palette.OnImage.guide.setStroke()
        path.stroke()
    }

    private func handle(_ p: CGPoint) {
        let r: CGFloat = 3
        let dot = NSBezierPath(ovalIn: CGRect(x: p.x - r, y: p.y - r, width: 2 * r, height: 2 * r))
        Theme.Palette.OnImage.guide.setFill()
        dot.fill()
        dot.lineWidth = 1
        Theme.Palette.OnImage.shadow.setStroke()
        dot.stroke()
    }

    private func local(_ e: NSEvent) -> CGPoint { convert(e.locationInWindow, from: nil) }

    override func mouseDown(with e: NSEvent) {
        window?.makeFirstResponder(self)
        guard let m = model else { return }
        if e.clickCount == 2, e.modifierFlags.contains(.option) { m.fit(in: bounds.size); return }
        m.pointerDown(m.view.sourcePoint(view: local(e)), tolerance: 6 / m.view.scale)
    }

    override func mouseDragged(with e: NSEvent) {
        guard let m = model else { return }
        m.pointerDragged(m.view.sourcePoint(view: local(e)))
    }

    override func mouseUp(with e: NSEvent) {
        guard let m = model else { return }
        m.pointerUp(m.view.sourcePoint(view: local(e)), constrain: e.modifierFlags.contains(.shift))
    }

    override func scrollWheel(with e: NSEvent) {
        guard let m = model else { return }
        if e.modifierFlags.contains(.command) {
            m.zoom(by: exp(Double(e.scrollingDeltaY) * (e.hasPreciseScrollingDeltas ? 0.01 : 0.1)), around: local(e))
        } else {
            m.pan(dx: Double(e.scrollingDeltaX), dy: Double(e.scrollingDeltaY))
        }
    }

    override func magnify(with e: NSEvent) {
        model?.zoom(by: 1 + Double(e.magnification), around: local(e))
    }

    override func keyDown(with e: NSEvent) {
        if model?.key(e) == true { return }
        super.keyDown(with: e)
    }
}
