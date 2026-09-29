import AppKit
import SwiftUI
import TesseraCore

/// The Content-Aware Move tool's palette slot, after Remove in the Healing Brush group.
struct ContentAwareToolSlot: View {
    @Bindable var cam: DocumentContentAware

    var body: some View {
        IconButton(symbol: "square.on.square.dashed", help: "Content-Aware Move", on: cam.active, size: Theme.Height.large) {
            cam.active ? cam.deactivate() : cam.activate()
        }
        .accessibilityIdentifier("document.tool.contentAwareMove")
    }
}

/// The options bar while Content-Aware Move is on: Mode, Structure, Color, Seed; while a move is arranged, its offset
/// with Cancel and Apply; while it computes, a spinner with Cancel. Errors are an inline `StatusLine`.
struct ContentAwareOptionsBar: View {
    @Bindable var document: DocumentController
    @Bindable var cam: DocumentContentAware

    private var separator: some View { Hairline(vertical: true).frame(height: Theme.Height.small) }

    var body: some View {
        SegmentedPicker(selection: $cam.options.mode, segments: ContentAwareMoveMode.allCases.map {
            .init(value: $0, title: $0.title, help: $0.help)
        }, height: Theme.Height.small, fill: false)
        .fixedSize()
        .disabled(cam.jobs.isBusy)
        .accessibilityIdentifier("document.cam.mode")
        OptionField(title: "Structure", value: Binding(get: { Double(cam.options.structure) },
                                                       set: { cam.options.setStructure(Int($0)) }),
                    range: 1...7, identifier: "document.cam.structure")
            .help("How closely the fill keeps the surrounding structure (patch size 1…7)")
        Text("Color").font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary).fixedSize()
        MenuPicker(selection: $cam.options.seam, options: ContentAwareSeamLevel.allCases.map { ($0, $0.title) })
            .fixedSize()
            .help("How much the moved part's colours adapt to its new surroundings")
            .accessibilityIdentifier("document.cam.color")
        OptionField(title: "Seed", value: Binding(get: { Double(cam.options.seed) }, set: { cam.options.seed = UInt64(max($0, 0)) }),
                    range: 0...999_999, identifier: "document.cam.seed")
            .help("The same seed repeats the same fill")
        separator
        if let busy = cam.busy {
            ProgressView().controlSize(.small)
            Text(busy).font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary).fixedSize()
            Button("Cancel") { cam.cancel() }
                .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                .help("Esc: stops at once; nothing changes")
                .accessibilityIdentifier("document.cam.cancel")
        } else if let s = cam.session {
            Text("\(s.mode.title) by \(cam.offset.dx), \(cam.offset.dy) px").font(Theme.Fonts.captionNumeric)
                .foregroundStyle(Theme.textPrimary).fixedSize()
                .accessibilityIdentifier("document.cam.offset")
            Button("Cancel") { cam.cancel() }
                .buttonStyle(.theme(.borderless, height: Theme.Height.small))
                .help("Esc")
                .accessibilityIdentifier("document.cam.cancel")
            Button("Apply") { cam.apply() }
                .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                .disabled(cam.preview == nil)
                .help("Return")
                .accessibilityIdentifier("document.cam.apply")
        } else {
            Text(ContentAwareMenuState.canStart(layerKind: document.primary?.kind, hasSelection: document.marquee != nil)
                 ?? "Drag the selection to where it should go")
                .font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary).fixedSize()
        }
        if let e = cam.error {
            separator
            StatusLine(text: e, kind: .error).lineLimit(1).truncationMode(.tail)
                .frame(maxWidth: 420, alignment: .leading)
                .help(e)
                .accessibilityIdentifier("document.cam.error")
        }
    }
}

/// On-canvas feedback (on-image set only): the frozen selection's ghost at the offset (1.5 pt `OnImage.guide` over a
/// 3 pt `OnImage.shadow`, dashed while dragging), a faint line from where it was, and a scrim chip with the offset.
@MainActor
final class ContentAwareOverlayView: NSView {
    weak var viewport: DocumentViewportView?

    override var isFlipped: Bool { true }
    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    override func draw(_ dirtyRect: NSRect) {
        guard let v = viewport, let doc = v.controller, DocumentTools.shared.document === doc else { return }
        let m = DocumentContentAware.shared
        guard m.active, let b = m.session?.selectionBounds ?? (m.dragging ? doc.marquee : nil) else { return }
        let (dx, dy) = (Double(m.offset.dx), Double(m.offset.dy))
        guard dx != 0 || dy != 0 || m.dragging else { return }
        // The ghost: the selection outline (marching-ants polylines) moved by the offset.
        let ghost = NSBezierPath()
        let outline = DocumentTools.shared.outline
        if outline.isEmpty {
            let a = v.viewPoint(canvas: CGPoint(x: Double(b.x) + dx, y: Double(b.y) + dy))
            let z = v.viewPoint(canvas: CGPoint(x: Double(b.x + b.width) + dx, y: Double(b.y + b.height) + dy))
            ghost.appendRect(CGRect(x: a.x, y: a.y, width: z.x - a.x, height: z.y - a.y))
        } else {
            for o in outline {
                guard let f = o.points.first else { continue }
                ghost.move(to: v.viewPoint(canvas: CGPoint(x: Double(f.x) + dx, y: Double(f.y) + dy)))
                for p in o.points.dropFirst() { ghost.line(to: v.viewPoint(canvas: CGPoint(x: Double(p.x) + dx, y: Double(p.y) + dy))) }
                if o.closed { ghost.close() }
            }
        }
        ghost.lineWidth = 3
        Theme.Palette.OnImage.shadow.setStroke()
        ghost.stroke()
        ghost.lineWidth = 1.5
        if m.dragging || m.computing { ghost.setLineDash([4, 3], count: 2, phase: 0) }
        Theme.Palette.OnImage.guide.setStroke()
        ghost.stroke()
        // From the original place to the new one.
        let c0 = v.viewPoint(canvas: CGPoint(x: Double(b.x) + Double(b.width) / 2, y: Double(b.y) + Double(b.height) / 2))
        let c1 = CGPoint(x: c0.x + dx * v.pointsPerPixel, y: c0.y + dy * v.pointsPerPixel)
        let line = NSBezierPath()
        line.move(to: c0)
        line.line(to: c1)
        line.lineWidth = 1
        line.setLineDash([2, 3], count: 2, phase: 0)
        Theme.Palette.OnImage.guideFaint.setStroke()
        line.stroke()
        let label = m.computing ? "Computing… \(m.offset.dx), \(m.offset.dy) px" : "\(m.offset.dx), \(m.offset.dy) px"
        chip(label, at: CGPoint(x: c1.x + 8, y: c1.y + 8))
    }

    private func chip(_ text: String, at p: CGPoint) {
        let attrs: [NSAttributedString.Key: Any] = [.font: Theme.NSFonts.caption, .foregroundColor: Theme.Palette.OnImage.text]
        let s = NSAttributedString(string: text, attributes: attrs)
        let size = s.size()
        let r = CGRect(x: p.x, y: p.y, width: size.width + 12, height: 16)
        Theme.Palette.OnImage.scrim.setFill()
        NSBezierPath(roundedRect: r, xRadius: 4, yRadius: 4).fill()
        s.draw(at: CGPoint(x: r.minX + 6, y: r.minY + (16 - size.height) / 2))
    }
}
