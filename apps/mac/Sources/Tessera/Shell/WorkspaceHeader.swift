import SwiftUI
import TesseraCore

/// Persistent, inexpensive description of the target; no rendering or FFI work in the view.
struct WorkspaceHeader: View {
    let model: AppModel
    private var source: String { model.source == .all ? model.library.title : model.source.title }

    private var target: String {
        if model.isReviewing { return "Review" + (model.selectedReviewEntry.map { " › \($0.name)" } ?? "") }
        if model.isReviewEditing { return "Review › " + (model.editTarget?.name ?? "Photo unavailable") }
        return model.source == .people ? "People" : source + (model.focusedItem.map { " › \($0.name)" } ?? "")
    }

    var body: some View {
        HStack(spacing: Theme.Space.m) {
            if model.isPhotoEditing || model.isReviewing {
                Button { model.isPhotoEditing ? model.returnFromPhotoEdit() : model.returnToLibrary() } label: {
                    Label(model.isReviewEditing ? "Back to Review" : "Back to Library", systemImage: "arrow.left")
                }
                .buttonStyle(.theme(.borderless, height: Theme.Height.regular))
                .help(model.isReviewEditing ? "Return to the same review photo and draft" : "Return to \(source), preserving your selection and position")
                .accessibilityIdentifier("workspace-back-to-library")
            }
            VStack(alignment: .leading, spacing: Theme.Space.xxs) {
                Text(target)
                    .font(Theme.Fonts.labelMedium).foregroundStyle(Theme.textPrimary)
                    .lineLimit(1).truncationMode(.middle)
                    .help(target)
                    .accessibilityIdentifier("workspace-photo-target")
                Text(model.workspaceScope)
                    .font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                    .lineLimit(1).help(model.workspaceScope)
                    .accessibilityIdentifier("workspace-command-scope")
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            if !model.isPhotoEditing {
                Button("Edit photo") { model.enterPhotoEdit() }
                    .buttonStyle(.theme(.bordered, height: Theme.Height.regular))
                    .disabled(!model.canEnterPhotoEdit)
                    .help(model.canEnterPhotoEdit ? "Develop the focused photo (D)" : "Choose a photo to edit")
                    .accessibilityIdentifier("workspace-edit-photo")
            }
        }
        .padding(.horizontal, Theme.Space.gutter)
        .padding(.vertical, Theme.Space.s)
        .background(Theme.panel)
        Hairline()
    }
}

struct LayeredCopyRequest: Identifiable {
    let id = UUID()
    let item: PhotoItem
    let library: EngineLibrary?
    init(item: PhotoItem, library: EngineLibrary? = nil) {
        self.item = item
        self.library = library
    }
}

/// The existing engine handoff is a rendered copy, not a live RAW layer.
struct LayeredCopySheet: View {
    let model: AppModel
    let request: LayeredCopyRequest
    var body: some View {
        SheetScaffold(title: "Open in Layers",
                      subtitle: request.item.name) {
            EmptyView()
        } content: {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                Text("The first open creates a rendered copy with the photo’s current adjustments.")
                    .font(Theme.Fonts.label).foregroundStyle(Theme.textPrimary)
                Text("If a layered copy is already open, it reopens with its existing pixels and layer edits. Later photo adjustments do not refresh that copy. The original photo and its Develop settings stay separate; RAW development is not live in the pixel layer. Save the layered document separately.")
                    .font(Theme.Fonts.label).foregroundStyle(Theme.textSecondary)
            }
        } leading: {
            EmptyView()
        } actions: {
            Button("Cancel") { model.layeredCopyRequest = nil }
                .keyboardShortcut(.cancelAction).sheetButton()
            Button("Open in Layers") { model.createRequestedLayeredCopy() }
                .keyboardShortcut(.defaultAction).sheetButton(primary: true)
                .accessibilityIdentifier("workspace-create-layered-copy")
        }
        .frame(width: 480)
    }
}
