import AppKit
import CoreText
import SwiftUI
import TesseraCore

/// The watermark's 3 × 3 anchor grid (WP M2-46): a `well` track like `SegmentedPicker`, the chosen
/// cell raised with the 1 pt shadow and a primary dot, the others a tertiary dot. Neutral, not
/// accent (a control's mode).
struct AnchorPicker: View {
    @Binding var selection: ExportWatermark.Anchor
    @Environment(\.isEnabled) private var enabled

    var body: some View {
        VStack(spacing: Theme.Space.xxs) {
            ForEach(0..<3, id: \.self) { row in
                HStack(spacing: Theme.Space.xxs) {
                    ForEach(0..<3, id: \.self) { column in cell(ExportWatermark.Anchor.allCases[row * 3 + column]) }
                }
            }
        }
        .padding(Theme.Space.xxs)
        .background(RoundedRectangle(cornerRadius: Theme.Radius.control).fill(Theme.well))
        .overlay(RoundedRectangle(cornerRadius: Theme.Radius.control).strokeBorder(Theme.hairline, lineWidth: Theme.Space.hairline))
        .opacity(enabled ? 1 : Theme.Opacity.disabled)
        .accessibilityElement(children: .contain)
    }

    private func cell(_ anchor: ExportWatermark.Anchor) -> some View {
        let on = anchor == selection
        return Button { selection = anchor } label: {
            Circle()
                .fill(on ? Theme.textPrimary : Theme.textTertiary)
                .frame(width: Theme.Space.xs + Theme.Space.xxs, height: Theme.Space.xs + Theme.Space.xxs)
                .frame(width: Theme.Height.small, height: Theme.Height.small - Theme.Space.xs)
                .background(RoundedRectangle(cornerRadius: Theme.Radius.chip)
                    .fill(on ? Theme.raised : Theme.clear)
                    .shadow(color: on ? Theme.shadow.opacity(0.4) : Theme.clear, radius: 1, y: 0.5))
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .help(anchor.title)
        .accessibilityLabel(anchor.title)
        .accessibilityAddTraits(on ? .isSelected : [])
        .accessibilityIdentifier("export-watermark-anchor-\(anchor.rawValue)")
    }
}

/// Where the watermark lands on a 3:2 picture: drawn here with the engine's placement arithmetic
/// (anchor, inset and size as fractions of the short edge, rotation clockwise), or, after
/// "Render with Engine", the engine's own 480 px render of the first photo. A scope-style well
/// (`plotWell`, radius 4) with an on-image chip naming which one is shown.
struct WatermarkPlacementPreview: View {
    let mark: ExportWatermark
    /// The engine render, when it was made with this exact watermark.
    let engineImage: NSImage?

    static let size = CGSize(width: 240, height: 160)
    @State private var graphic: NSImage?

    var body: some View {
        ZStack(alignment: .topLeading) {
            RoundedRectangle(cornerRadius: Theme.Radius.chip).fill(Color(nsColor: Theme.Palette.plotWell))
            if let engineImage {
                Image(nsImage: engineImage)
                    .resizable()
                    .aspectRatio(contentMode: .fit)
                    .frame(width: Self.size.width, height: Self.size.height)
            } else {
                RoundedRectangle(cornerRadius: Theme.Radius.chip)
                    .strokeBorder(Color(nsColor: Theme.Palette.plotGuide), lineWidth: Theme.Space.hairline)
                stamp
            }
            VStack {
                Spacer()
                Text(engineImage == nil ? "Placement preview" : "Rendered by the engine")
                    .font(Theme.Fonts.caption)
                    .foregroundStyle(Color(nsColor: Theme.Palette.OnImage.text))
                    .padding(.horizontal, Theme.Space.xs)
                    .background(RoundedRectangle(cornerRadius: Theme.Radius.chip).fill(Color(nsColor: Theme.Palette.OnImage.scrim)))
                    .padding(Theme.Space.xs)
            }
        }
        .frame(width: Self.size.width, height: Self.size.height)
        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.chip))
        .task(id: mark.path) { graphic = mark.path.isEmpty ? nil : NSImage(contentsOfFile: mark.path) }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("export-watermark-preview")
    }

    @ViewBuilder private var stamp: some View {
        let w = Self.size.width, h = Self.size.height, short = min(w, h)
        switch mark.kind {
        case .text:
            let font = Self.font(path: mark.font, size: max(short * mark.size, 1))
            let text = mark.text.isEmpty ? " " : mark.text
            let measured = (text as NSString).size(withAttributes: [.font: font as NSFont])
            let angle = CGFloat(mark.rotation) * .pi / 180   // explicit CGFloat: Swift 6.2.4 finds cos(angle) ambiguous
            let rw = measured.width * abs(cos(angle)) + measured.height * abs(sin(angle))
            let rh = measured.height * abs(cos(angle)) + measured.width * abs(sin(angle))
            let o = mark.origin(markWidth: rw, markHeight: rh, width: w, height: h)
            Text(text)
                .font(Font(font))
                .foregroundStyle(Color(.sRGB, red: mark.color[safe: 0], green: mark.color[safe: 1], blue: mark.color[safe: 2])) // lint:allow (user watermark colour)
                .fixedSize()
                .frame(width: measured.width, height: measured.height)
                .rotationEffect(.degrees(mark.rotation))
                .opacity(mark.opacity)
                .position(x: o.x + rw / 2, y: o.y + rh / 2)
        case .graphic:
            let gw = max(short * mark.scale, 1)
            let aspect = graphic.map { $0.size.height / max($0.size.width, 1) } ?? 1
            let gh = gw * aspect
            let o = mark.origin(markWidth: gw, markHeight: gh, width: w, height: h)
            Group {
                if let graphic {
                    Image(nsImage: graphic).resizable().opacity(mark.opacity)
                } else {
                    RoundedRectangle(cornerRadius: Theme.Radius.chip)
                        .strokeBorder(Color(nsColor: Theme.Palette.OnImage.guideFaint),
                                      style: StrokeStyle(lineWidth: Theme.Space.hairline, dash: [Theme.Space.xs, Theme.Space.xs - 1]))
                }
            }
            .frame(width: gw, height: gh)
            .position(x: o.x + gw / 2, y: o.y + gh / 2)
        }
    }

    /// The watermark font file at `size` points (the system UI font when the file cannot be read).
    static func font(path: String, size: CGFloat) -> CTFont {
        if !path.isEmpty,
           let descriptors = CTFontManagerCreateFontDescriptorsFromURL(URL(fileURLWithPath: path) as CFURL) as? [CTFontDescriptor],
           let first = descriptors.first {
            return CTFontCreateWithFontDescriptor(first, size, nil)
        }
        return CTFontCreateUIFontForLanguage(.system, size, nil) ?? CTFontCreateWithName("Helvetica" as CFString, size, nil)
    }
}

private extension Array where Element == Double {
    subscript(safe i: Int) -> Double { indices.contains(i) ? Swift.min(Swift.max(self[i], 0), 1) : 1 }
}
