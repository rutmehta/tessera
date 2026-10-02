import AppKit
import ObjectiveC
import os
@testable import Tessera

/// Process-local Full Keyboard Access pin (B5-49c). No defaults domain or persistent preference
/// changes. Main-actor hosted tests are serialized; scopes restore the prior value even when tests throw.
///
/// What a pin controls: the app's `KeyboardAccessPolicy`, `NSApplication.isFullKeyboardAccessEnabled`
/// and `NSButton.canBecomeKeyView` (so every plain or subclassed button without its own override).
/// What it cannot: AppKit decides the key-view membership of its other control classes, and SwiftUI
/// that of its focus proxies, from the real system setting through private state. A test that needs
/// those must say so (N/A with the reason) rather than assume either machine. `withSwiftUIProxies`
/// can move SwiftUI's proxies in or out of the key-view loop; walks that may cross such stops are
/// judged with `KeyViewWalk`, which does not assume how many there are.
@MainActor
enum KeyboardAccessHarness {
    /// What the exchanged getters read. They can be called by AppKit on any thread, so this is
    /// lock-protected state, never main-actor state (`KeyboardAccessPolicy.override` mirrors `pin`).
    private struct State {
        var pin: Bool?
        var proxies: Bool?
    }
    nonisolated private static let state = OSAllocatedUnfairLock(initialState: State())

    /// Stands in for the other machine's system setting in suites that do not pin a mode themselves:
    /// `TESSERA_TEST_SYSTEM_FKA=1|0`, or `-AppleKeyboardUIMode <n>` passed to the xctest process. AppKit
    /// itself ignores that argument-domain value (measured on macOS 26: the accessor and button
    /// eligibility do not change), so the harness applies it through the same pinned accessors.
    nonisolated static let simulatedSystem: Bool? = {
        if let value = ProcessInfo.processInfo.environment["TESSERA_TEST_SYSTEM_FKA"] {
            precondition(value == "0" || value == "1")
            return value == "1"
        }
        let args = ProcessInfo.processInfo.arguments
        if let index = args.firstIndex(of: "-AppleKeyboardUIMode"), args.indices.contains(index + 1) {
            return (Int(args[index + 1]) ?? 0) & 2 != 0
        }
        return nil
    }()
    nonisolated static var controlledMode: Bool? { state.withLock { $0.pin } ?? simulatedSystem }
    nonisolated fileprivate static var proxyMode: Bool? { state.withLock { $0.proxies } }
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
        state.withLock { $0.pin = enabled }
        defer {
            KeyboardAccessPolicy.override = previous
            state.withLock { $0.pin = previous }
        }
        return try body()
    }

    // MARK: SwiftUI focus proxies

    /// SwiftUI adds one `KeyViewProxy` view per focusable SwiftUI control and lets it take the keyboard
    /// only under the real system setting. Its `acceptsFirstResponder` is the gate AppKit's key-view
    /// loop asks. Nil when this SwiftUI has no such class or method.
    private static let proxyGate: Method? = {
        guard let proxy = NSClassFromString("SwiftUI.KeyViewProxy") else { return nil }
        let selector = #selector(getter: NSView.acceptsFirstResponder)
        var count: UInt32 = 0
        guard let methods = class_copyMethodList(proxy, &count) else { return nil }
        defer { free(methods) }
        // Only the class's own override: never replace NSView's implementation.
        guard let method = (0..<Int(count)).map({ methods[$0] }).first(where: { method_getName($0) == selector })
        else { return nil }
        typealias Getter = @convention(c) (AnyObject, Selector) -> Bool
        let original = unsafeBitCast(method_getImplementation(method), to: Getter.self)
        let block: @convention(block) (AnyObject) -> Bool = { view in
            KeyboardAccessHarness.proxyMode ?? original(view, selector)
        }
        method_setImplementation(method, imp_implementationWithBlock(block))
        return method
    }()
    static var canControlSwiftUIProxies: Bool { proxyGate != nil }

    /// Makes SwiftUI's focus proxies Tab stops (`true`, as on a machine whose real setting is on) or
    /// not (`false`, as on one where it is off) for a scope, on either machine. Returns nil without
    /// running `body` when the proxy class cannot be controlled.
    ///
    /// This moves the proxies in and out of AppKit's key-view loop. It does not change what SwiftUI
    /// itself believes the setting is, so the number of proxy stops a walk meets need not equal the
    /// number on a machine with the real setting.
    static func withSwiftUIProxies<T>(focusable: Bool, _ body: () throws -> T) rethrows -> T? {
        guard canControlSwiftUIProxies else { return nil }
        let previous = state.withLock { state in
            defer { state.proxies = focusable }
            return state.proxies
        }
        defer { state.withLock { $0.proxies = previous } }
        return try body()
    }
}

extension NSApplication {
    /// Exchanged with `isFullKeyboardAccessEnabled`. Reads no main-actor state: safe on any thread.
    @objc nonisolated fileprivate func tesseraTestFullKeyboardAccess() -> Bool {
        KeyboardAccessHarness.controlledMode ?? tesseraTestFullKeyboardAccess()
    }
}

extension NSButton {
    /// Exchanged with `canBecomeKeyView`. The pinned answer reads view state, so it is given only on
    /// the main thread; anywhere else the call goes straight to AppKit's own implementation.
    @objc nonisolated fileprivate func tesseraTestCanBecomeKeyView() -> Bool {
        guard Thread.isMainThread, let mode = KeyboardAccessHarness.controlledMode else { return tesseraTestCanBecomeKeyView() }
        return MainActor.assumeIsolated { mode && isEnabled && acceptsFirstResponder && !isHiddenOrHasHiddenAncestor }
    }
}

/// Fixed point geometry, independent of the attached display's size and backing scale.
@MainActor
final class KeyboardTestWindow: NSWindow {
    override func constrainFrameRect(_ frameRect: NSRect, to screen: NSScreen?) -> NSRect { frameRect }
}
