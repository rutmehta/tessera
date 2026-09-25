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
        VStack(spacing: 0) {
            header
            Divider()
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
                Divider()
                Label(error, systemImage: "exclamationmark.triangle.fill")
                    .font(.system(size: 11)).foregroundStyle(Color(nsColor: Theme.reject))
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 16).padding(.vertical, 6)
                    .accessibilityIdentifier("lrimport-error")
            }
            Divider()
            footer
        }
        .frame(width: 820, height: 620)
    }

    private var header: some View {
        HStack(alignment: .firstTextBaseline) {
            VStack(alignment: .leading, spacing: 3) {
                Text("Import Lightroom Catalog").font(.system(size: 15, weight: .semibold))
                Text(importer.catalogURL?.path ?? "Folders, photos, edits, selections, collections and keywords. The catalog is only read.")
                    .font(.system(size: 11)).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
            }
            Spacer()
            HStack(spacing: 6) {
                ForEach(LightroomImportController.Step.allCases, id: \.self) { s in
                    Text(s.title)
                        .font(.system(size: 11, weight: s == importer.step ? .semibold : .regular))
                        .foregroundStyle(s == importer.step ? Color.primary : s < importer.step ? Color.secondary : Color(nsColor: .tertiaryLabelColor))
                    if s != .report { Image(systemName: "chevron.right").font(.system(size: 8)).foregroundStyle(.tertiary) }
                }
            }
            .accessibilityElement(children: .combine)
            .accessibilityIdentifier("lrimport-steps")
        }
        .padding(16)
    }

    private var footer: some View {
        HStack {
            if let busy = importer.busy {
                ProgressView().controlSize(.small)
                Text(busy).font(.system(size: 11)).foregroundStyle(.secondary)
            } else if importer.step == .mapping, let p = importer.preview {
                Text("\(p.toImport) photo\(p.toImport == 1 ? "" : "s") will be imported · \(p.missing) missing · \(p.conflicts) skipped · \(p.virtualCopies) virtual cop\(p.virtualCopies == 1 ? "y" : "ies") kept in the bundle")
                    .font(.system(size: 11).monospacedDigit()).foregroundStyle(.secondary)
                    .accessibilityIdentifier("lrimport-plan-line")
            }
            Spacer()
            switch importer.step {
            case .choose:
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
            case .summary:
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Choose Another…") { importer.chooseCatalog(in: NSApp.keyWindow) }
                Button("Continue") { importer.goTo(.mapping) }.keyboardShortcut(.defaultAction)
            case .mapping:
                Button("Back") { importer.goTo(.summary) }
                Button("Preview Fidelity") { importer.goTo(.fidelity) }.keyboardShortcut(.defaultAction)
                    .disabled(importer.preview == nil)
            case .fidelity:
                Button("Back") { importer.goTo(.mapping) }
                Button("Import \(importer.preview?.toImport ?? 0) Photos") {
                    dismiss()
                    importer.startImport()
                }
                .keyboardShortcut(.defaultAction)
                .disabled(importer.preview.map { $0.toImport == 0 } ?? true || importer.busy != nil)
                .help("Writes .edits/ and .xmp sidecars beside the photos and merges library.json. Runs in the background.")
            case .report:
                if let url = importer.reportURL {
                    Button("Show Report in Finder") { NSWorkspace.shared.activateFileViewerSelecting([url]) }
                }
                if importer.report?.cancelled == true {
                    Button("Resume Import") {
                        // Dismiss first: resuming clears the report this button belongs to.
                        dismiss()
                        importer.resume()
                    }
                }
                Button("Done") { importer.reset(); dismiss() }.keyboardShortcut(.defaultAction)
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
    }
}

// MARK: Choose

private struct ChooseStep: View {
    let importer: LightroomImportController
    var body: some View {
        VStack(spacing: 14) {
            Image(systemName: "square.stack.3d.down.right").font(.system(size: 34)).foregroundStyle(.secondary)
            Text("Choose a Lightroom Classic catalog (.lrcat)").font(.system(size: 14, weight: .medium))
            Text("Tessera reads a copy of the catalog and its previews. Your photos stay where they are: edits and\nselections are written as sidecars next to them, collections go into library.json.")
                .font(.system(size: 11)).foregroundStyle(.secondary).multilineTextAlignment(.center)
            Button("Choose Catalog…") { importer.chooseCatalog(in: NSApp.keyWindow) }
                .keyboardShortcut(.defaultAction)
                .disabled(importer.busy != nil)
        }
        .padding(30)
    }
}

// MARK: Summary

private struct SummaryStep: View {
    let importer: LightroomImportController
    var body: some View {
        if let s = importer.summary {
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    if s.lightroomRunning || s.catalogLocked {
                        Label(s.lightroomRunning
                              ? "Lightroom Classic is running. Quit it first: Tessera reads a copy, and a catalog that changes while it is copied cannot be read."
                              : "The catalog has a lock file (Lightroom may be open, or quit unexpectedly). It is read from a copy and left untouched.",
                              systemImage: "lock.fill")
                            .font(.system(size: 12)).foregroundStyle(Color(nsColor: Theme.accent))
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
                    .font(.system(size: 12).monospacedDigit())
                    .accessibilityIdentifier("lrimport-summary-counts")
                    Text("Catalog version \(s.schemaVersion) · roots: \(s.roots.joined(separator: ", "))")
                        .font(.system(size: 11)).foregroundStyle(.secondary)
                    Divider()
                    Text("Not fully supported (\(s.unsupported.count))").font(.system(size: 12, weight: .semibold))
                    if s.unsupported.isEmpty {
                        Text("Everything in this catalog has a Tessera equivalent.").font(.system(size: 11)).foregroundStyle(.secondary)
                    }
                    ForEach(Array(s.unsupported.enumerated()), id: \.offset) { _, issue in
                        IssueRow(issue: issue)
                    }
                }
                .padding(16)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }

    @ViewBuilder private func row(_ a: String, _ av: String, _ b: String, _ bv: String) -> some View {
        GridRow {
            Text(a).foregroundStyle(.secondary)
            Text(av).fontWeight(.medium)
            Text(b).foregroundStyle(.secondary)
            Text(bv).fontWeight(.medium)
        }
    }
}

private struct IssueRow: View {
    let issue: LrcatIssue
    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text(issue.category).font(.system(size: 11, weight: .medium)).frame(width: 120, alignment: .leading)
            VStack(alignment: .leading, spacing: 2) {
                Text(issue.reason).font(.system(size: 11))
                if !issue.examples.isEmpty {
                    Text(issue.examples.joined(separator: ", ")).font(.system(size: 10)).foregroundStyle(.tertiary)
                }
            }
            Spacer()
            Text("\(issue.count)").font(.system(size: 11).monospacedDigit()).foregroundStyle(.secondary)
        }
    }
}

// MARK: Mapping

private struct MappingStep: View {
    @Bindable var importer: LightroomImportController

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                libraryFolder
                foldersSection
                selectionSection
                marksSection
                keywordsSection
                Toggle("Replace edits already made in Tessera", isOn: $importer.overwriteExistingEdits)
                    .font(.system(size: 12))
                    .help("Off: photos that already have Tessera edits keep them and are listed as skipped.")
            }
            .padding(16)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private func section(_ title: String, _ detail: String) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(title).font(.system(size: 12, weight: .semibold))
            Text(detail).font(.system(size: 11)).foregroundStyle(.secondary)
        }
    }

    private var libraryFolder: some View {
        VStack(alignment: .leading, spacing: 6) {
            section("Library folder", "library.json and import-report.md are written here; the app opens this folder when the import finishes.")
            HStack {
                Text(importer.folders?.libraryFolder ?? "").font(.system(size: 12, design: .monospaced))
                    .lineLimit(1).truncationMode(.middle)
                    .accessibilityIdentifier("lrimport-library-folder")
                Spacer()
                Button("Choose…") { importer.chooseLibraryFolder(in: NSApp.keyWindow) }
            }
            if let p = importer.preview {
                if p.libraryExists {
                    Text("library.json exists: Lightroom collections are merged into it (existing albums keep their names; clashes get “(Lightroom)”).")
                        .font(.system(size: 11)).foregroundStyle(.secondary)
                }
                if p.outsideLibrary > 0 {
                    Label("\(p.outsideLibrary) photos are outside this folder and will not appear when it is opened.", systemImage: "exclamationmark.triangle")
                        .font(.system(size: 11)).foregroundStyle(Color(nsColor: Theme.accent))
                }
            }
        }
    }

    private var foldersSection: some View {
        VStack(alignment: .leading, spacing: 6) {
            section("Folders", "Where Lightroom's folders are now. Locate a root folder if its drive moved or was renamed.")
            ForEach(importer.folders?.roots ?? []) { root in
                let row = importer.preview?.roots.first { $0.catalogPath == root.catalogPath }
                HStack(spacing: 8) {
                    Image(systemName: row?.exists == true ? "checkmark.circle.fill" : "questionmark.folder.fill")
                        .foregroundStyle(row?.exists == true ? Color(nsColor: Theme.keep) : Color(nsColor: Theme.accent))
                    VStack(alignment: .leading, spacing: 1) {
                        Text(root.catalogPath).font(.system(size: 11, design: .monospaced)).foregroundStyle(.secondary)
                        if importer.folders?.isRelocated(root) == true {
                            Text("→ \(root.path)").font(.system(size: 11, design: .monospaced))
                        }
                    }
                    Spacer()
                    if let row { Text("\(row.images - row.missing)/\(row.images) found").font(.system(size: 11).monospacedDigit()) }
                    Button(row?.exists == true ? "Change…" : "Locate…") { importer.locate(root: root, in: NSApp.keyWindow) }
                        .accessibilityIdentifier("lrimport-locate")
                    if importer.folders?.isRelocated(root) == true {
                        Button("Reset") { importer.folders?.reset(root.catalogPath) }
                    }
                }
                .padding(8)
                .background(RoundedRectangle(cornerRadius: 6).fill(Color.white.opacity(0.04)))
            }
            if let folders = importer.preview?.folders, !folders.isEmpty {
                Grid(alignment: .leading, horizontalSpacing: 14, verticalSpacing: 3) {
                    GridRow {
                        Text("Folder"); Text("Photos"); Text("Missing"); Text("Copies"); Text("")
                    }
                    .font(.system(size: 10, weight: .medium)).foregroundStyle(.secondary)
                    ForEach(folders, id: \.catalogPath) { f in
                        GridRow {
                            Text(importer.folders?.displayName(f.path) ?? f.path)
                                .font(.system(size: 11, design: .monospaced)).lineLimit(1).truncationMode(.head)
                                .help(f.path)
                            Text("\(f.images)")
                            Text("\(f.missing)").foregroundStyle(f.missing > 0 ? Color(nsColor: Theme.reject) : .secondary)
                            Text("\(f.virtualCopies)")
                            Text(!f.exists ? "not found" : f.insideLibrary ? "" : "outside library")
                                .foregroundStyle(Color(nsColor: Theme.accent))
                        }
                        .font(.system(size: 11).monospacedDigit())
                    }
                }
                .accessibilityIdentifier("lrimport-folders")
            }
        }
    }

    private var selectionSection: some View {
        VStack(alignment: .leading, spacing: 6) {
            section("Selection mapping", "Lightroom flags and stars become Tessera decisions and grades (docs/06 §2.1). Stars 2 / 3–4 / 5 become grades 1 / 2 / 3.")
            Grid(alignment: .leading, horizontalSpacing: 18, verticalSpacing: 3) {
                GridRow { Text("Lightroom"); Text("Tessera"); Text("Photos") }
                    .font(.system(size: 10, weight: .medium)).foregroundStyle(.secondary)
                ForEach(importer.preview?.selectionRows ?? [], id: \.lightroom) { r in
                    GridRow {
                        Text(r.lightroom)
                        Text(SelectionText.tessera(r.decision, grade: r.grade))
                            .foregroundStyle(Color(nsColor: r.decision == .reject ? Theme.reject : r.decision == .keep ? Theme.keep : Theme.textSecondary))
                        Text("\(r.count)")
                    }
                    .font(.system(size: 11).monospacedDigit())
                }
            }
            .accessibilityIdentifier("lrimport-selection")
            if let c = importer.preview?.selection {
                Text("\(c.keeps) Keep · \(c.rejects) Reject · \(c.undecided) Undecided · \(c.marked) marked")
                    .font(.system(size: 11).monospacedDigit()).foregroundStyle(.secondary)
            }
        }
    }

    private var marksSection: some View {
        VStack(alignment: .leading, spacing: 6) {
            section("Colour labels → marks", "Keep a label's text, map it to one of Tessera's marks (keys 6–9), or drop it.")
            if let table = importer.marks, !table.rows.isEmpty {
                ForEach(table.rows) { row in
                    HStack {
                        Text(row.label).frame(width: 140, alignment: .leading)
                        Text("\(row.count) photo\(row.count == 1 ? "" : "s")").foregroundStyle(.secondary).frame(width: 80, alignment: .leading)
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
                    .font(.system(size: 11))
                }
                if !table.mergedTargets.isEmpty {
                    Text("Merged: " + table.mergedTargets.joined(separator: ", ")).font(.system(size: 11)).foregroundStyle(.secondary)
                }
            } else {
                Text("No colour labels in this catalog.").font(.system(size: 11)).foregroundStyle(.secondary)
            }
        }
    }

    private var keywordsSection: some View {
        VStack(alignment: .leading, spacing: 6) {
            section("Keyword hierarchy", "Merged into the library's keyword list; photos get the keywords in their XMP sidecars.")
            VStack(alignment: .leading, spacing: 2) {
                ForEach(Array((importer.preview?.keywords ?? []).enumerated()), id: \.offset) { _, k in
                    HStack(spacing: 6) {
                        Text(k.name).padding(.leading, CGFloat(k.depth) * 16)
                        if !k.synonyms.isEmpty {
                            Text("(\(k.synonyms.joined(separator: ", ")))").foregroundStyle(.tertiary)
                        }
                        if k.merged { Text("merged").font(.system(size: 9, weight: .semibold)).foregroundStyle(Color(nsColor: Theme.accent)) }
                        Spacer()
                        Text("\(k.images)").foregroundStyle(.secondary)
                    }
                    .font(.system(size: 11).monospacedDigit())
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
                        .font(.system(size: 11)).foregroundStyle(.secondary)
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
            .padding(.horizontal, 16).padding(.vertical, 8)
            Divider()
            if let grid = importer.fidelity {
                if grid.samples.isEmpty {
                    Text(importer.fidelityResult?.previewsAvailable == false
                         ? "This catalog has no Previews.lrdata: nothing to compare against."
                         : "No photo could be rendered (are the originals located?).")
                        .font(.system(size: 12)).foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else {
                    ScrollView {
                        LazyVGrid(columns: [GridItem(.adaptive(minimum: 360), spacing: 12)], spacing: 12) {
                            ForEach(grid.visible, id: \.catalogId) { FidelityPair(sample: $0) }
                        }
                        .padding(12)
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
        VStack(alignment: .leading, spacing: 5) {
            HStack(spacing: 4) {
                side(sample.lightroomJpeg, "Lightroom")
                side(sample.tesseraJpeg, "Tessera")
            }
            HStack {
                Text(sample.name).font(.system(size: 11, weight: .medium)).lineLimit(1)
                Spacer()
                badge
            }
            if !sample.message.isEmpty {
                Text(sample.message).font(.system(size: 10)).foregroundStyle(.secondary)
            }
        }
        .padding(8)
        .background(RoundedRectangle(cornerRadius: 7).fill(Color.white.opacity(0.04)))
        .accessibilityElement(children: .combine)
    }

    @ViewBuilder private var badge: some View {
        if sample.status == .compared {
            let different = FidelityGrid.looksDifferent(sample)
            Text(String(format: "ΔE %.1f · p95 %.1f", sample.deltaEMean, sample.deltaEP95))
                .font(.system(size: 10, weight: .semibold).monospacedDigit())
                .padding(.horizontal, 6).padding(.vertical, 2)
                .background(Capsule().fill(Color(nsColor: different ? Theme.reject : sample.deltaEMean < 2 ? Theme.keep : Theme.accent).opacity(0.85)))
                .foregroundStyle(.black)
                .help(different ? "Looks different: mean ΔE ≥ 3 or 95th percentile ≥ 10" : "Mean and 95th-percentile CIEDE2000")
        } else {
            Text(sample.status == .noPreview ? "no preview" : sample.status == .missingOriginal ? "missing" : "failed")
                .font(.system(size: 10, weight: .semibold)).foregroundStyle(.secondary)
        }
    }

    private func side(_ jpeg: Data, _ label: String) -> some View {
        ZStack(alignment: .topLeading) {
            Rectangle().fill(Color.black.opacity(0.3))
            if let image = NSImage(data: jpeg) {
                Image(nsImage: image).resizable().aspectRatio(contentMode: .fit)
            }
            Text(label).font(.system(size: 9, weight: .semibold))
                .padding(.horizontal, 4).padding(.vertical, 1)
                .background(Color.black.opacity(0.55)).padding(3)
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
                VStack(alignment: .leading, spacing: 12) {
                    Label(r.cancelled ? "Import cancelled. Resume to continue where it stopped; finished photos are skipped."
                          : "Imported \(r.imported + r.resumed) photos into \(URL(fileURLWithPath: r.libraryPath).deletingLastPathComponent().lastPathComponent).",
                          systemImage: r.cancelled ? "pause.circle.fill" : "checkmark.circle.fill")
                        .font(.system(size: 13, weight: .medium))
                        .foregroundStyle(Color(nsColor: r.cancelled ? Theme.accent : Theme.keep))
                        .accessibilityIdentifier("lrimport-report-status")
                    Grid(alignment: .leading, horizontalSpacing: 24, verticalSpacing: 4) {
                        GridRow { Text("Photos written").foregroundStyle(.secondary); Text("\(r.imported)")
                                  Text("Resumed").foregroundStyle(.secondary); Text("\(r.resumed)") }
                        GridRow { Text("Albums").foregroundStyle(.secondary); Text("\(r.albums)")
                                  Text("Album groups").foregroundStyle(.secondary); Text("\(r.albumGroups)") }
                        GridRow { Text("Smart albums").foregroundStyle(.secondary); Text("\(r.smartAlbums)")
                                  Text("Keywords").foregroundStyle(.secondary); Text("\(r.keywords)") }
                        GridRow { Text("Skipped").foregroundStyle(.secondary); Text("\(r.skipped.count)")
                                  Text("Virtual copies (bundle)").foregroundStyle(.secondary); Text("\(r.virtualCopies)") }
                    }
                    .font(.system(size: 12).monospacedDigit())
                    .accessibilityIdentifier("lrimport-report-counts")
                    if !r.skipped.isEmpty {
                        Text("Skipped").font(.system(size: 12, weight: .semibold))
                        ForEach(Array(r.skipped.enumerated()), id: \.offset) { _, s in
                            HStack(alignment: .firstTextBaseline) {
                                Text(s.name).font(.system(size: 11, weight: .medium)).frame(width: 200, alignment: .leading)
                                Text(s.reason).font(.system(size: 11)).foregroundStyle(.secondary)
                            }
                        }
                    }
                    if let url = importer.reportURL {
                        Text("Full report: \(url.path)").font(.system(size: 11)).foregroundStyle(.secondary)
                            .textSelection(.enabled)
                            .accessibilityIdentifier("lrimport-report-path")
                    }
                    if let md = importer.reportMarkdown {
                        DisclosureGroup("import-report.md") {
                            Text(md).font(.system(size: 10, design: .monospaced)).textSelection(.enabled)
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                        .font(.system(size: 11))
                    }
                }
                .padding(16)
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
            HStack(spacing: 10) {
                Text("Lightroom import · \(p.phase.title)").font(.system(size: 11, weight: .medium))
                if p.total > 0 {
                    ProgressView(value: Double(min(p.done, p.total)), total: Double(p.total))
                        .frame(width: 180)
                    Text("\(p.done) / \(p.total)").font(.system(size: 11).monospacedDigit()).foregroundStyle(.secondary)
                } else {
                    ProgressView().controlSize(.small)
                }
                Text(p.current).font(.system(size: 11)).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                Spacer()
                Button("Cancel Import") { importer.cancelImport() }
                    .controlSize(.small)
                    .accessibilityIdentifier("lrimport-cancel")
            }
            .padding(.horizontal, 12)
            .frame(height: 28)
            .background(Color(nsColor: Theme.cellBackground))
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("lrimport-progress")
        }
    }
}
