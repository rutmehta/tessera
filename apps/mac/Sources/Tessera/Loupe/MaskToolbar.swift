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
        VStack(spacing: 6) {
            if masks.active {
                HStack(spacing: 2) {
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
                        .buttonStyle(.plain)
                        .font(.system(size: 11, weight: .medium))
                        .padding(.horizontal, 8)
                        .help("Leave masking (M)")
                }
                .padding(4)
                .background(bar)
                if masks.tool == .brush { brushHUD.padding(.horizontal, 10).padding(.vertical, 5).background(bar) }
                if masks.list.busy, let p = masks.list.progress.values.first {
                    HStack(spacing: 6) {
                        ProgressView(value: Double(p.fraction)).frame(width: 90).controlSize(.small)
                        Text(p.message).font(.system(size: 10)).foregroundStyle(.secondary)
                    }
                    .padding(.horizontal, 10).padding(.vertical, 4).background(bar)
                }
            } else if ready {
                HStack {
                    Spacer()
                    Button {
                        masks.setActive(true)
                    } label: {
                        Label("Masks", systemImage: "circle.lefthalf.striped.horizontal")
                            .font(.system(size: 11, weight: .medium))
                            .padding(.horizontal, 8).padding(.vertical, 4)
                            .background(bar)
                    }
                    .buttonStyle(.plain)
                    .help("Local adjustments with masks (M)")
                }
                .padding(.top, 28)
            }
            Spacer()
        }
        .padding(10)
        .disabled(!ready)
        .animation(.easeOut(duration: 0.12), value: masks.active)
        .animation(.easeOut(duration: 0.12), value: masks.tool)
    }

    private var bar: some View {
        RoundedRectangle(cornerRadius: 7)
            .fill(Color(nsColor: NSColor(calibratedWhite: 0.1, alpha: 0.88)))
            .overlay(RoundedRectangle(cornerRadius: 7).strokeBorder(Color.white.opacity(0.08)))
    }

    private var separator: some View {
        Rectangle().fill(Color.white.opacity(0.12)).frame(width: 1, height: 18).padding(.horizontal, 4)
    }

    private func toolButton(_ t: MaskTool) -> some View {
        let on = masks.tool == t
        return Button {
            masks.target = nil
            masks.tool = on ? nil : t
        } label: {
            Image(systemName: t.symbol)
                .font(.system(size: 12, weight: .medium))
                .frame(width: 28, height: 24)
                .foregroundStyle(on ? Color.black.opacity(0.85) : Color.primary)
                .background(RoundedRectangle(cornerRadius: 5).fill(on ? Color(nsColor: Theme.accent) : Color.clear))
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .help("\(t.title) — \(t.hint)")
    }

    private func aiButton(_ title: String, _ symbol: String, _ request: AiMaskRequest) -> some View {
        Button {
            masks.runAI(request, title: title, option: NSEvent.modifierFlags.contains(.option))
        } label: {
            HStack(spacing: 3) {
                Image(systemName: symbol).font(.system(size: 11))
                Text(title).font(.system(size: 11))
            }
            .padding(.horizontal, 6)
            .frame(height: 24)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .help("Select the \(title.lowercased()) with the on-device model (⌥ subtracts from the selected mask)")
    }

    private var overlayControls: some View {
        HStack(spacing: 4) {
            Button {
                masks.overlayOn.toggle()
            } label: {
                Image(systemName: masks.overlayOn ? "eye.fill" : "eye.slash")
                    .font(.system(size: 11)).frame(width: 24, height: 24).contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .help("Show the mask overlay (O)")
            Menu {
                ForEach(MaskOverlayColor.allCases, id: \.self) { c in
                    Button(c.rawValue.capitalized) { masks.overlayColor = c; masks.overlayOn = true }
                }
                Divider()
                ForEach([0.3, 0.5, 0.7, 1.0], id: \.self) { o in
                    Button(String(format: "Opacity %.0f%%", o * 100)) { masks.overlayOpacity = o }
                }
            } label: {
                Circle().fill(Color(red: Double(masks.overlayColor.rgb.r), green: Double(masks.overlayColor.rgb.g),
                                    blue: Double(masks.overlayColor.rgb.b)))
                    .overlay(Circle().strokeBorder(Color.white.opacity(0.4)))
                    .frame(width: 12, height: 12)
            }
            .menuStyle(.borderlessButton)
            .menuIndicator(.hidden)
            .fixedSize()
            .help("Overlay colour (⇧O)")
        }
    }

    private var brushHUD: some View {
        HStack(spacing: 14) {
            hudSlider("Size", value: $masks.brushSize, range: 2...400, format: "%.0f pt")
            hudSlider("Feather", value: $masks.brushFeather, range: 0...100, format: "%.0f")
            hudSlider("Flow", value: $masks.brushFlow, range: 1...100, format: "%.0f")
            Text("⌥ erase  [ ] size").font(.system(size: 10)).foregroundStyle(.tertiary)
        }
    }

    private func hudSlider(_ title: String, value: Binding<Double>, range: ClosedRange<Double>, format: String) -> some View {
        HStack(spacing: 5) {
            Text(title).font(.system(size: 10)).foregroundStyle(.secondary)
            Slider(value: value, in: range).controlSize(.mini).frame(width: 80)
            Text(String(format: format, value.wrappedValue))
                .font(.system(size: 10).monospacedDigit()).foregroundStyle(.secondary).frame(width: 40, alignment: .leading)
        }
    }
}
