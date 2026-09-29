import AppKit

extension DevelopRecoveryCoordinator {
    /// What `applicationShouldTerminate` admits: no Develop session still open or saving,
    /// and no saved-read gate (Export, Print, …) still held.
    var allowsTermination: Bool { !hasUnresolvedSessions && !hasActiveReservations }
}

extension AppModel {
    /// Quit at the end of a hidden-flag self-test (`--export-selftest`, `--print-pdf-selftest`,
    /// `--timing-selftest`). Export/Print call their finish callback before the job's `defer`
    /// releases its saved-read gate, and the timing run leaves the loupe's Develop session open;
    /// `applicationShouldTerminate` refuses both. So quit on a later main-actor turn (after the
    /// calling job, and its `defer`, has finished) and close any Develop session first.
    func quitAfterSelfTest(_ terminate: @escaping @MainActor () -> Void = { NSApp.terminate(nil) }) {
        Task { @MainActor in
            if let close = closeDevelop() { _ = await close.value }
            terminate()
        }
    }
}
