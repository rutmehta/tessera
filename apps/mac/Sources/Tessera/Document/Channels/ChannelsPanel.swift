import AppKit
import SwiftUI
import TesseraCore

/// Channels panel (WP B5-08, DESIGN.md §10): the RGB composite and its Red / Green / Blue rows
/// (read-only; eyes drive the preview), then the document's saved alpha and spot channels with a
/// thumbnail, the name (double-click renames), the eye and, for spot channels, the ink swatch. Click
/// highlights a row, ⌘-click loads it as the selection (⇧ adds, ⌥ subtracts). Footer: load as
/// selection, save selection as channel, new channel / spot channel, delete.
struct ChannelsPanel: View {
    let document: DocumentController
    @Bindable var channels: DocumentChannels

    private var rowHeight: CGFloat { Theme.Height.sectionHeader }

    var body: some View {
        let rows = channels.rows(document)
        VStack(spacing: 0) {
            ScrollView {
                VStack(spacing: 0) {
                    ForEach(rows) { row in
                        ChannelRowView(document: document, channels: channels, row: row)
                            .frame(height: rowHeight)
                    }
                }
            }
            .scrollIndicators(.automatic)
            // B5-16 (H3): the Channels tab gives the list the column's height (at least four rows).
            .frame(minHeight: rowHeight * CGFloat(min(rows.count, 4)), maxHeight: .infinity, alignment: .top)
            .accessibilityIdentifier("document.channels.list")
            Hairline()
            footer
                .inspectorProbe("channelsFooter")
        }
        .task(id: RefreshKey(doc: document.id, epoch: document.info.epoch, head: document.info.historyHead)) {
            channels.reload(document)
        }
    }

    private struct RefreshKey: Equatable {
        let doc: String
        let epoch: UInt64
        let head: DocHistoryID
    }

    private var footer: some View {
        let selected = channels.selectedChannel
        let hasSelection = document.marquee != nil
        return HStack(spacing: Theme.Space.xxs) {
            IconButton(symbol: "circle.dashed", help: "Load the highlighted channel as a selection (⌘-click a channel)") {
                if let selected { channels.load(selected) }
            }
            .disabled(selected == nil)
            .accessibilityIdentifier("document.channels.load")
            IconButton(symbol: "square.and.arrow.down", help: "Save the selection as a new channel") {
                channels.quickSaveSelection()
            }
            .disabled(!hasSelection)
            .accessibilityIdentifier("document.channels.save")
            IconButton(symbol: "square.dashed", help: "Quick Mask mode (Q)", on: channels.isQuickMask(document)) {
                channels.toggleQuickMask()
            }
            .accessibilityIdentifier("document.channels.quickMask")
            Spacer(minLength: 0)
            Menu {
                Button("New Channel") { channels.newChannel() }
                Button("New Spot Channel…") { channels.sheet = .newSpot }
            } label: { Image(systemName: "plus.square") }
                .menuStyle(IconMenuStyle())
                .help("New alpha or spot channel")
                .accessibilityIdentifier("document.channels.add")
            IconButton(symbol: "trash", help: "Delete the highlighted channel") {
                if let selected { channels.delete(selected) }
            }
            .disabled(selected == nil)
            .accessibilityIdentifier("document.channels.delete")
        }
        .padding(.horizontal, Theme.Space.s)
        .frame(height: Theme.Height.large)
    }
}

private struct ChannelRowView: View {
    let document: DocumentController
    @Bindable var channels: DocumentChannels
    let row: ChannelRow
    @State private var draft = ""
    @FocusState private var editing: Bool

    private var highlighted: Bool { row.channelID != nil && row.channelID == channels.selectedChannel }

    var body: some View {
        HStack(spacing: Theme.Space.s) {
            Button { channels.toggleVisible(row) } label: {
                Image(systemName: row.visible ? "eye" : "eye.slash")
                    .font(Theme.Fonts.iconSmall)
                    .foregroundStyle(row.visible ? Theme.textSecondary : Theme.textTertiary)
                    .frame(width: Theme.Height.small, height: Theme.Height.small)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .help(row.visible ? "Hide in the preview" : "Show in the preview")
            .accessibilityLabel(row.visible ? "Hide \(row.title)" : "Show \(row.title)")
            .accessibilityIdentifier("document.channels.\(row.id).eye")
            thumbnail
            if channels.renaming == row.channelID, row.channelID != nil {
                TextField("Name", text: $draft)
                    .textFieldStyle(.roundedBorder).controlSize(.small).font(Theme.Fonts.label)
                    .focused($editing)
                    .onSubmit { commitRename() }
                    .onExitCommand { channels.renaming = nil }
                    .onAppear { draft = row.title; editing = true }
                    .onChange(of: editing) { if !$1 { commitRename() } }
                    .accessibilityIdentifier("document.channels.\(row.id).rename")
            } else {
                Text(row.title)
                    .font(Theme.Fonts.label)
                    .foregroundStyle(row.visible || row.editable ? Theme.textPrimary : Theme.textSecondary)
                    .lineLimit(1).truncationMode(.middle)
                    .onTapGesture(count: 2) { if row.editable { channels.renaming = row.channelID } }
            }
            if row.isQuickMask { Chip(text: "Temporary", color: Theme.textSecondary, style: .outlined) }
            Spacer(minLength: 0)
            if row.kind == .spot, let c = row.color {
                RoundedRectangle(cornerRadius: Theme.Radius.chip)
                    .fill(swatchColor(c))
                    .overlay(RoundedRectangle(cornerRadius: Theme.Radius.chip)
                        .strokeBorder(Theme.hairlineStrong, lineWidth: Theme.Space.hairline))
                    .frame(width: Theme.Space.m, height: Theme.Space.m)
                    .help("Spot ink colour (preview only)")
                    .accessibilityIdentifier("document.channels.\(row.id).swatch")
            }
            kindGlyph
        }
        .padding(.horizontal, Theme.Space.s)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(RoundedRectangle(cornerRadius: Theme.Radius.control).fill(highlighted ? Theme.accentSubtle : Theme.clear))
        .contentShape(Rectangle())
        .onTapGesture { click() }
        .contextMenu { if row.editable { menu } }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(row.title)
        .accessibilityAddTraits(highlighted ? .isSelected : [])
        .accessibilityIdentifier("document.channels.\(row.id)")
    }

    @ViewBuilder private var thumbnail: some View {
        let size = Theme.Height.regular
        Group {
            if let image = row.editable ? channels.thumbnail(document, row) : channels.componentThumbnail(document, row) {
                Image(nsImage: image).resizable().interpolation(.medium).aspectRatio(contentMode: .fit)
            } else {
                Theme.well
            }
        }
        .frame(width: size, height: size)
        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.chip))
        .overlay(RoundedRectangle(cornerRadius: Theme.Radius.chip).strokeBorder(Theme.hairlineStrong, lineWidth: Theme.Space.hairline))
    }

    @ViewBuilder private var kindGlyph: some View {
        switch row.kind {
        case .composite, .component:
            Image(systemName: "lock").font(Theme.Fonts.iconSmall).foregroundStyle(Theme.textTertiary)
                .help("Colour channels are read-only")
        case .alpha, .spot:
            EmptyView()
        }
    }

    @ViewBuilder private var menu: some View {
        if let id = row.channelID {
            Button("Load as Selection") { channels.load(id) }
            Button("Add to Selection") { channels.load(id, op: .add) }
            Button("Subtract from Selection") { channels.load(id, op: .subtract) }
            Button("Intersect with Selection") { channels.load(id, op: .intersect) }
            Divider()
            Button("Duplicate Channel") { channels.duplicate(id) }
            Button("Rename…") { channels.renaming = id }
            Button("Channel Options…") { channels.selectedChannel = id; channels.sheet = .options(id) }
            Divider()
            Button("Delete Channel") { channels.delete(id) }
        }
    }

    /// The highlight always shows the paint target (B5-17d): RGB / colour rows clear it.
    private func click() {
        let mods = NSEvent.modifierFlags.intersection(.deviceIndependentFlagsMask)
        guard let c = ChannelsPanelModel.click(row, command: mods.contains(.command)) else { return }
        if c.retarget { DocumentTools.shared.targetChannel(c.paintTarget, in: document) } // B5-17c
        channels.selectedChannel = c.highlight
        guard c.load, let id = c.highlight else { return }
        let op: SelectionCombine = mods.contains(.shift) && mods.contains(.option) ? .intersect
            : mods.contains(.shift) ? .add : mods.contains(.option) ? .subtract : .replace
        channels.load(id, op: op)
    }

    private func commitRename() {
        guard let id = row.channelID, channels.renaming == id else { return }
        channels.rename(id, to: draft)
    }

    private func swatchColor(_ c: ToolColor) -> Color {
        Color(.sRGB, red: Double(c.r), green: Double(c.g), blue: Double(c.b)) // lint:allow (user-chosen spot ink)
    }
}
