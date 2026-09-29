import SwiftUI
import TesseraCore

/// Edit ▸ Auto-Align Layers… / Auto-Blend Layers… (document mode, WP B5-19). Enabled for two or more
/// top-level, unlocked pixel layers selected in the Layers panel (aligned layers can be blended).
struct StackEditMenuItems: View {
    let stack: DocumentStack

    var body: some View {
        Button("Auto-Align Layers…") { stack.presentAlign() }
            .disabled(!stack.canAlign)
        Button("Auto-Blend Layers…") { stack.presentBlend() }
            .disabled(!stack.canBlend)
    }
}

/// File ▸ Automate ▸ Photomerge… (library selection or files, into a new document).
struct StackFileMenuItems: View {
    let stack: DocumentStack

    var body: some View {
        Menu("Automate") {
            Button("Photomerge…") { stack.presentPhotomerge() }
                .disabled(!stack.canPhotomerge)
        }
    }
}

/// The stack sheets and the busy sheet, attached once to the window's root (they are reachable from the
/// library as well as from document mode).
struct StackSheetsModifier: ViewModifier {
    let model: AppModel
    @Bindable var stack: DocumentStack

    func body(content: Content) -> some View {
        content
            .onAppear { stack.attach(model) }
            .sheet(item: $stack.sheet) { sheet in
                switch sheet {
                case .align: AutoAlignSheet(stack: stack)
                case .blend: AutoBlendSheet(stack: stack)
                case .photomerge: PhotomergeSheet(stack: stack, hasDocument: model.documents.current != nil)
                case .busy(let what): StackBusySheet(title: what)
                }
            }
    }
}

extension View {
    /// Auto-Align / Auto-Blend / Photomerge sheets (WP B5-19).
    func stackSheets(_ model: AppModel) -> some View { modifier(StackSheetsModifier(model: model, stack: .shared)) }
}

/// Indeterminate progress while the engine aligns or blends (no Cancel: it cannot be interrupted yet).
struct StackBusySheet: View {
    let title: String

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.m) {
            HStack(spacing: Theme.Space.m) {
                ProgressView().controlSize(.small)
                Text("\(title)…").font(Theme.Fonts.body)
            }
            Text(StackCommandRules.busyNote).font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
        }
        .padding(Theme.Space.l)
        .frame(width: 360, alignment: .leading)
        .interactiveDismissDisabled()
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("stack-busy")
    }
}

/// Cancel / OK footer of the stack sheets.
struct StackSheetFooter: View {
    let ok: String
    let disabled: Bool
    let cancel: () -> Void
    let action: () -> Void

    var body: some View {
        HStack(spacing: Theme.Space.s) {
            Spacer()
            Button("Cancel", action: cancel)
                .buttonStyle(.theme(.bordered, height: Theme.Height.large)).keyboardShortcut(.cancelAction)
            Button(ok, action: action)
                .buttonStyle(.theme(.primary, height: Theme.Height.large)).keyboardShortcut(.defaultAction)
                .disabled(disabled)
        }
    }
}

/// Lens correction toggles, off and disabled until calibrations are available, with the reason.
struct StackLensToggles: View {
    @Binding var vignette: Bool
    @Binding var distortion: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            Toggle("Vignette Removal", isOn: $vignette)
            Toggle("Geometric Distortion Correction", isOn: $distortion)
            if !StackCommandRules.lensCorrectionAvailable {
                Text(StackCommandRules.lensCorrectionNote)
                    .font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .disabled(!StackCommandRules.lensCorrectionAvailable)
        .accessibilityIdentifier("stack-lens")
    }
}
