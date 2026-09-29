import SwiftUI
import TesseraCore

// File name differs from the type: `PhotomergeSheet.swift` would share an object file with
// Photo/PhotoMergeSheet.swift on the case-insensitive file system and drop one from the link.

/// File ▸ Automate ▸ Photomerge…: source photos (the library selection and / or files), layout, blending
/// options, and whether to merge into a new document (default) or the current one.
struct PhotomergeSheet: View {
    @Bindable var stack: DocumentStack
    let hasDocument: Bool

    var body: some View {
        let form = stack.photomerge
        VStack(alignment: .leading, spacing: Theme.Space.m) {
            Text("Photomerge").font(Theme.Fonts.title)
            HStack(alignment: .top, spacing: Theme.Space.l) {
                VStack(alignment: .leading, spacing: Theme.Space.s) {
                    Text("Layout").font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                    Picker("Layout", selection: $stack.photomerge.layout) {
                        ForEach(StackAlignLayout.allCases) { l in Text(l.title).tag(l) }
                    }
                    .pickerStyle(.radioGroup)
                    .labelsHidden()
                    .accessibilityIdentifier("photomerge-layout")
                }
                VStack(alignment: .leading, spacing: Theme.Space.s) {
                    Text("Source Files").font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                    List {
                        ForEach(form.sources) { s in
                            HStack {
                                Text(s.title).lineLimit(1)
                                Spacer()
                                Button {
                                    stack.photomerge.sources.removeAll { $0 == s }
                                } label: { Image(systemName: "minus.circle") }
                                .buttonStyle(.borderless)
                                .accessibilityLabel("Remove \(s.title)")
                            }
                        }
                    }
                    .frame(minHeight: 140)
                    .accessibilityIdentifier("photomerge-sources")
                    HStack(spacing: Theme.Space.s) {
                        Button("Add Files…") { stack.addFiles() }
                        Button("Add Library Selection") {
                            stack.photomerge.sources += stack.libraryPhotos()
                            stack.photomerge.removeDuplicates()
                        }
                        .disabled(stack.libraryPhotos().isEmpty)
                    }
                    .controlSize(.small)
                }
            }
            Toggle("Blend Images Together", isOn: .constant(true)).disabled(true)
                .help("Photomerge always blends; use Auto-Align Layers to align without blending.")
            Toggle("Seamless Tones and Colors", isOn: $stack.photomerge.seamlessTones)
            Toggle("Content-Aware Fill Transparent Areas", isOn: $stack.photomerge.contentAwareFill)
                .accessibilityIdentifier("photomerge-fill")
            StackLensToggles(vignette: .constant(false), distortion: .constant(false))
            if hasDocument {
                Toggle("Add to the current document", isOn: $stack.photomerge.intoCurrentDocument)
                    .accessibilityIdentifier("photomerge-into-current")
            }
            if let p = form.problem {
                Text(p).font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
            }
            StackSheetFooter(ok: "OK", disabled: form.problem != nil, cancel: { stack.sheet = nil }) {
                stack.runPhotomerge()
            }
        }
        .padding(Theme.Space.l)
        .frame(width: 560, alignment: .leading)
        .accessibilityIdentifier("photomerge-sheet")
    }
}
