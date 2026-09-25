import AppKit
import SwiftUI
import TesseraCore
import TesseraFFI

/// File ▸ Import Lightroom Catalog… (docs/05 §3, docs/06 §2.1): choose `.lrcat` → summary →
/// mapping (folders with relocation, selection mapping, mark names, keyword hierarchy) → fidelity
/// preview → import (non-modal, in the main window) → report. Nothing is written before Import.
struct LightroomImportSheet: View {
    @Bindable var importer: LightroomImportController
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        SheetScaffold(title: "Import Lightroom Catalog",
                      subtitle: importer.catalogURL?.path ?? "Folders, photos, edits, selections, collections and keywords. The catalog is only read.") {
            HStack(spacing: Theme.Space.s - Theme.Space.xxs) {
                ForEach(LightroomImportController.Step.allCases, id: \.self) { s in
                    Text(s.title)
                        .font(s == importer.step ? Theme.Fonts.captionSemibold : Theme.Fonts.caption)
                        .foregroundStyle(s == importer.step ? Theme.textPrimary : s < importer.step ? Theme.textSecondary : Theme.textTertiary)
                    if s != .report {
                        Image(systemName: "chevron.right").font(Theme.Fonts.iconSmall).foregroundStyle(Theme.textTertiary)
                    }
                }
            }
            .accessibilityElement(children: .combine)
            .accessibilityIdentifier("lrimport-steps")
        } content: {
            VStack(spacing: 0) {
                Group {
                    switch importer.step {
                    case .choose: ChooseStep(importer: importer)
                    case .summary: SummaryStep(importer: importer)
                    case .mapping: MappingStep(importer: importer)
                    case .fidelity: FidelityStep(importer: importer)
                    case .report: ReportStep(importer: importer)
                    }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                if let error = importer.error {
                    Hairline()
                    StatusLine(text: error, kind: .error)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.horizontal, Theme.Space.l).padding(.vertical, Theme.Space.s)
                        .accessibilityIdentifier("lrimport-error")
                }
            }
        } leading: {
            if let busy = importer.busy {
                ProgressView().controlSize(.small)
                Text(busy)
            } else if importer.step == .mapping, let p = importer.preview {
                Text("\(p.toImport) photo\(p.toImport == 1 ? "" : "s") will be imported · \(p.missing) missing · \(p.conflicts) skipped · \(p.virtualCopies) virtual cop\(p.virtualCopies == 1 ? "y" : "ies") kept in the bundle")
                    .monospacedDigit()
                    .accessibilityIdentifier("lrimport-plan-line")
            }
        } actions: {
            switch importer.step {
            case .choose:
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction).sheetButton()
            case .summary:
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction).sheetButton()
                Button("Choose Another…") { importer.chooseCatalog(in: NSApp.keyWindow) }.sheetButton()
                Button("Continue") { importer.goTo(.mapping) }.keyboardShortcut(.defaultAction).sheetButton(primary: true)
            case .mapping:
                Button("Back") { importer.goTo(.summary) }.sheetButton()
                Button("Preview Fidelity") { importer.goTo(.fidelity) }.keyboardShortcut(.defaultAction).sheetButton(primary: true)
                    .disabled(importer.preview == nil)
            case .fidelity:
                Button("Back") { importer.goTo(.mapping) }.sheetButton()
                Button("Import \(importer.preview?.toImport ?? 0) Photos") {
                    dismiss()
                    importer.startImport()
                }
                .keyboardShortcut(.defaultAction)
                .sheetButton(primary: true)
                .disabled(importer.preview.map { $0.toImport == 0 } ?? true || importer.busy != nil)
                .help("Writes .edits/ and .xmp sidecars beside the photos and merges library.json. Runs in the background.")
            case .report:
                if let url = importer.reportURL {
                    Button("Show Report in Finder") { NSWorkspace.shared.activateFileViewerSelecting([url]) }.sheetButton()
                }
                if importer.report?.cancelled == true {
                    Button("Resume Import") {
                        // Dismiss first: resuming clears the report this button belongs to.
                        dismiss()
                        importer.resume()
                    }
                    .sheetButton()
                }
                Button("Done") { importer.reset(); dismiss() }.keyboardShortcut(.defaultAction).sheetButton(primary: true)
            }
        }
        .frame(width: 820, height: 620)
    }
}

// MARK: Choose

private struct ChooseStep: View {
    let importer: LightroomImportController
    var body: some View {
        VStack(spacing: Theme.Space.m) {
            Image(systemName: "square.stack.3d.down.right").font(Theme.Fonts.iconLarge).foregroundStyle(Theme.textSecondary)
            Text("Choose a Lightroom Classic catalog (.lrcat)").font(Theme.Fonts.title)
            Text("Tessera reads a copy of the catalog and its previews. Your photos stay where they are: edits and\nselections are written as sidecars next to them, collections go into library.json.")
                .font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary).multilineTextAlignment(.center)
            Button("Choose Catalog…") { importer.chooseCatalog(in: NSApp.keyWindow) }
                .keyboardShortcut(.defaultAction)
                .buttonStyle(.theme(.primary, height: Theme.Height.large))
                .disabled(importer.busy != nil)
        }
        .padding(Theme.Space.xxl)
    }
}

// MARK: Summary

private struct SummaryStep: View {
    let importer: LightroomImportController
    var body: some View {
        if let s = importer.summary {
            ScrollView {
                VStack(alignment: .leading, spacing: Theme.Space.m) {
                    if s.lightroomRunning || s.catalogLocked {
                        Label(s.lightroomRunning
                              ? "Lightroom Classic is running. Quit it first: Tessera reads a copy, and a catalog that changes while it is copied cannot be read."
                              : "The catalog has a lock file (Lightroom may be open, or quit unexpectedly). It is read from a copy and left untouched.",
                              systemImage: "lock.fill")
                            .font(Theme.Fonts.label).foregroundStyle(Theme.warning)
                            .accessibilityIdentifier("lrimport-lock-warning")
                    }
                    Grid(alignment: .leading, horizontalSpacing: 28, verticalSpacing: 6) {
                        row("Photos", "\(s.images - s.virtualCopies)", "Virtual copies", "\(s.virtualCopies)")
                        row("Folders", "\(s.folders)", "With develop edits", "\(s.edited)")
                        row("Keywords", "\(s.keywords)", "Collections", "\(s.collections)")
                        row("Collection sets", "\(s.collectionSets)", "Smart collections", "\(s.smartCollections)")
                        row("Stacks", "\(s.stacks)", "Faces", "\(s.faces)")
                        row("Lightroom previews", "\(s.previews)", "Disk space needed", ByteCountFormatter.fileSize(s.estimatedBytes))
                    }
                    .font(Theme.Fonts.labelNumeric)
                    .accessibilityIdentifier("lrimport-summary-counts")
                    Text("Catalog version \(s.schemaVersion) · roots: \(s.roots.joined(separator: ", "))")
                        .font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                    Divider()
                    Text("Not fully supported (\(s.unsupported.count))").font(Theme.Fonts.labelSemibold)
                    if s.unsupported.isEmpty {
                        Text("Everything in this catalog has a Tessera equivalent.").font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                    }
                    ForEach(Array(s.unsupported.enumerated()), id: \.offset) { _, issue in
                        IssueRow(issue: issue)
                    }
                }
                .padding(Theme.Space.l)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }

    @ViewBuilder private func row(_ a: String, _ av: String, _ b: String, _ bv: String) -> some View {
        GridRow {
            Text(a).foregroundStyle(Theme.textSecondary)
            Text(av).fontWeight(.medium)
            Text(b).foregroundStyle(Theme.textSecondary)
            Text(bv).fontWeight(.medium)
        }
    }
}

private struct IssueRow: View {
    let issue: LrcatIssue
    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: Theme.Space.s) {
            Text(issue.category).font(Theme.Fonts.captionMedium).frame(width: 120, alignment: .leading)
            VStack(alignment: .leading, spacing: Theme.Space.xxs) {
                Text(issue.reason).font(Theme.Fonts.caption)
                if !issue.examples.isEmpty {
                    Text(issue.examples.joined(separator: ", ")).font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary)
                }
            }
            Spacer()
            Text("\(issue.count)").font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textSecondary)
        }
    }
}

// MARK: Mapping

private struct MappingStep: View {
    @Bindable var importer: LightroomImportController

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Theme.Space.l) {
                libraryFolder
                foldersSection
                selectionSection
                marksSection
                keywordsSection
                Toggle("Replace edits already made in Tessera", isOn: $importer.overwriteExistingEdits)
                    .font(Theme.Fonts.label)
                    .help("Off: photos that already have Tessera edits keep them and are listed as skipped.")
            }
            .padding(Theme.Space.l)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private func section(_ title: String, _ detail: String) -> some View {
        VStack(alignment: .leading, spacing: Theme.Space.xxs) {
            Text(title).font(Theme.Fonts.labelSemibold)
            Text(detail).font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
        }
    }

    private var libraryFolder: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            section("Library folder", "library.json and import-report.md are written here; the app opens this folder when the import finishes.")
            HStack {
                Text(importer.folders?.libraryFolder ?? "").font(Theme.Fonts.labelMono)
                    .lineLimit(1).truncationMode(.middle)
                    .accessibilityIdentifier("lrimport-library-folder")
                Spacer()
                Button("Choose…") { importer.chooseLibraryFolder(in: NSApp.keyWindow) }.buttonStyle(.themeBordered)
            }
            if let p = importer.preview {
                if p.libraryExists {
                    Text("library.json exists: Lightroom collections are merged into it (existing albums keep their names; clashes get “(Lightroom)”).")
                        .font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                }
                if p.outsideLibrary > 0 {
                    Label("\(p.outsideLibrary) photos are outside this folder and will not appear when it is opened.", systemImage: "exclamationmark.triangle")
                        .font(Theme.Fonts.caption).foregroundStyle(Theme.warning)
                }
            }
        }
    }

    private var foldersSection: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            section("Folders", "Where Lightroom's folders are now. Locate a root folder if its drive moved or was renamed.")
            ForEach(importer.folders?.roots ?? []) { root in
                let row = importer.preview?.roots.first { $0.catalogPath == root.catalogPath }
                HStack(spacing: Theme.Space.s) {
                    Image(systemName: row?.exists == true ? "checkmark.circle.fill" : "questionmark.folder.fill")
                        .foregroundStyle(row?.exists == true ? Theme.keep : Theme.warning)
                    VStack(alignment: .leading, spacing: Theme.Space.xxs) {
                        Text(root.catalogPath).font(Theme.Fonts.captionMono).foregroundStyle(Theme.textSecondary)
                        if importer.folders?.isRelocated(root) == true {
                            Text("→ \(root.path)").font(Theme.Fonts.captionMono)
                        }
                    }
                    Spacer()
                    if let row { Text("\(row.images - row.missing)/\(row.images) found").font(Theme.Fonts.captionNumeric) }
                    Button(row?.exists == true ? "Change…" : "Locate…") { importer.locate(root: root, in: NSApp.keyWindow) }
                        .buttonStyle(.themeBordered)
                        .accessibilityIdentifier("lrimport-locate")
                    if importer.folders?.isRelocated(root) == true {
                        Button("Reset") { importer.folders?.reset(root.catalogPath) }.buttonStyle(.themeBorderless)
                    }
                }
                .padding(Theme.Space.s)
                .background(RoundedRectangle(cornerRadius: Theme.Radius.control).fill(Theme.raised))
                .overlay(RoundedRectangle(cornerRadius: Theme.Radius.control).strokeBorder(Theme.hairline, lineWidth: Theme.Space.hairline))
            }
            if let folders = importer.preview?.folders, !folders.isEmpty {
                Grid(alignment: .leading, horizontalSpacing: 14, verticalSpacing: 3) {
                    GridRow {
                        Text("Folder"); Text("Photos"); Text("Missing"); Text("Copies"); Text("")
                    }
                    .font(Theme.Fonts.captionMedium).foregroundStyle(Theme.textSecondary)
                    ForEach(folders, id: \.catalogPath) { f in
                        GridRow {
                            Text(importer.folders?.displayName(f.path) ?? f.path)
                                .font(Theme.Fonts.captionMono).lineLimit(1).truncationMode(.head)
                                .help(f.path)
                            Text("\(f.images)")
                            Text("\(f.missing)").foregroundStyle(f.missing > 0 ? Theme.reject : .secondary)
                            Text("\(f.virtualCopies)")
                            Text(!f.exists ? "not found" : f.insideLibrary ? "" : "outside library")
                                .foregroundStyle(Theme.warning)
                        }
                        .font(Theme.Fonts.captionNumeric)
                    }
                }
                .accessibilityIdentifier("lrimport-folders")
            }
        }
    }

    private var selectionSection: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            section("Selection mapping", "Lightroom flags and stars become Tessera decisions and grades (docs/06 §2.1). Stars 2 / 3–4 / 5 become grades 1 / 2 / 3.")
            Grid(alignment: .leading, horizontalSpacing: 18, verticalSpacing: 3) {
                GridRow { Text("Lightroom"); Text("Tessera"); Text("Photos") }
                    .font(Theme.Fonts.captionMedium).foregroundStyle(Theme.textSecondary)
                ForEach(importer.preview?.selectionRows ?? [], id: \.lightroom) { r in
                    GridRow {
                        Text(r.lightroom)
                        Text(SelectionText.tessera(r.decision, grade: r.grade))
                            .foregroundStyle(r.decision == .reject ? Theme.reject : r.decision == .keep ? Theme.keep : Theme.textSecondary)
                        Text("\(r.count)")
                    }
                    .font(Theme.Fonts.captionNumeric)
                }
            }
            .accessibilityIdentifier("lrimport-selection")
            if let c = importer.preview?.selection {
                Text("\(c.keeps) Keep · \(c.rejects) Reject · \(c.undecided) Undecided · \(c.marked) marked")
                    .font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textSecondary)
            }
        }
    }

    private var marksSection: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            section("Colour labels → marks", "Keep a label's text, map it to one of Tessera's marks (keys 6–9), or drop it.")
            if let table = importer.marks, !table.rows.isEmpty {
                ForEach(table.rows) { row in
                    HStack {
                        Text(row.label).frame(width: 140, alignment: .leading)
                        Text("\(row.count) photo\(row.count == 1 ? "" : "s")").foregroundStyle(Theme.textSecondary).frame(width: 80, alignment: .leading)
                        Picker("", selection: Binding(get: { row.choice },
                                                      set: { importer.marks?.set(row.label, to: $0) })) {
                            Text("Keep “\(row.label)”").tag(MarkChoice.keepLabel)
                            Divider()
                            ForEach(table.appMarks, id: \.self) { Text($0).tag(MarkChoice.mark($0)) }
                            Divider()
                            Text("No mark").tag(MarkChoice.drop)
                        }
                        .labelsHidden()
                        .frame(width: 220)
                        .accessibilityIdentifier("lrimport-mark-\(row.label)")
                        Spacer()
                    }
                    .font(Theme.Fonts.caption)
                }
                if !table.mergedTargets.isEmpty {
                    Text("Merged: " + table.mergedTargets.joined(separator: ", ")).font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                }
            } else {
                Text("No colour labels in this catalog.").font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
            }
        }
    }

    private var keywordsSection: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            section("Keyword hierarchy", "Merged into the library's keyword list; photos get the keywords in their XMP sidecars.")
            VStack(alignment: .leading, spacing: Theme.Space.xxs) {
                ForEach(Array((importer.preview?.keywords ?? []).enumerated()), id: \.offset) { _, k in
                    HStack(spacing: Theme.Space.s) {
                        Text(k.name).padding(.leading, CGFloat(k.depth) * 16)
                        if !k.synonyms.isEmpty {
                            Text("(\(k.synonyms.joined(separator: ", ")))").foregroundStyle(Theme.textTertiary)
                        }
                        if k.merged { Text("merged").font(Theme.Fonts.captionSemibold).foregroundStyle(Theme.warning) }
                        Spacer()
                        Text("\(k.images)").foregroundStyle(Theme.textSecondary)
                    }
                    .font(Theme.Fonts.captionNumeric)
                }
            }
            .frame(maxWidth: 420, alignment: .leading)
            .accessibilityIdentifier("lrimport-keywords")
        }
    }
}

// MARK: Fidelity

private struct FidelityStep: View {
    @Bindable var importer: LightroomImportController

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                if let f = importer.fidelityResult {
                    Text(f.renderer == "native"
                         ? "Rendered with Tessera's native pipeline (the Adobe-compatible renderer is not available yet); compared with Lightroom's cached previews."
                         : "Rendered with the \(f.renderer) renderer; compared with Lightroom's cached previews.")
                        .font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                }
                Spacer()
                if importer.fidelity != nil {
                    Picker("Sort", selection: Binding(get: { importer.fidelity?.sort ?? .largestDifference },
                                                      set: { importer.fidelity?.sort = $0 })) {
                        ForEach(FidelityGrid.Sort.allCases) { Text($0.rawValue).tag($0) }
                    }
                    .frame(width: 210)
                    Toggle("Looks different (\(importer.fidelity?.differentCount ?? 0))", isOn: Binding(
                        get: { importer.fidelity?.onlyDifferent ?? false }, set: { importer.fidelity?.onlyDifferent = $0 }))
                        .toggleStyle(.checkbox)
                        .accessibilityIdentifier("lrimport-looks-different")
                }
            }
            .padding(.horizontal, Theme.Space.l).padding(.vertical, Theme.Space.s)
            Divider()
            if let grid = importer.fidelity {
                if grid.samples.isEmpty {
                    Text(importer.fidelityResult?.previewsAvailable == false
                         ? "This catalog has no Previews.lrdata: nothing to compare against."
                         : "No photo could be rendered (are the originals located?).")
                        .font(Theme.Fonts.label).foregroundStyle(Theme.textSecondary)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else {
                    ScrollView {
                        LazyVGrid(columns: [GridItem(.adaptive(minimum: 360), spacing: Theme.Space.m)], spacing: Theme.Space.m) {
                            ForEach(grid.visible, id: \.catalogId) { FidelityPair(sample: $0) }
                        }
                        .padding(Theme.Space.m)
                    }
                    .accessibilityIdentifier("lrimport-fidelity-grid")
                }
            } else if importer.busy != nil {
                Spacer()
            }
        }
    }
}

private struct FidelityPair: View {
    let sample: LrcatFidelitySample
    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            HStack(spacing: Theme.Space.xs) {
                side(sample.lightroomJpeg, "Lightroom")
                side(sample.tesseraJpeg, "Tessera")
            }
            HStack {
                Text(sample.name).font(Theme.Fonts.captionMedium).lineLimit(1)
                Spacer()
                badge
            }
            if !sample.message.isEmpty {
                Text(sample.message).font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
            }
        }
        .padding(Theme.Space.s)
        .background(RoundedRectangle(cornerRadius: Theme.Radius.card).fill(Theme.raised))
        .overlay(RoundedRectangle(cornerRadius: Theme.Radius.card).strokeBorder(Theme.hairline, lineWidth: Theme.Space.hairline))
        .accessibilityElement(children: .combine)
    }

    @ViewBuilder private var badge: some View {
        if sample.status == .compared {
            let different = FidelityGrid.looksDifferent(sample)
            Chip(text: String(format: "ΔE %.1f · p95 %.1f", sample.deltaEMean, sample.deltaEP95),
                 color: different ? Theme.reject : sample.deltaEMean < 2 ? Theme.keep : Theme.warning, style: .outlined)
                .help(different ? "Looks different: mean ΔE ≥ 3 or 95th percentile ≥ 10" : "Mean and 95th-percentile CIEDE2000")
        } else {
            Text(sample.status == .noPreview ? "no preview" : sample.status == .missingOriginal ? "missing" : "failed")
                .font(Theme.Fonts.captionSemibold).foregroundStyle(Theme.textSecondary)
        }
    }

    private func side(_ jpeg: Data, _ label: String) -> some View {
        ZStack(alignment: .topLeading) {
            Rectangle().fill(Theme.well)
            if let image = NSImage(data: jpeg) {
                Image(nsImage: image).resizable().aspectRatio(contentMode: .fit)
            }
            Text(label).font(Theme.Fonts.captionMedium)
                .foregroundStyle(Color(nsColor: Theme.Palette.OnImage.text))
                .padding(.horizontal, Theme.Space.xs)
                .frame(height: Theme.Height.chip)
                .background(RoundedRectangle(cornerRadius: Theme.Radius.chip).fill(Color(nsColor: Theme.Palette.OnImage.scrim)))
                .padding(Theme.Space.xs)
        }
        .frame(height: 118)
    }
}

// MARK: Report

private struct ReportStep: View {
    let importer: LightroomImportController
    var body: some View {
        if let r = importer.report {
            ScrollView {
                VStack(alignment: .leading, spacing: Theme.Space.m) {
                    Label(r.cancelled ? "Import cancelled. Resume to continue where it stopped; finished photos are skipped."
                          : "Imported \(r.imported + r.resumed) photos into \(URL(fileURLWithPath: r.libraryPath).deletingLastPathComponent().lastPathComponent).",
                          systemImage: r.cancelled ? "pause.circle.fill" : "checkmark.circle.fill")
                        .font(Theme.Fonts.bodyMedium)
                        .foregroundStyle(r.cancelled ? Theme.warning : Theme.keep)
                        .accessibilityIdentifier("lrimport-report-status")
                    Grid(alignment: .leading, horizontalSpacing: 24, verticalSpacing: 4) {
                        GridRow { Text("Photos written").foregroundStyle(Theme.textSecondary); Text("\(r.imported)")
                                  Text("Resumed").foregroundStyle(Theme.textSecondary); Text("\(r.resumed)") }
                        GridRow { Text("Albums").foregroundStyle(Theme.textSecondary); Text("\(r.albums)")
                                  Text("Album groups").foregroundStyle(Theme.textSecondary); Text("\(r.albumGroups)") }
                        GridRow { Text("Smart albums").foregroundStyle(Theme.textSecondary); Text("\(r.smartAlbums)")
                                  Text("Keywords").foregroundStyle(Theme.textSecondary); Text("\(r.keywords)") }
                        GridRow { Text("Skipped").foregroundStyle(Theme.textSecondary); Text("\(r.skipped.count)")
                                  Text("Virtual copies (bundle)").foregroundStyle(Theme.textSecondary); Text("\(r.virtualCopies)") }
                    }
                    .font(Theme.Fonts.labelNumeric)
                    .accessibilityIdentifier("lrimport-report-counts")
                    if !r.skipped.isEmpty {
                        Text("Skipped").font(Theme.Fonts.labelSemibold)
                        ForEach(Array(r.skipped.enumerated()), id: \.offset) { _, s in
                            HStack(alignment: .firstTextBaseline) {
                                Text(s.name).font(Theme.Fonts.captionMedium).frame(width: 200, alignment: .leading)
                                Text(s.reason).font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                            }
                        }
                    }
                    if let url = importer.reportURL {
                        Text("Full report: \(url.path)").font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                            .textSelection(.enabled)
                            .accessibilityIdentifier("lrimport-report-path")
                    }
                    if let md = importer.reportMarkdown {
                        DisclosureGroup("import-report.md") {
                            Text(md).font(Theme.Fonts.captionMono).textSelection(.enabled)
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                        .font(Theme.Fonts.caption)
                    }
                }
                .padding(Theme.Space.l)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }
}

// MARK: Non-modal progress (main window)

/// Shown above the status bar while an import runs; the rest of the window stays usable.
struct LightroomImportProgressBar: View {
    let importer: LightroomImportController
    var body: some View {
        if let p = importer.progress {
            ProgressStrip(title: "Lightroom import · \(p.phase.title)", done: Int(p.done), total: Int(p.total), current: p.current) {
                Button("Cancel Import") { importer.cancelImport() }
                    .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                    .accessibilityIdentifier("lrimport-cancel")
            }
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("lrimport-progress")
        }
    }
}
