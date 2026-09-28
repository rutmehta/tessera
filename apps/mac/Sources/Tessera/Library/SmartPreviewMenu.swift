import AppKit
import SwiftUI
import TesseraCore

/// Bounded status reads happen on selection; menus and thumbnail drawing only read
/// cached observable state. No automatic catalog-wide builds or polling.
struct SmartPreviewMenu: View {
    let model: AppModel
    var body: some View {
        Menu("Smart Previews") {
            Toggle("Use Smart Previews When Available", isOn: Binding(
                get: { model.preferSmartPreviews }, set: { model.setPreferSmartPreviews($0) }))
                .accessibilityIdentifier("use-smart-previews")
            Text("Source changes save and close the current editor first")
            if let route = model.developSourceRoute {
                Text("Editing source: \(route.label)").accessibilityIdentifier("develop-source-badge")
            }
            if let info = model.smartPreviews.selectedInfo {
                Text(info.badge).accessibilityLabel(info.badge)
                if !info.message.isEmpty { Text(info.message) }
            }
            if let error = model.smartPreviews.selectionError { Text(error) }
            Divider()
            ForEach(SmartPreviewController.Action.allCases, id: \.self) { action in
                Button("\(action.rawValue) Smart Previews for Selected RAW Photos") {
                    model.runSmartPreviewBatch(action)
                }
                .disabled(model.viewMode == .document || model.smartPreviewBatchActive || !model.isEngineBacked || model.focusedItem == nil || model.isReviewing)
                .accessibilityIdentifier("smart-preview-\(action.rawValue.lowercased())")
            }
            Text(model.smartPreviewBatchActive && !model.smartPreviews.isRunning
                 ? "Waiting for photo save…" : model.smartPreviews.progressLabel)
            Button("Cancel After Current Photo") { model.cancelSmartPreviewBatch() }
                .disabled(!model.smartPreviewBatchActive || model.smartPreviewCancelRequested)
                .help("The current native operation finishes before cancellation takes effect.")
            Menu("Last Batch Results") {
                ForEach(Array(model.smartPreviews.results.prefix(100))) { result in
                    Text("\(result.name): \(result.succeeded ? "Succeeded" : "Not completed") · \(result.message)")
                }
                if model.smartPreviews.results.count > 100 { Text("Showing first 100 photos; copy results for all photos") }
                Button("Copy All Batch Results") {
                    let report = model.smartPreviews.results.map {
                        "\($0.name): \($0.succeeded ? "Succeeded" : "Not completed") · \($0.message)"
                    }.joined(separator: "\n")
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(report, forType: .string)
                }
            }.disabled(model.smartPreviews.results.isEmpty)
            Divider()
            Text("Full-quality export requires the matching original and synchronized edits")
        }
    }
}
