import SwiftUI
import TesseraCore

/// Edit ▸ Auto-Blend Layers…: Panorama or Stack Images, seamless tones, Content-Aware Fill.
struct AutoBlendSheet: View {
    @Bindable var stack: DocumentStack

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.m) {
            Text("Auto-Blend Layers").font(Theme.Fonts.title)
            Picker("Blend Method", selection: $stack.blend.method) {
                ForEach(StackBlendMethod.allCases) { m in Text(m.title).tag(m) }
            }
            .pickerStyle(.radioGroup)
            .accessibilityIdentifier("stack-blend-method")
            Text(stack.blend.method.help).font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
            Toggle("Seamless Tones and Colors", isOn: $stack.blend.seamlessTones)
                .accessibilityIdentifier("stack-blend-tones")
            Toggle("Content-Aware Fill Transparent Areas", isOn: $stack.blend.contentAwareFill)
                .accessibilityIdentifier("stack-blend-fill")
            Text("Each layer gets an editable mask; one undo removes the whole blend.")
                .font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
            StackSheetFooter(ok: "OK", disabled: false, cancel: { stack.sheet = nil }) { stack.runBlend() }
        }
        .padding(Theme.Space.l)
        .frame(width: 420, alignment: .leading)
        .accessibilityIdentifier("stack-blend-sheet")
    }
}
