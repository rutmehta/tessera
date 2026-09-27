import SwiftUI
import TesseraCore

/// Settings ▸ AI ▸ Develop models (M2-51): whether AI Denoise and Lens Blur may download their
/// pinned weights on first use. Off keeps everything on this Mac: models already in the cache
/// still work, missing ones fail with the reason in the panel.
struct ModelDownloadsSettingsSection: View {
    @Bindable var models: ModelAcquisition = .shared

    var body: some View {
        Section("Develop models") {
            Toggle("Allow model downloads", isOn: $models.allowDownloads)
                .accessibilityIdentifier("ai-allow-model-downloads")
            Hint("AI Denoise and Lens Blur fetch their pinned, checksum-verified models the first time you use them. Off: only models already on this Mac are used.")
        }
    }
}
