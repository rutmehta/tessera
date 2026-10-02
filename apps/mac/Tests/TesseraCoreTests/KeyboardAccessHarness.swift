import AppKit
import ObjectiveC

/// Process-local AppKit accessor override. No defaults domains or persistent preferences change.
/// Main-actor hosted tests are serialized; scopes restore the prior value even when tests throw.
@MainActor
enum KeyboardAccessHarness {
    static var override: Bool?
    static func install() {
        _ = installation
    }
    private static let installation: Void = {
        let original = class_getInstanceMethod(NSApplication.self,
            #selector(getter: NSApplication.isFullKeyboardAccessEnabled))!
        let replacement = class_getInstanceMethod(NSApplication.self,
            #selector(NSApplication.tesseraTestFullKeyboardAccess))!
        method_exchangeImplementations(original, replacement)
    }()
    static func withMode<T>(_ enabled: Bool, _ body: () throws -> T) rethrows -> T {
        install()
        let previous = override
        override = enabled
        defer { override = previous }
        return try body()
    }
}

extension NSApplication {
    @objc fileprivate func tesseraTestFullKeyboardAccess() -> Bool {
        KeyboardAccessHarness.override ?? tesseraTestFullKeyboardAccess()
    }
}
