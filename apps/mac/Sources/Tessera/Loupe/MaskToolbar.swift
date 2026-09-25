import SwiftUI
import TesseraCore
import TesseraFFI

/// The loupe's masking toolbar (M2-14): tools, AI masks, overlay and the brush HUD. Hidden until
/// masking is on (M or the Masks button); the tools themselves act in `LoupeToolOverlay`.
struct MaskToolbar: View {
    let model: AppModel
    @Bindable var masks: MaskTools

    var body: some View {
        let ready = model.developStatus == .ready
        VStack(spacing: Theme.Space.xs) {
            if masks.active {
                HStack(spacing: Theme.Space.xxs) {
                    ForEach([MaskTool.brush, .linear, .radial, .colorRange, .luminanceRange], id: \.self) { t in
                        toolButton(t)
                    }
                    separator
                    aiButton("Subject", "person.and.background.dotted", .subject)
                    aiButton("Sky", "cloud.sun", .sky)
                    aiButton("Background", "photo.on.rectangle", .background)
                    toolButton(.person)
                    toolButton(.object)
                    separator
                    overlayControls
                    separator
                    Button("Done") { masks.setActive(false) }
                        .buttonStyle(.theme(.borderless, height: Theme.Height.large))
                        .help("Leave masking (M)")
                }
                .padding(Theme.Space.xs)
                .background(HUDBackground())
                if masks.tool == .brush {
                    brushHUD
                        .padding(.horizontal, Theme.Space.m)
                        .frame(height: Theme.Height.large + Theme.Space.xs)
                        .background(HUDBackground())
                }
                if masks.list.busy, let p = masks.list.progress.values.first {
                    HStack(spacing: Theme.Space.s) {
                        ProgressView(value: Double(p.fraction)).frame(width: 96).controlSize(.small)
                        Text(p.message).font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                    }
                    .padding(.horizontal, Theme.Space.m)
                    .frame(height: Theme.Height.large)
                    .background(HUDBackground())
                }
            }
            Spacer()
        }
        // Below the loupe's 32 pt information strip, never over its text.
        .padding(.top, Theme.Height.sectionHeader + Theme.Space.xs)
        .disabled(!ready)
        .tint(Theme.accent)
    }

    private var separator: some View {
        Hairline(vertical: true).frame(height: Theme.Height.small).padding(.horizontal, Theme.Space.xs)
    }

    private func toolButton(_ t: MaskTool) -> some View {
        IconButton(symbol: t.symbol, help: "\(t.title) — \(t.hint)", on: masks.tool == t, size: Theme.Height.large) {
            masks.target = nil
            masks.tool = masks.tool == t ? nil : t
        }
    }

    private func aiButton(_ title: String, _ symbol: String, _ request: AiMaskRequest) -> some View {
        Button {
            masks.runAI(request, title: title, option: NSEvent.modifierFlags.contains(.option))
        } label: {
            Label(title, systemImage: symbol).labelStyle(.titleAndIcon)
        }
        .buttonStyle(.theme(.borderless, height: Theme.Height.large))
        .help("Select the \(title.lowercased()) with the on-device model (⌥ subtracts from the selected mask)")
    }

    private var overlayControls: some View {
        HStack(spacing: Theme.Space.xxs) {
            IconButton(symbol: masks.overlayOn ? "eye.fill" : "eye.slash", help: "Show the mask overlay (O)",
                       on: masks.overlayOn, size: Theme.Height.large) {
                masks.overlayOn.toggle()
            }
            Menu {
                ForEach(MaskOverlayColor.allCases, id: \.self) { c in
                    Button(c.rawValue.capitalized) { masks.overlayColor = c; masks.overlayOn = true }
                }
                Divider()
                ForEach([0.3, 0.5, 0.7, 1.0], id: \.self) { o in
                    Button(String(format: "Opacity %.0f%%", o * 100)) { masks.overlayOpacity = o }
                }
            } label: {
                Circle().fill(Color(nsColor: NSColor(srgbRed: CGFloat(masks.overlayColor.rgb.r),   // lint:allow (user overlay colour)
                                                     green: CGFloat(masks.overlayColor.rgb.g),
                                                     blue: CGFloat(masks.overlayColor.rgb.b), alpha: 1)))
                    .overlay(Circle().strokeBorder(Theme.hairlineStrong))
                    .frame(width: Theme.Space.m, height: Theme.Space.m)
            }
            .menuStyle(IconMenuStyle())
            .help("Overlay colour (⇧O)")
        }
    }

    private var brushHUD: some View {
        HStack(spacing: Theme.Space.l) {
            hudSlider("Size", value: $masks.brushSize, range: 2...400, format: "%.0f pt")
            hudSlider("Feather", value: $masks.brushFeather, range: 0...100, format: "%.0f")
            hudSlider("Flow", value: $masks.brushFlow, range: 1...100, format: "%.0f")
            Text("⌥ erase  ·  [ ] size").font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary)
        }
    }

    private func hudSlider(_ title: String, value: Binding<Double>, range: ClosedRange<Double>, format: String) -> some View {
        HStack(spacing: Theme.Space.s - Theme.Space.xxs) {
            Text(title).font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
            Slider(value: value, in: range).controlSize(.mini).frame(width: 80)
            Text(String(format: format, value.wrappedValue))
                .font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textPrimary).frame(width: 40, alignment: .leading)
        }
    }
}
