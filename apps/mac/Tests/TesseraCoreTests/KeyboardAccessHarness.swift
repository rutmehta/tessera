import AppKit
import ObjectiveC
@testable import Tessera

/// Process-local AppKit accessor override. No defaults domains or persistent preferences change.
/// Main-actor hosted tests are serialized; scopes restore the prior value even when tests throw.
@MainActor
enum KeyboardAccessHarness {
    static var simulatedSystem: Bool? {
        if let value = ProcessInfo.processInfo.environment["TESSERA_TEST_SYSTEM_FKA"] {
            precondition(value == "0" || value == "1")
            return value == "1"
        }
        let args = ProcessInfo.processInfo.arguments
        if let index = args.firstIndex(of: "-AppleKeyboardUIMode"), args.indices.contains(index + 1) {
            return (Int(args[index + 1]) ?? 0) & 2 != 0
        }
        return nil
    }
    static var controlledMode: Bool? { KeyboardAccessPolicy.override ?? simulatedSystem }
    static func install() {
        _ = installation
    }
    /// The machine's real setting as AppKit reports it, bypassing every pin. Reporting only.
    static var systemFullKeyboardAccess: Bool {
        install()
        return NSApplication.shared.tesseraTestFullKeyboardAccess()
    }
    private static let installation: Void = {
        let original = class_getInstanceMethod(NSApplication.self,
            #selector(getter: NSApplication.isFullKeyboardAccessEnabled))!
        let replacement = class_getInstanceMethod(NSApplication.self,
            #selector(NSApplication.tesseraTestFullKeyboardAccess))!
        method_exchangeImplementations(original, replacement)
        // AppKit's NSButton key-view eligibility uses a cached preference internally, bypassing
        // NSApplication's public accessor. Override that boundary too, in the test bundle only.
        let selector = #selector(getter: NSButton.canBecomeKeyView)
        let alternate = #selector(NSButton.tesseraTestCanBecomeKeyView)
        let originalButton = class_getInstanceMethod(NSButton.self, selector)!
        let replacementButton = class_getInstanceMethod(NSButton.self, alternate)!
        if class_addMethod(NSButton.self, selector, method_getImplementation(replacementButton),
                           method_getTypeEncoding(replacementButton)) {
            class_replaceMethod(NSButton.self, alternate, method_getImplementation(originalButton),
                                method_getTypeEncoding(originalButton))
        } else {
            method_exchangeImplementations(originalButton, replacementButton)
        }
    }()
    static func withMode<T>(_ enabled: Bool, _ body: () throws -> T) rethrows -> T {
        install()
        let previous = KeyboardAccessPolicy.override
        KeyboardAccessPolicy.override = enabled
        defer { KeyboardAccessPolicy.override = previous }
        return try body()
    }
}

extension NSApplication {
    @objc fileprivate func tesseraTestFullKeyboardAccess() -> Bool {
        KeyboardAccessHarness.controlledMode ?? tesseraTestFullKeyboardAccess()
    }
}

extension NSButton {
    @objc fileprivate func tesseraTestCanBecomeKeyView() -> Bool {
        guard let mode = KeyboardAccessHarness.controlledMode else { return tesseraTestCanBecomeKeyView() }
        return mode && isEnabled && acceptsFirstResponder && !isHiddenOrHasHiddenAncestor
    }
}

/// Fixed point geometry, independent of the attached display's size and backing scale.
@MainActor
final class KeyboardTestWindow: NSWindow {
    override func constrainFrameRect(_ frameRect: NSRect, to screen: NSScreen?) -> NSRect { frameRect }
}
