import AppKit

extension DevelopRecoveryCoordinator {
    /// What `applicationShouldTerminate` admits: no Develop session still open or saving,
    /// and no saved-read gate (Export, Print, …) still held.
    var allowsTermination: Bool { !hasUnresolvedSessions && !hasActiveReservations }
}

extension AppModel {
    /// Quit at the end of a hidden-flag self-test (`--export-selftest`, `--print-pdf-selftest`,
    /// `--timing-selftest`).
    func quitAfterSelfTest(_ terminate: @escaping @MainActor () -> Void = { NSApp.terminate(nil) }) {
        terminate()
    }
}
