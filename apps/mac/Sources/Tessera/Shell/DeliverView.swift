import AppKit
import SwiftUI
import TesseraCore

/// Deliver's canvas (redesign SPEC §5, WP UX-15): the What · How · Where column on the left (Tab
/// hides it, like every mode's panel) and, on the right, the files that will be written as a grid
/// with their output sizes, a size estimate, a progress bar while the export runs and one status
/// line. The Export button sits beside the agent field (`DeliverExportButton`, AgentRow).
struct DeliverView: View {
    let model: AppModel

    var body: some View {
        HStack(spacing: 0) {
            if model.showInspector {
                DeliverColumn(model: model)
                    .transition(Theme.Motion.transition(from: .leading))
            }
            DeliverPreview(model: model)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Theme.canvas)
        .animation(Theme.Motion.appear, value: model.showInspector)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("library.deliver.workspace")
    }
}

/// What (Keeps n · Selection n · Whole shoot n) · How (the four rows and More…) · Where (the
/// folder and the first file's name).
private struct DeliverColumn: View {
    let model: AppModel

    var body: some View {
        let _ = model.decisionRevision
        let exporter = model.exporter
        VStack(alignment: .leading, spacing: Theme.Space.xl) {
            section("What", detail: whatDetail) {
                ForEach(DeliverWhat.allCases) { what in
                    DeliverRow(title: what.title, detail: count(model.deliverItems(what).count), selected: model.deliverWhat == what) {
                        model.setDeliverWhat(what)
                    }
                    .accessibilityIdentifier("library.deliver.what.\(what.rawValue)")
                }
            }
            section("How", detail: "") {
                ForEach(DeliverHow.allCases) { how in
                    DeliverRow(title: how.title, detail: how.subtitle, selected: model.deliverHow == how) {
                        model.setDeliverHow(how)
                    }
                    .accessibilityIdentifier("library.deliver.how.\(how.rawValue)")
                }
                Button("More…") { model.presentDeliverMore() }
                    .buttonStyle(.theme(.borderless, height: Theme.Height.large))
                    .foregroundStyle(Theme.accent)
                    .disabled(model.deliverItems.isEmpty || exporter.isRunning)
                    .help("Every export option: format, quality, colour space, watermark, naming")
                    .accessibilityIdentifier("library.deliver.how.more")
            }
            section("Where", detail: "") {
                whereRow(folderName(exporter.settings.destination), help: exporter.settings.destination) {
                    Button("change") { exporter.chooseDestination(in: NSApp.keyWindow) }
                        .buttonStyle(.plain)
                        .font(Theme.Fonts.label)
                        .foregroundStyle(Theme.accent)
                        .disabled(exporter.isRunning)
                        .help("Choose the export folder")
                        .accessibilityIdentifier("library.deliver.where.change")
                }
                .accessibilityIdentifier("library.deliver.where.folder")
                whereRow("Name", help: "The first file to be written; the naming template is in More…") {
                    Text(model.deliverNamingExample)
                        .font(Theme.Fonts.label).foregroundStyle(Theme.textSecondary)
                        .lineLimit(1).truncationMode(.middle)
                        .accessibilityIdentifier("library.deliver.where.name")
                }
            }
            Spacer(minLength: 0)
        }
        .padding(.horizontal, Theme.Space.l)
        .padding(.top, Theme.Space.l)
        .padding(.bottom, Theme.Space.m)
        .frame(width: Theme.Width.deliverColumn)
        .frame(maxHeight: .infinity, alignment: .top)
        // UX-16: the prototype's column has no fill, only the hairline on its right.
        .background(Theme.canvas)
        .overlay(alignment: .trailing) { Hairline(vertical: true) }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Deliver")
        .accessibilityIdentifier("library.deliver.column")
    }

    private var whatDetail: String {
        let n = model.deliverItems.count
        return "\(n.formatted()) \(model.deliverWhat == .keeps ? "keep" : "photo")\(n == 1 ? "" : "s")"
    }

    private func count(_ n: Int) -> String { "\(n.formatted()) photo\(n == 1 ? "" : "s")" }

    /// "Desktop / World Cup export": the folder's last two components.
    private func folderName(_ path: String) -> String {
        guard !path.isEmpty else { return "No folder chosen" }
        let parts = URL(fileURLWithPath: path).pathComponents.suffix(2)
        return parts.joined(separator: " / ")
    }

    private func section<Rows: View>(_ title: String, detail: String, @ViewBuilder rows: () -> Rows) -> some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            HStack(alignment: .firstTextBaseline) {
                Text(title).font(Theme.Fonts.title).foregroundStyle(Theme.textPrimary)
                Spacer()
                Text(detail).font(Theme.Fonts.labelNumeric).foregroundStyle(Theme.textTertiary)
            }
            VStack(alignment: .leading, spacing: Theme.Space.xs) { rows() }
        }
    }

    private func whereRow<Trailing: View>(_ title: String, help: String, @ViewBuilder trailing: () -> Trailing) -> some View {
        VStack(spacing: 0) {
            HStack(spacing: Theme.Space.m) {
                Text(title).font(Theme.Fonts.body).foregroundStyle(Theme.textPrimary)
                    .lineLimit(1).truncationMode(.middle)
                Spacer(minLength: Theme.Space.m)
                trailing()
            }
            .frame(height: Theme.Height.deliverRow)
            Hairline()
        }
        .help(help)
    }
}

/// One row of What or How: the title left, the detail right, the chosen one on `raised` with a
/// strong hairline.
private struct DeliverRow: View {
    let title: String
    let detail: String
    let selected: Bool
    let action: () -> Void
    @State private var hovering = false

    var body: some View {
        Button(action: action) {
            HStack(spacing: Theme.Space.m) {
                Text(title).font(Theme.Fonts.body).foregroundStyle(Theme.textPrimary)
                Spacer(minLength: Theme.Space.m)
                Text(detail).font(Theme.Fonts.labelNumeric).foregroundStyle(Theme.textTertiary)
                    .lineLimit(1).truncationMode(.tail)
            }
            .padding(.horizontal, Theme.Space.m)
            .frame(height: Theme.Height.deliverRow)
            .frame(maxWidth: .infinity)
            .background(RoundedRectangle(cornerRadius: Theme.Radius.card).fill(selected ? Theme.raised : hovering ? Theme.hover : Theme.clear))
            .overlay(RoundedRectangle(cornerRadius: Theme.Radius.card)
                .strokeBorder(selected ? Theme.hairlineStrong : Theme.clear, lineWidth: Theme.Space.hairline))
            .contentShape(RoundedRectangle(cornerRadius: Theme.Radius.card))
        }
        .buttonStyle(.plain)
        .onHover { hovering = $0 }
        .accessibilityAddTraits(selected ? .isSelected : [])
        .accessibilityLabel("\(title), \(detail)")
    }
}

/// The header ("4 photos · Web JPEG · 2048 px · sRGB" and "about 3.1 MB"), the grid of files with
/// their output sizes, the progress bar and the status line.
private struct DeliverPreview: View {
    let model: AppModel

    var body: some View {
        let _ = model.decisionRevision
        let items = model.deliverItems
        VStack(alignment: .leading, spacing: Theme.Space.m) {
            HStack(alignment: .firstTextBaseline) {
                Text(model.deliverHeadline).font(Theme.Fonts.title).foregroundStyle(Theme.textPrimary)
                    .lineLimit(1).truncationMode(.tail)
                    .accessibilityIdentifier("library.deliver.headline")
                Spacer()
                if let estimate = model.deliverEstimate {
                    Text(estimate).font(Theme.Fonts.labelNumeric).foregroundStyle(Theme.textTertiary)
                        .help("A typical size for these settings; the files may differ")
                        .accessibilityIdentifier("library.deliver.estimate")
                }
            }
            .frame(height: Theme.Height.large)
            if items.isEmpty {
                Text(model.deliverEmptyMessage)
                    .font(Theme.Fonts.label).foregroundStyle(Theme.textTertiary)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .accessibilityIdentifier("library.deliver.empty")
            } else {
                DeliverGrid(model: model, items: items)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
            progress
            status
        }
        .padding(.top, Theme.Space.m)
        .padding(.horizontal, Theme.Space.xl)
        // Clear of the agent row (the field and the button) at the bottom of the canvas, and of the
        // toast band above it (UX-15 item 20).
        .padding(.bottom, Theme.Height.agentField + Theme.Space.l + Theme.Space.l + Theme.Height.toastBand)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    /// A 4 pt bar: the run's fraction while exporting, full once done, empty otherwise.
    private var progress: some View {
        let exporter = model.exporter
        let fraction: CGFloat = {
            if let p = exporter.progress, p.total > 0 { return CGFloat(p.done) / CGFloat(p.total) }
            return model.deliverResult != nil ? 1 : 0
        }()
        return GeometryReader { geo in
            ZStack(alignment: .leading) {
                RoundedRectangle(cornerRadius: Theme.Space.xxs).fill(Theme.track)
                RoundedRectangle(cornerRadius: Theme.Space.xxs).fill(Theme.accent)
                    .frame(width: geo.size.width * fraction)
                    .animation(Theme.Motion.appear, value: fraction)
            }
        }
        .frame(height: Theme.Space.xs)
        .accessibilityElement()
        .accessibilityLabel(exporter.progress.map { "Exporting \($0.done) of \($0.total)" } ?? "Export progress")
        .accessibilityIdentifier("library.deliver.progress")
    }

    /// "Exporting · 3 of 8 · name" while it runs; "Done · n files · Reveal" after; the exporter's
    /// problem; else what the agent field can do.
    private var status: some View {
        let exporter = model.exporter
        return HStack(spacing: Theme.Space.xs) {
            if let p = exporter.progress {
                Text("Exporting · \(p.done.formatted()) of \(p.total.formatted())" + (p.current.isEmpty ? "" : " · \(p.current)"))
                    .lineLimit(1).truncationMode(.middle)
            } else if exporter.starting {
                Text("Exporting…")
            } else if let result = model.deliverResult {
                let n = result.files.count
                Text("Done · \(n.formatted()) file\(n == 1 ? "" : "s")" + (result.failed > 0 ? " · \(result.failed.formatted()) failed" : "") + " ·")
                Button("Reveal") { model.revealDeliverResult() }
                    .buttonStyle(.plain)
                    .foregroundStyle(Theme.accent)
                    .help("Show the files in Finder")
                    .accessibilityIdentifier("library.deliver.reveal")
            } else if let error = exporter.error {
                Text(error).foregroundStyle(Theme.reject).lineLimit(2)
            } else {
                Text("Ready. The agent can also do this: “export the keeps for Instagram”.")
            }
        }
        .font(Theme.Fonts.label)
        .foregroundStyle(Theme.textTertiary)
        .frame(height: Theme.Height.large)
        .shellProbe("deliverStatus")
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("library.deliver.status")
    }
}

/// The cells, top-aligned; in a scroll view only when they do not all fit (a shoot of hundreds),
/// so a small set has no scroller and no scroll ground.
private struct DeliverGrid: View {
    let model: AppModel
    let items: [PhotoItem]

    var body: some View {
        GeometryReader { geo in
            let column = Theme.Width.deliverCell + Theme.Space.m
            let row = Theme.Height.deliverCell + Theme.Space.xs + Theme.Height.chip + Theme.Space.m
            let columns = max(Int((geo.size.width + Theme.Space.m) / column), 1)
            let rows = max(Int((geo.size.height + Theme.Space.m) / row), 1)
            if items.count <= columns * rows {
                grid.frame(width: geo.size.width, height: geo.size.height, alignment: .topLeading)
            } else {
                ScrollView { grid }.frame(width: geo.size.width, height: geo.size.height)
            }
        }
    }

    private var grid: some View {
        LazyVGrid(columns: [GridItem(.adaptive(minimum: Theme.Width.deliverCell), spacing: Theme.Space.m, alignment: .leading)],
                  alignment: .leading, spacing: Theme.Space.m) {
            ForEach(items) { item in
                DeliverCell(model: model, item: item, size: model.deliverOutputSize(of: item))
            }
        }
    }
}

/// One file: the thumbnail (aspect fit), the name and the output size under it.
private struct DeliverCell: View {
    let model: AppModel
    let item: PhotoItem
    let size: String?

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            PreviewImageView(model: model, item: item, fill: false)
                .frame(width: Theme.Width.deliverCell, height: Theme.Height.deliverCell)
                .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.chip))
            HStack(spacing: Theme.Space.s) {
                Text((item.name as NSString).deletingPathExtension)
                    .font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                    .lineLimit(1).truncationMode(.middle)
                Spacer(minLength: Theme.Space.xs)
                Text(size ?? "–").font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textTertiary)
                    .lineLimit(1)
            }
            .frame(width: Theme.Width.deliverCell)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("\(item.name)\(size.map { ", \($0)" } ?? "")")
        .accessibilityIdentifier("library.deliver.cell.\(item.id)")
    }
}

/// "Export n", the one primary action of Deliver, 48 pt like the agent field beside it.
struct DeliverExportButton: View {
    let model: AppModel
    @State private var hovering = false

    var body: some View {
        let _ = model.decisionRevision
        let n = model.deliverItems.count
        let exporter = model.exporter
        let enabled = n > 0 && model.isEngineBacked && !exporter.isRunning
        Button { model.runDeliver() } label: {
            Text(exporter.isRunning ? "Exporting…" : model.deliverHow == .print ? "Print \(n.formatted())" : "Export \(n.formatted())")
                .font(Theme.Fonts.bodyMedium)
                .foregroundStyle(Theme.textOnAccent)
                .lineLimit(1)
                .padding(.horizontal, Theme.Space.xl)
                .frame(height: Theme.Height.agentField)
                .background(RoundedRectangle(cornerRadius: Theme.Radius.agentField)
                    .fill(hovering && enabled ? Theme.accent.opacity(0.9) : Theme.accent))
                .contentShape(RoundedRectangle(cornerRadius: Theme.Radius.agentField))
                .opacity(enabled ? 1 : Theme.Opacity.disabled)
        }
        .buttonStyle(.plain)
        .disabled(!enabled)
        .onHover { hovering = $0 }
        .help(model.deliverHow == .print ? "Print these photos" : "Write these files with the chosen settings")
        .accessibilityIdentifier("library.deliver.export")
    }
}
