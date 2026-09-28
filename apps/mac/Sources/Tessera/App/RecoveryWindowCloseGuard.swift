import AppKit

/// Intercepts only the Tessera main window's close decision. SwiftUI still owns
/// its window delegate callbacks through forwarding to the delegate we replaced.
@MainActor
final class RecoveryWindowCloseGuard: NSObject, NSWindowDelegate {
    private static var installed: [ObjectIdentifier: RecoveryWindowCloseGuard] = [:]

    private weak var window: NSWindow?
    nonisolated(unsafe) private weak var previous: (any NSWindowDelegate)?
    private let shouldBlock: @MainActor () -> Bool
    private let blocked: @MainActor (NSWindow) -> Void
    private var closeObserver: NSObjectProtocol?

    private init(window: NSWindow, previous: (any NSWindowDelegate)?,
                 shouldBlock: @escaping @MainActor () -> Bool,
                 blocked: @escaping @MainActor (NSWindow) -> Void) {
        self.window = window
        self.previous = previous
        self.shouldBlock = shouldBlock
        self.blocked = blocked
        super.init()
    }

    /// Called only by the NSViewRepresentable hosted in ContentView's main window.
    static func install(on window: NSWindow) {
        let id = ObjectIdentifier(window)
        if let guardDelegate = installed[id] {
            guard window.delegate !== guardDelegate else { return }
            // SwiftUI may replace its delegate after the view first appears.
            // Capture the new one, never our former proxy as a forwarding target.
            if let replacement = window.delegate, replacement !== guardDelegate,
               !(replacement is RecoveryWindowCloseGuard) {
                guardDelegate.previous = replacement
            }
            window.delegate = guardDelegate
            return
        }
        let guardDelegate = RecoveryWindowCloseGuard(window: window, previous: window.delegate,
            shouldBlock: {
                let recovery = AppModel.shared.developRecovery
                return recovery.hasUnresolvedSessions || recovery.hasActiveReservations
            }, blocked: { window in
                AppModel.shared.statusMessage = "Finish the current photo save or operation before closing the window"
                window.makeKeyAndOrderFront(nil)
            })
        installed[id] = guardDelegate
        guardDelegate.closeObserver = NotificationCenter.default.addObserver(
            forName: NSWindow.willCloseNotification, object: window, queue: .main
        ) { [weak guardDelegate] _ in
            MainActor.assumeIsolated { guardDelegate?.uninstall() }
        }
        window.delegate = guardDelegate
    }

    /// Internal test seam; production always uses `install(on:)` above.
    static func testing(window: NSWindow, previous: (any NSWindowDelegate)?,
                        shouldBlock: @escaping @MainActor () -> Bool,
                        blocked: @escaping @MainActor (NSWindow) -> Void) -> RecoveryWindowCloseGuard {
        RecoveryWindowCloseGuard(window: window, previous: previous,
                                 shouldBlock: shouldBlock, blocked: blocked)
    }

    func windowShouldClose(_ sender: NSWindow) -> Bool {
        guard sender === window else { return previous?.windowShouldClose?(sender) ?? true }
        if shouldBlock() {
            blocked(sender)
            return false
        }
        return previous?.windowShouldClose?(sender) ?? true
    }

    func windowWillClose(_ notification: Notification) {
        previous?.windowWillClose?(notification)
        uninstall()
    }

    private func uninstall() {
        if let closeObserver {
            NotificationCenter.default.removeObserver(closeObserver)
            self.closeObserver = nil
        }
        if let window {
            if window.delegate === self { window.delegate = previous }
            Self.installed.removeValue(forKey: ObjectIdentifier(window))
        }
    }

    nonisolated override func responds(to selector: Selector!) -> Bool {
        super.responds(to: selector) || (previous as? NSObject)?.responds(to: selector) == true
    }

    nonisolated override func forwardingTarget(for selector: Selector!) -> Any? {
        if let prior = previous as? NSObject, prior.responds(to: selector) { return prior }
        return super.forwardingTarget(for: selector)
    }
}
