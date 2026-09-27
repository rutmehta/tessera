import SwiftUI
import TesseraCore

/// The Channels sheets (WP B5-08): Save Selection, Load Selection, Channel Options, New Spot Channel.
/// They hang off the document view next to the tools' sheets.
struct ChannelSheetsModifier: ViewModifier {
    @Bindable var channels: DocumentChannels

    func body(content: Content) -> some View {
        content.sheet(item: $channels.sheet) { sheet in
            switch sheet {
            case .save: SaveSelectionChannelSheet(channels: channels)
            case .load: LoadSelectionChannelSheet(channels: channels)
            case .options(let id): ChannelOptionsSheet(channels: channels, id: id)
            case .newSpot: NewSpotChannelSheet(channels: channels)
            }
        }
    }
}

/// A label-column form row.
private struct FormRow<Content: View>: View {
    let label: String
    @ViewBuilder var content: Content
    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: Theme.Space.s) {
            Text(label).font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                .frame(width: Theme.Width.label, alignment: .leading)
            content
            Spacer(minLength: 0)
        }
    }
}

/// A saved channel pop-up (duplicate names are told apart by position).
private struct ChannelMenu: View {
    let records: [SavedChannel]
    @Binding var selection: UInt64?
    var newTitle: String?
    let identifier: String

    var body: some View {
        Picker("Channel", selection: $selection) {
            if let newTitle { Text(newTitle).tag(UInt64?.none) }
            ForEach(records) { r in
                let dup = records.filter { $0.name == r.name }.count > 1
                Text(dup ? "\(r.name) (\(r.index + 1))" : r.name).tag(Optional(r.id))
            }
        }
        .labelsHidden()
        .controlSize(.small)
        .frame(maxWidth: Theme.Width.inspectorMin)
        .accessibilityIdentifier(identifier)
    }
}

/// Operation radio buttons.
private struct OperationPicker: View {
    let operations: [SelectionCombine]
    let title: (SelectionCombine) -> String
    @Binding var selection: SelectionCombine
    let identifier: String

    var body: some View {
        Picker("Operation", selection: $selection) {
            ForEach(operations, id: \.self) { op in Text(title(op)).tag(op) }
        }
        .pickerStyle(.radioGroup)
        .labelsHidden()
        .font(Theme.Fonts.caption)
        .accessibilityIdentifier(identifier)
    }
}

@MainActor private func footerButtons(ok: String = "OK", disabled: Bool = false, dismiss: DismissAction,
                           action: @escaping @MainActor () -> Void) -> some View {
    HStack(spacing: Theme.Space.s) {
        Button("Cancel") { dismiss() }.buttonStyle(.theme(.bordered, height: Theme.Height.large)).keyboardShortcut(.cancelAction)
        Button(ok) { action(); dismiss() }
            .buttonStyle(.theme(.primary, height: Theme.Height.large)).keyboardShortcut(.defaultAction)
            .disabled(disabled)
            .accessibilityIdentifier("document.channels.sheet.ok")
    }
}

/// Select ▸ Save Selection…: into a new channel (name) or an existing one (replace / add / subtract / intersect).
struct SaveSelectionChannelSheet: View {
    let channels: DocumentChannels
    @Environment(\.dismiss) private var dismiss
    @State private var form = SaveSelectionForm(name: "Alpha 1")

    var body: some View {
        let records = channels.records.filter { $0.id != channels.document.flatMap { channels.quickMask[$0.id] } }
        SheetScaffold(title: "Save Selection", subtitle: "Saved with the document as a channel") {
            EmptyView()
        } content: {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                FormRow(label: "Channel") {
                    ChannelMenu(records: records, selection: $form.destination, newTitle: "New",
                                identifier: "document.channels.save.channel")
                }
                FormRow(label: "Name") {
                    TextField("Name", text: $form.name).textFieldStyle(.roundedBorder).controlSize(.small)
                        .disabled(form.destination != nil)
                        .accessibilityIdentifier("document.channels.save.name")
                }
                FormRow(label: "Operation") {
                    OperationPicker(operations: form.operations,
                                    title: { SaveSelectionForm.title($0, newChannel: form.destination == nil) },
                                    selection: $form.operation, identifier: "document.channels.save.operation")
                }
            }
            .padding(Theme.Space.l)
        } leading: {
            EmptyView()
        } actions: {
            footerButtons(disabled: form.request == nil || channels.document?.marquee == nil, dismiss: dismiss) {
                channels.saveSelection(form)
            }
        }
        .frame(width: Theme.Width.inspectorMax + Theme.Space.xxl, height: Theme.Width.sidebarMax)
        .onAppear {
            form.name = ChannelsPanelModel.nextName("Alpha", existing: channels.records)
        }
        .onChange(of: form.destination) { if $1 == nil { form.operation = .replace } }
    }
}

/// Select ▸ Load Selection…: a saved channel, inverted or not, combined with the selection.
struct LoadSelectionChannelSheet: View {
    let channels: DocumentChannels
    @Environment(\.dismiss) private var dismiss
    @State private var form = LoadSelectionForm(channel: nil, hasSelection: false)

    var body: some View {
        SheetScaffold(title: "Load Selection", subtitle: "From a saved channel") {
            EmptyView()
        } content: {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                if channels.records.isEmpty {
                    Hint("This document has no saved channels. Save a selection first (Select ▸ Save Selection…).")
                } else {
                    FormRow(label: "Channel") {
                        ChannelMenu(records: channels.records, selection: $form.channel, identifier: "document.channels.load.channel")
                    }
                    FormRow(label: "") {
                        Toggle("Invert", isOn: $form.invert).controlSize(.small).font(Theme.Fonts.caption)
                            .accessibilityIdentifier("document.channels.load.invert")
                    }
                    FormRow(label: "Operation") {
                        OperationPicker(operations: form.operations, title: \.title, selection: $form.operation,
                                        identifier: "document.channels.load.operation")
                    }
                }
            }
            .padding(Theme.Space.l)
        } leading: {
            EmptyView()
        } actions: {
            footerButtons(disabled: form.request == nil, dismiss: dismiss) { channels.loadSelection(form) }
        }
        .frame(width: Theme.Width.inspectorMax + Theme.Space.xxl, height: Theme.Width.sidebarMax)
        .onAppear {
            form = LoadSelectionForm(channel: channels.selectedChannel ?? channels.records.first?.id,
                                     hasSelection: channels.document?.marquee != nil)
        }
    }
}

/// Converts between the colour well and straight sRGB channel values.
private func swatch(_ c: ToolColor) -> Color {
    Color(.sRGB, red: Double(c.r), green: Double(c.g), blue: Double(c.b)) // lint:allow (user-chosen channel colour)
}

private func toolColor(_ c: Color) -> ToolColor {
    let n = NSColor(c).usingColorSpace(.sRGB) ?? NSColor(c)
    return ToolColor(r: Float(n.redComponent), g: Float(n.greenComponent), b: Float(n.blueComponent))
}

private struct PercentField: View {
    let title: String
    @Binding var value: Double
    let identifier: String
    var body: some View {
        OptionField(title: title, value: $value, range: 0...100, unit: "%", identifier: identifier)
    }
}

/// The note every spot colour control carries.
private struct SpotNote: View {
    var body: some View {
        Hint("Spot colour and solidity are preview information for the Channels overlay and the PSD ink list. They never change the RGB image or its export.")
            .accessibilityIdentifier("document.channels.spotNote")
    }
}

/// Channel Options: name, kind, colour and opacity / solidity, and for alpha channels which areas the
/// overlay colour marks.
struct ChannelOptionsSheet: View {
    let channels: DocumentChannels
    let id: UInt64
    @Environment(\.dismiss) private var dismiss
    @State private var name = ""
    @State private var kind: SavedChannelKind = .alpha
    @State private var color = Color.clear
    @State private var opacity = 50.0
    @State private var indicatesSelected = false

    var body: some View {
        SheetScaffold(title: "Channel Options", subtitle: channels.records.first { $0.id == id }?.name) {
            EmptyView()
        } content: {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                FormRow(label: "Name") {
                    TextField("Name", text: $name).textFieldStyle(.roundedBorder).controlSize(.small)
                        .accessibilityIdentifier("document.channels.options.name")
                }
                FormRow(label: "Kind") {
                    SegmentedPicker(selection: $kind, segments: [
                        .init(value: .alpha, title: "Alpha", help: "A saved selection or mask"),
                        .init(value: .spot, title: "Spot Color", help: "A spot ink plane (preview only)"),
                    ], height: Theme.Height.regular, fill: false)
                    .accessibilityIdentifier("document.channels.options.kind")
                }
                if kind == .alpha {
                    FormRow(label: "Indicates") {
                        Picker("Color Indicates", selection: $indicatesSelected) {
                            Text("Masked Areas").tag(false)
                            Text("Selected Areas").tag(true)
                        }
                        .pickerStyle(.radioGroup).labelsHidden().font(Theme.Fonts.caption)
                        .accessibilityIdentifier("document.channels.options.indicates")
                    }
                }
                FormRow(label: "Color") {
                    ColorPicker("", selection: $color, supportsOpacity: false).labelsHidden()
                        .accessibilityIdentifier("document.channels.options.color")
                    PercentField(title: kind == .spot ? "Solidity" : "Opacity", value: $opacity,
                                 identifier: "document.channels.options.opacity")
                }
                if kind == .spot {
                    SpotNote()
                } else {
                    Hint("The overlay colour and opacity of alpha channels are a preview preference of this session.")
                }
            }
            .padding(Theme.Space.l)
        } leading: {
            EmptyView()
        } actions: {
            footerButtons(disabled: name.trimmingCharacters(in: .whitespaces).isEmpty, dismiss: dismiss) {
                channels.applyOptions(id, name: name, kind: kind, color: toolColor(color), opacity: Float(opacity / 100),
                                      indicatesSelected: indicatesSelected)
            }
        }
        .frame(width: Theme.Width.inspectorMax + Theme.Space.xxl, height: Theme.Width.sidebarMax + Theme.Space.xxl)
        .onAppear {
            guard let doc = channels.document, let r = channels.records.first(where: { $0.id == id }) else { return }
            name = r.name
            kind = r.kind
            if r.kind == .spot {
                color = swatch(r.color)
                opacity = Double(r.opacity * 100)
            } else {
                let st = channels.style(doc, id)
                color = swatch(st.color)
                opacity = Double(st.opacity * 100)
                indicatesSelected = st.indicatesSelected
            }
        }
    }
}

/// New Spot Channel…: name, ink colour, solidity, from the selection or empty.
struct NewSpotChannelSheet: View {
    let channels: DocumentChannels
    @Environment(\.dismiss) private var dismiss
    @State private var name = "Spot Color 1"
    @State private var color = swatch(ToolColor(r: 0, g: 0.6, b: 1))
    @State private var solidity = 0.0
    @State private var fromSelection = true

    var body: some View {
        let hasSelection = channels.document?.marquee != nil
        SheetScaffold(title: "New Spot Channel", subtitle: nil) {
            EmptyView()
        } content: {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                FormRow(label: "Name") {
                    TextField("Name", text: $name).textFieldStyle(.roundedBorder).controlSize(.small)
                        .accessibilityIdentifier("document.channels.spot.name")
                }
                FormRow(label: "Ink") {
                    ColorPicker("", selection: $color, supportsOpacity: false).labelsHidden()
                        .accessibilityIdentifier("document.channels.spot.color")
                    PercentField(title: "Solidity", value: $solidity, identifier: "document.channels.spot.solidity")
                }
                FormRow(label: "") {
                    Toggle("From the selection", isOn: $fromSelection).controlSize(.small).font(Theme.Fonts.caption)
                        .disabled(!hasSelection)
                        .accessibilityIdentifier("document.channels.spot.fromSelection")
                }
                SpotNote()
            }
            .padding(Theme.Space.l)
        } leading: {
            EmptyView()
        } actions: {
            footerButtons(disabled: name.trimmingCharacters(in: .whitespaces).isEmpty, dismiss: dismiss) {
                channels.newSpot(name: name, color: toolColor(color), solidity: Float(solidity / 100),
                                 fromSelection: fromSelection && hasSelection)
            }
        }
        .frame(width: Theme.Width.inspectorMax + Theme.Space.xxl, height: Theme.Width.sidebarMax)
        .onAppear {
            name = ChannelsPanelModel.nextName("Spot Color", existing: channels.records)
            fromSelection = hasSelection
        }
    }
}
