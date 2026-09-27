import AppKit
import TesseraCore
import TesseraFFI

/// Photo ▸ Photo Merge and Photo ▸ Enhance (WP M2-50): what they apply to, when they are
/// available, and what happens when a job ends (the new DNGs join the grid, selected).
extension AppModel {
    /// Engine image ids of the photos the command applies to, in grid order.
    var photoTargetImageIDs: [String] {
        targetIDs.compactMap { library.items.indices.contains($0) ? library.items[$0].engineImage?.imageID : nil }
    }

    /// Observed count for menu enablement (the selection, else the focused photo).
    var photoTargetCount: Int { selectionCount > 0 ? selectionCount : (focusedItem == nil ? 0 : 1) }

    private var photoCommandsAvailable: Bool { viewMode != .document && source != .people }

    func canPhotoMerge(_ kind: PhotoMergeKind) -> Bool {
        photoCommandsAvailable && PhotoCommandRules.canMerge(kind, selected: photoTargetCount,
                                                             engineBacked: isEngineBacked, running: photoJobs.isRunning)
    }

    var canEnhance: Bool {
        photoCommandsAvailable && PhotoCommandRules.canEnhance(selected: photoTargetCount,
                                                               engineBacked: isEngineBacked, running: photoJobs.isRunning)
    }

    func presentPhotoMerge(_ kind: PhotoMergeKind) {
        let ids = photoTargetImageIDs
        if let problem = PhotoCommandRules.mergeProblem(kind, selected: ids.count, engineBacked: engineLibrary != nil,
                                                        running: photoJobs.isRunning) {
            statusMessage = problem
            return
        }
        guard let lib = engineLibrary else { return }
        preparePhotoJobs(lib)
        photoJobs.presentMerge(kind, imageIDs: ids, title: photoTargetTitle(ids.count))
    }

    func presentEnhance() {
        let ids = photoTargetImageIDs
        if let problem = PhotoCommandRules.enhanceProblem(selected: ids.count, engineBacked: engineLibrary != nil,
                                                          running: photoJobs.isRunning) {
            statusMessage = problem
            return
        }
        guard let lib = engineLibrary else { return }
        preparePhotoJobs(lib)
        photoJobs.presentEnhance(imageIDs: ids, title: photoTargetTitle(ids.count))
    }

    private func photoTargetTitle(_ count: Int) -> String {
        let items = selectedItems
        guard let first = items.first else { return "" }
        return count == 1 ? first.name : "\(first.name) and \(count - 1) more"
    }

    /// Points the controller at the open library and flushes pending develop edits (the engine
    /// reads the sources' recipes).
    private func preparePhotoJobs(_ lib: EngineLibrary) {
        if let d = develop { try? d.session.flush() }
        var metadata: (@Sendable (String) -> [(name: String, value: String)]?)?
        if let store = collections.catalog?.store {
            metadata = { id in
                guard let m = try? store.metadata(imageId: id) else { return nil }
                return m.fields.map { (name: $0.name, value: $0.value) }
            }
        }
        photoJobs.backend = EnginePhotoBackend(engine: lib.engine, metadata: metadata)
        photoJobs.onFinish = { [weak self] outcome in self?.photoJobDidFinish(outcome) }
    }

    /// Toast and status; published DNGs (also those of a job that failed or was cancelled later)
    /// are pulled into the grid through the change feed and selected.
    func photoJobDidFinish(_ outcome: PhotoJobController.Outcome) {
        let names = outcome.outputs.map { URL(fileURLWithPath: $0.path).lastPathComponent }
        let what = outcome.operation.title
        let message: String
        var details: [String] = names
        switch outcome.state {
        case .completed:
            if case .merge = outcome.operation {
                message = "\(what): created \(names.first ?? "a DNG")"
            } else {
                message = "Enhanced \(names.count) photo\(names.count == 1 ? "" : "s")"
            }
            if let stack = photoStackLine(outcome) { details.append(stack) }
        case .cancelled:
            message = names.isEmpty ? "\(what) cancelled" : "\(what) cancelled after \(names.count) photo\(names.count == 1 ? "" : "s")"
        case .failed, .running:
            message = "\(what) failed: \(outcome.explanation ?? "unknown error")"
            if let stage = outcome.stage { details.append("Stage: \(PhotoJobStage.title(stage))") }
        }
        statusMessage = message
        showToast(message, undoable: false, details: details)
        guard !outcome.outputs.isEmpty else { return }
        let ids = outcome.outputIDs
        syncLibrary { [weak self] in self?.selectPhotoOutputs(ids) }
    }

    /// "Stacked with 3 source photos" from the engine's persistent stack.
    private func photoStackLine(_ outcome: PhotoJobController.Outcome) -> String? {
        guard let lib = engineLibrary, let first = outcome.outputs.first,
              let stack = try? lib.engine.photoStack(imageId: first.imageId), stack.count > 1 else { return nil }
        return "Stacked with \(stack.count - 1) source photo\(stack.count == 2 ? "" : "s")"
    }

    /// Selects the new photos (admitting them if the current filter would hide them).
    func selectPhotoOutputs(_ imageIDs: [String]) {
        guard let lib = engineLibrary else { return }
        let items = imageIDs.compactMap { lib.itemOfImage[$0] }
        guard !items.isEmpty else { return }
        let hidden = items.filter { !visible.contains($0) }
        if !hidden.isEmpty { admit(hidden) }
        let positions = IndexSet(items.compactMap { visible.firstIndex(of: $0) })
        guard let first = positions.first else { return }
        select(position: first)
        if positions.count > 1 { setSelectionFromUI(positions, clicked: first) }
    }
}
