import SwiftUI
import TesseraCore

/// Settings ▸ Offline (OF-2, docs/design/offline/SPEC.md): which photos get an offline preview
/// while their drive is attached, the disk budget and what it fits, the space used, a button that
/// makes the open folder's previews now, and one line on what the builder is doing.
struct OfflineSettingsView: View {
    let model: AppModel

    private static let minimumGigabytes = 1.0
    private static let maximumGigabytes = 2000.0

    var body: some View {
        let previews = model.offlinePreviews
        Form {
            Section("Offline previews") {
                Picker("Make previews for", selection: Binding(get: { previews.mode }, set: { previews.setMode($0) })) {
                    ForEach(OfflinePreviewsMode.allCases, id: \.self) { Text($0.title).tag($0) }
                }
                .accessibilityIdentifier("offline-mode")
                Hint("While a drive is attached, Tessera makes a preview for each RAW in the folders you open, newest and kept photos first, in the background. With the drive gone, Edit opens the preview; edits are written to the drive when it is back.")
            }
            Section("Budget") {
                LabeledContent("Disk budget") {
                    Stepper(value: gigabytes, in: Self.minimumGigabytes...Self.maximumGigabytes, step: 1) {
                        Text("\(Int(previews.budget / OfflinePreviewsSettings.gigabyte)) GB")
                            .font(Theme.Fonts.bodyNumeric).foregroundStyle(Theme.textPrimary)
                    }
                    .accessibilityIdentifier("offline-budget")
                }
                Hint(OfflinePreviewsSettings.fitsText(budget: previews.budget) + " on the boot volume; the default is 10 % of the free space, at most 25 GB.")
                LabeledContent("Used") {
                    Text(previews.usageText.replacingOccurrences(of: "Used: ", with: ""))
                        .font(Theme.Fonts.bodyNumeric).foregroundStyle(Theme.textSecondary)
                        .accessibilityIdentifier("offline-usage")
                }
            }
            Section("This folder") {
                LabeledContent("Folder") {
                    Text(model.library.folder?.lastPathComponent ?? "No folder open")
                        .font(Theme.Fonts.body)
                        .foregroundStyle(model.library.folder == nil ? Theme.textTertiary : Theme.textSecondary)
                        .lineLimit(1).truncationMode(.middle)
                }
                if let coverage = previews.coverageText {
                    LabeledContent("Editable offline") {
                        Text(coverage).font(Theme.Fonts.bodyNumeric).foregroundStyle(Theme.textSecondary)
                            .accessibilityIdentifier("offline-coverage")
                    }
                }
                Button("Make this folder available offline now") { model.makeFolderAvailableOffline() }
                    .disabled(!previews.canBuild || previews.isBuilding)
                    .accessibilityIdentifier("offline-build-now")
                Hint(previews.canBuild
                     ? "Runs right away, without waiting for the app to be idle, until the folder is covered or the budget is reached."
                     : "Open a folder on an attached drive first.")
                LabeledContent("Builder") {
                    Text(previews.statusText)
                        .font(Theme.Fonts.body).foregroundStyle(Theme.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("offline-status")
                }
            }
        }
        .formStyle(.grouped)
        .frame(width: 520, height: 560)
        .tint(Theme.accent)
        .onAppear { previews.refreshCoverage() }
    }

    private var gigabytes: Binding<Double> {
        Binding(get: { Double(model.offlinePreviews.budget) / Double(OfflinePreviewsSettings.gigabyte) },
                set: { model.offlinePreviews.setBudget(UInt64(max(Self.minimumGigabytes, $0)) * OfflinePreviewsSettings.gigabyte) })
    }
}
