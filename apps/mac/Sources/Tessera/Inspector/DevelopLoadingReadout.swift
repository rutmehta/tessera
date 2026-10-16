import Foundation

/// The Edits readout while a photo's develop session opens (UX-15 item 19): "Rendering…" for the
/// first two seconds, then why it is taking long, counted in whole seconds and updated each
/// second ("Rendering… · Waiting for the engine · 12 s"). The loupe shows the embedded preview
/// meanwhile; nothing floats over the panel.
enum DevelopLoadingReadout {
    /// Seconds before the reason is appended.
    static let reasonAfter: TimeInterval = 2

    static func text(since start: Date, now: Date = Date()) -> String {
        let elapsed = max(0, now.timeIntervalSince(start))
        guard elapsed >= reasonAfter else { return "Rendering…" }
        return "Rendering… · Waiting for the engine · \(Int(elapsed.rounded(.down))) s"
    }
}
