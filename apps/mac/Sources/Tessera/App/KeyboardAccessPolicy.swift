import AppKit

/// The app's single Full Keyboard Access input. Nil follows the system dynamically.
/// Responder ownership (Tab routing and keyboard data safety) is intentionally mode-independent:
/// a control that already owns focus retains its keys in either mode.
@MainActor
enum KeyboardAccessPolicy {
    static var override: Bool?
    static var isEnabled: Bool { override ?? NSApplication.shared.isFullKeyboardAccessEnabled }
}
