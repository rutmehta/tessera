import SwiftUI
import TesseraCore
import TesseraFFI

/// The Photo menu (WP M2-50): Photo Merge ▸ HDR… (⌃H) / Panorama… (⌃M) / HDR Panorama…, and
/// Enhance… (⌃⌥I). Merge needs 2+ selected photos (4+ for HDR Panorama), Enhance 1+.
struct PhotoMenuItems: View {
    let model: AppModel

    var body: some View {
        Menu("Photo Merge") {
            Button("HDR…") { model.presentPhotoMerge(.hdr) }
                .keyboardShortcut("h", modifiers: .control)
                .disabled(!model.canPhotoMerge(.hdr))
            Button("Panorama…") { model.presentPhotoMerge(.panorama) }
                .keyboardShortcut("m", modifiers: .control)
                .disabled(!model.canPhotoMerge(.panorama))
            Button("HDR Panorama…") { model.presentPhotoMerge(.hdrPanorama) }
                .disabled(!model.canPhotoMerge(.hdrPanorama))
        }
        Button("Enhance…") { model.presentEnhance() }
            .keyboardShortcut("i", modifiers: [.control, .option])
            .disabled(!model.canEnhance)
        Divider()
        Button("Cancel \(model.photoJobs.running?.title ?? "Photo Merge")") { model.photoJobs.cancel() }
            .disabled(!model.photoJobs.isRunning)
    }
}

/// Photo Merge / Enhance progress above the status bar (the activity area), with Cancel.
struct PhotoJobProgressBar: View {
    let jobs: PhotoJobController

    var body: some View {
        if let p = jobs.progress {
            // Model downloads report only start and ready: an indeterminate bar, not a fake percentage.
            let determinate = p.downloadingModel == nil && p.total > 0
            ProgressStrip(title: jobs.runningTitle, done: determinate ? p.done : 0, total: determinate ? p.total : 0,
                          current: p.downloadingModel != nil ? "\(p.title)…" : p.title) {
                Button("Cancel") { jobs.cancel() }
                    .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                    .accessibilityIdentifier("photo-job-cancel")
            }
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("photo-job-progress")
        }
    }
}

extension View {
    /// The Photo Merge and Enhance sheets (attached once, in ContentView).
    func photoJobSheets(_ model: AppModel) -> some View {
        let jobs = model.photoJobs
        return self
            .sheet(item: Binding(get: { jobs.mergeSheet }, set: { if $0 == nil { jobs.dismissSheets() } else { jobs.mergeSheet = $0 } })) { _ in
                PhotoMergeSheet(jobs: jobs)
            }
            .sheet(isPresented: Binding(get: { jobs.showEnhance }, set: { if !$0 { jobs.dismissSheets() } })) {
                EnhanceSheet(jobs: jobs)
            }
    }
}
