import AppKit
import TesseraCore
import TesseraFFI

/// File ▸ Export… and File ▸ Print… (WP M2-20): what they apply to, and what happens afterwards.
extension AppModel {
    func installOutputHandlers() {
        exporter.onFinish = { [weak self] report, settings in self?.exportDidFinish(report, settings: settings) }
        exporter.onFailure = { [weak self] message in
            self?.statusMessage = "Export failed: \(message)"
            self?.showToast("Export failed: \(message)", undoable: false)
        }
        printing.onMessage = { [weak self] message, details in
            self?.statusMessage = message
            self?.showToast(message, undoable: false, details: details)
        }
    }

    var engineLibrary: EngineLibrary? { library as? EngineLibrary }

    /// Items at the selected positions (the focused one when nothing else is selected).
    var selectedItems: [PhotoItem] {
        let positions = selection.isEmpty ? (focus.map { IndexSet(integer: $0) } ?? []) : selection
        return positions.sorted().map { item(at: $0) }
    }

    /// Items shown in the grid (the current album, smart album, filter…).
    var visibleItems: [PhotoItem] { visible.map { library.items[$0] } }

    /// Export targets: the selection, and the current view when it is narrower than "All Photos".
    func exportTargets() -> [ExportController.Target] {
        guard let lib = engineLibrary else { return [] }
        func target(_ kind: ExportController.Target.Kind, _ title: String, _ items: [PhotoItem])
            -> ExportController.Target? {
            let ids = items.compactMap { $0.engineImage?.imageID }
            guard let first = items.first, !ids.isEmpty else { return nil }
            return ExportController.Target(kind: kind, title: title, target: .images(imageIds: ids),
                                           count: ids.count, firstName: first.name, firstDate: first.captureDate, hasLightroomSmartPreviews: items.contains { $0.engineImage?.lightroomSmartPreview == true })
        }
        var result: [ExportController.Target] = []
        let selected = selectedItems
        if let t = target(.selection, selected.count == 1 ? selected[0].name : "Selected photos", selected) {
            result.append(t)
        }
        if source != .all || selected.count <= 1 {
            // Freeze the offered view to explicit image IDs so a later run gates
            // precisely the same photos, including a manual album's displayed order.
            let title = source == .all ? "All photos in \(lib.title)" : "“\(source.title)”"
            if let t = target(.view, title, visibleItems), t.count != result.first?.count || source != .all {
                result.append(t)
            }
        }
        return result
    }

    /// ⇧⌘E. Stub libraries have no pixels to export.
    func presentExport() {
        guard !exporter.isRunning else { statusMessage = "An export is already running"; return }
        guard let lib = engineLibrary else { statusMessage = "Export needs a folder opened on the engine"; return }
        let targets = exportTargets()
        guard !targets.isEmpty else { statusMessage = "Select photos to export"; return }
        let preferred: ExportController.Target.Kind = selectionCount > 1 || source == .all ? .selection : .view
        // Preparing the sheet only loads presets and frozen target IDs. The
        // actual image reads in Start and watermark Render reserve their own
        // saved-pixel gates for precisely the chosen target.
        exporter.acquireSaveGate = { [weak self, lib] ids in
            guard let self, self.engineLibrary === lib else { return nil }
            return self.prepareForRecipeRead(imageIDs: ids, library: lib)
        }
        exporter.prepare(engine: lib.engine, targets: targets, preferred: preferred)
        showExport = true
    }

    private func exportDidFinish(_ report: ExportReport, settings: ExportSettings) {
        let (headline, details) = report.toastLines
        statusMessage = headline
        showToast(headline, undoable: false, details: details)
        // Derived status: the grid's EXPORTED pills.
        let exportedIDs = Set(report.items.filter { $0.outputPath != nil }.map(\.imageId))
        if let lib = engineLibrary, !exportedIDs.isEmpty {
            let ids = lib.imageIDs.indices.filter { exportedIDs.contains(lib.imageIDs[$0]) }
            cull.refreshStatuses(ids)
            libraryItemsChanged(ids)
        }
        if settings.openInFinder {
            let urls = report.items.compactMap { $0.outputPath.map { URL(fileURLWithPath: $0) } }
            if !urls.isEmpty { NSWorkspace.shared.activateFileViewerSelecting(urls) }
        }
    }

    /// ⌘P: the selection (several) or the current view.
    func presentPrint() {
        guard !printing.isRunning else { statusMessage = "Printing is already in progress"; return }
        guard let lib = engineLibrary else { statusMessage = "Printing needs a folder opened on the engine"; return }
        let items = (selectionCount > 1 ? selectedItems : (source == .all ? selectedItems : visibleItems))
            .filter { $0.engineImage != nil }
        guard !items.isEmpty else { statusMessage = "Select photos to print"; return }
        let imageIDs = Set(items.compactMap { $0.engineImage?.imageID })
        let selected = selection
        let revision = libraryRevision
        let title = selectionCount > 1 || source == .all ? "Selection" : source.title
        let gate = prepareForRecipeRead(imageIDs: imageIDs, library: lib)
        Task { [weak self] in
            var previewOwnsGate = false
            defer { if !previewOwnsGate { gate.finish() } }
            guard await gate.result().isSaved else {
                self?.statusMessage = "Finish saving the photo before Print"
                return
            }
            guard let self, self.engineLibrary === lib, self.selection == selected,
                  self.libraryRevision == revision else { return }
            self.printing.acquireSaveGate = { [weak self, lib] ids in
                guard let self, self.engineLibrary === lib else { return nil }
                return self.prepareForRecipeRead(imageIDs: ids, library: lib)
            }
            self.printing.prepare(items: items, title: title, loader: self.loader, gate: gate)
            previewOwnsGate = true
            self.showPrint = true
        }
    }
}

// MARK: Self-test aids (hidden launch flags)

extension AppModel {
    /// `--export-selftest <dir>` / `--print-pdf-selftest <file.pdf>`: once the folder has loaded,
    /// select every photo and export it with the first (Web) preset into `dir`, or print a 5 × 4
    /// contact sheet to `file.pdf`; report on stderr and quit. Scratch folders only.
    func runOutputSelfTest(exportTo dir: URL?, pdf: URL?, polls: Int = 0) {
        guard !isLoading, !library.items.isEmpty, let lib = engineLibrary else {
            guard polls < 120 else {
                FileHandle.standardError.write(Data("output-selftest: no library\n".utf8))
                NSApp.terminate(nil)
                return
            }
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.25) {
                MainActor.assumeIsolated { self.runOutputSelfTest(exportTo: dir, pdf: pdf, polls: polls + 1) }
            }
            return
        }
        selectAll()
        if let dir {
            exporter.acquireSaveGate = { [weak self, lib] ids in
                guard let self, self.engineLibrary === lib else { return nil }
                return self.prepareForRecipeRead(imageIDs: ids, library: lib)
            }
            exporter.prepare(engine: lib.engine, targets: exportTargets(), preferred: .selection)
            if let web = exporter.presets.first { exporter.apply(web) }
            exporter.settings.destination = dir.path
            exporter.settings.openInFinder = false
            let previous = exporter.onFinish
            exporter.onFinish = { report, settings in
                previous(report, settings)
                FileHandle.standardError.write(Data(String(format: "export-selftest: %d exported, %d failed, cancelled %@, %.1f s → %@\n",
                                                            report.exported, report.failed, report.cancelled ? "yes" : "no",
                                                            report.seconds, report.destination).utf8))
                for item in report.items { FileHandle.standardError.write(Data("  \(item.name) → \(item.outputPath ?? item.error ?? "-")\n".utf8)) }
                if pdf == nil { self.quitAfterSelfTest() } else {
                    self.exporter.onFinish = previous
                    self.runOutputSelfTest(exportTo: nil, pdf: pdf)
                }
            }
            exporter.onFailure = { message in
                FileHandle.standardError.write(Data("export-selftest: failed: \(message)\n".utf8))
                self.quitAfterSelfTest()
            }
            exporter.start()
            return
        }
        if let pdf {
            printing.acquireSaveGate = { [weak self, lib] ids in
                guard let self, self.engineLibrary === lib else { return nil }
                return self.prepareForRecipeRead(imageIDs: ids, library: lib)
            }
            printing.prepare(items: visibleItems.filter { $0.engineImage != nil }, title: "Self-test", loader: loader)
            printing.settings.layout.style = .contactSheet
            printing.settings.layout.rows = 5
            printing.settings.layout.columns = 4
            let pages = printing.pageCount
            printing.run(.pdf(pdf), engine: lib.engine, window: nil) { ok in
                FileHandle.standardError.write(Data("print-selftest: \(ok ? "ok" : "FAILED"), \(pages) page(s) expected → \(pdf.path)\n".utf8))
                self.quitAfterSelfTest()
            }
        }
    }
}
