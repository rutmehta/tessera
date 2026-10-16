import SwiftUI
import TesseraCore

/// The breadcrumb header is gone (UX-11: the toolbar is the only bar; the status bar names the
/// position and the window title the folder). What remains is the one notice that changes what
/// the person can do: the drive is not attached, so the folder is open on its offline previews
/// and culling waits (OF-2 copy: `OfflineCopy.banner`).
struct CachedPreviewNotice: View {
    let model: AppModel

    var body: some View {
        HStack(spacing: Theme.Space.m) {
            Text(model.offlineBanner)
                .font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                .lineLimit(1).truncationMode(.tail)
                .accessibilityIdentifier("cached-preview-library-mode")
                .help(OfflineCopy.bannerHelp)
            Spacer(minLength: Theme.Space.m)
            Button("Reopen Library") { model.reopenCachedPreviewLibrary() }
                .buttonStyle(.theme(.bordered, height: Theme.Height.regular))
                .disabled(model.isLoading)
                .accessibilityIdentifier("reopen-cached-preview-library")
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
