import SwiftUI
import TesseraCore

/// Edit ▸ Auto-Align Layers…: projection, reference layer and (disabled) lens corrections.
struct AutoAlignSheet: View {
    @Bindable var stack: DocumentStack

    var body: some View {
        let names = stack.selectedNames()
        VStack(alignment: .leading, spacing: Theme.Space.m) {
            Text("Auto-Align Layers").font(Theme.Fonts.title)
            Text("Aligns \(names.count) layers; the canvas grows to fit them. Each layer keeps its original "
                 + "pixels and an editable transform.")
                .font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
            Picker("Projection", selection: $stack.align.layout) {
                ForEach(StackAlignLayout.allCases) { l in Text(l.title).tag(l) }
            }
            .pickerStyle(.radioGroup)
            .accessibilityIdentifier("stack-align-layout")
            Text(stack.align.layout.help).font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
            Picker("Reference", selection: $stack.align.referenceIndex) {
                ForEach(Array(names.enumerated()), id: \.offset) { i, n in Text(n).tag(UInt32(i)) }
            }
            .fixedSize()
            .accessibilityIdentifier("stack-align-reference")
            StackLensToggles(vignette: $stack.align.vignetteRemoval, distortion: $stack.align.geometricDistortion)
            StackSheetFooter(ok: "OK", disabled: names.count < 2, cancel: { stack.sheet = nil }) { stack.runAlign() }
        }
        .padding(Theme.Space.l)
        .frame(width: 420, alignment: .leading)
        .accessibilityIdentifier("stack-align-sheet")
    }
}
