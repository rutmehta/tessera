import AppKit
import SwiftUI
import TesseraCore
import TesseraFFI

/// RGB + luminance histogram of the displayed engine frame (256 bins each, from
/// `DevelopSession.get_histogram`). AppKit-drawn and fed straight from the render callback, so
/// slider drags never touch SwiftUI state.
final class HistogramView: NSView {
    var histogram: Histogram? { didSet { needsDisplay = true } }
    var placeholder = "No develop session" { didSet { needsDisplay = true } }

    override var isFlipped: Bool { false }
    static let height: CGFloat = 88
    override var intrinsicContentSize: NSSize { NSSize(width: NSView.noIntrinsicMetric, height: Self.height) }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        needsDisplay = true
    }

    override init(frame: NSRect) {
        super.init(frame: frame)
        setAccessibilityElement(true)
        setAccessibilityRole(.image)
        setAccessibilityLabel("Histogram")
    }
    required init?(coder: NSCoder) { fatalError() }

    override func draw(_ dirtyRect: NSRect) {
        let r = bounds.insetBy(dx: 0.5, dy: 0.5)
        Theme.Palette.plotWell.setFill()
        NSBezierPath(roundedRect: r, xRadius: Theme.Radius.chip, yRadius: Theme.Radius.chip).fill()
        guard let h = histogram, h.luminance.count == 256 else {
            let attrs: [NSAttributedString.Key: Any] = [.font: Theme.NSFonts.caption,
                                                        .foregroundColor: Theme.Palette.plotText]
            let text = placeholder as NSString
            let size = text.size(withAttributes: attrs)
            text.draw(at: NSPoint(x: r.midX - size.width / 2, y: r.midY - size.height / 2), withAttributes: attrs)
            return
        }
        // Square-root scale keeps shadows visible next to a tall spike; ignore the clipped end bins
        // when normalising so a blown sky does not flatten the rest.
        let channels: [([UInt32], NSColor)] = [
            (h.red, Theme.Palette.channelRed.withAlphaComponent(0.55)),
            (h.green, Theme.Palette.channelGreen.withAlphaComponent(0.55)),
            (h.blue, Theme.Palette.channelBlue.withAlphaComponent(0.55)),
        ]
        let peak = ([h.red, h.green, h.blue, h.luminance].flatMap { $0[1..<255] }.max()).map { sqrt(Double($0)) } ?? 1
        let plot = r.insetBy(dx: 3, dy: 3)
        func path(_ bins: [UInt32]) -> NSBezierPath {
            let p = NSBezierPath()
            p.move(to: NSPoint(x: plot.minX, y: plot.minY))
            for (i, v) in bins.enumerated() {
                let x = plot.minX + plot.width * CGFloat(i) / 255
                let y = plot.minY + plot.height * CGFloat(min(sqrt(Double(v)) / max(peak, 1), 1))
                p.line(to: NSPoint(x: x, y: y))
            }
            p.line(to: NSPoint(x: plot.maxX, y: plot.minY))
            p.close()
            return p
        }
        NSGraphicsContext.current?.compositingOperation = .plusLighter
        for (bins, color) in channels {
            color.setFill()
            path(bins).fill()
        }
        NSGraphicsContext.current?.compositingOperation = .sourceOver
        Theme.Palette.plotLine.withAlphaComponent(0.8).setStroke()
        let luma = path(h.luminance)
        luma.lineWidth = 1
        luma.stroke()
        // Clipping indicators: shadows (left) and highlights (right) when more than 0.5% of pixels clip.
        let total = Double(h.luminance.reduce(0) { $0 + UInt64($1) })
        let clip = { (i: Int) -> Bool in
            total > 0 && Double([h.red[i], h.green[i], h.blue[i]].max()!) / total > 0.005
        }
        for (i, x) in [(0, plot.minX), (255, plot.maxX - 6)] where clip(i) {
            Theme.Palette.plotLine.setFill()
            NSBezierPath(rect: NSRect(x: x, y: plot.maxY - 6, width: 6, height: 6)).fill()
        }
    }
}

struct HistogramPanel: NSViewRepresentable {
    let model: AppModel

    @MainActor final class Coordinator: LibraryObserver {
        let view = HistogramView(frame: .zero)
        weak var model: AppModel?
        init(model: AppModel) {
            self.model = model
            model.addObserver(self)
            developDidChange()
        }
        func libraryDidReload() { developDidChange() }
        func itemsDidChange(_ positions: IndexSet) {}
        func selectionDidChange(scrollToFocus: Bool) {}
        func developDidChange() {
            guard let model else { return }
            view.histogram = model.develop?.histogram
            view.placeholder = switch model.developStatus {
            case .loading: "Rendering…"
            case .unavailable(let why): why
            default: "Open a photo in the loupe (E)"
            }
        }
        func developDidRender(_ frame: DevelopFrame, controller: DevelopController) {
            view.histogram = controller.histogram
        }
    }

    func makeCoordinator() -> Coordinator { Coordinator(model: model) }
    func makeNSView(context: Context) -> HistogramView { context.coordinator.view }
    func updateNSView(_ nsView: HistogramView, context: Context) {
        _ = model.developStatus   // re-run when the session state changes
        context.coordinator.developDidChange()
    }
}
