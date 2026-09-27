import Foundation
import ObjectiveC

/// Per-user state under `--app-dir` (WP M2-56). With `--app-dir <dir>` or `TESSERA_APP_DIR` given, the
/// app's user defaults — the folder registry (last and recent folders), basket target, panel states,
/// window frames, every `@AppStorage` — live in `<dir>/Preferences.plist` instead of the shared
/// per-user domain. An instance started with a scratch app dir then neither lists nor changes the
/// folders of the person's library or of other instances (Machine B saw a fresh `--app-dir` instance
/// list other agents' folders).
///
/// `UserDefaults(suiteName:)` with an absolute path stores that plist and, unlike the app's own
/// domain, is not shared: it still reads the global domain (locale, system settings) and launch
/// arguments. `UserDefaults.standard` is redirected to it for the process, so no call site changes.
/// Without an explicit app dir nothing is changed.
public enum AppDefaultsIsolation {
    public static let fileName = "Preferences.plist"
    nonisolated(unsafe) private static var isolated: UserDefaults?

    /// The isolated defaults, when installed.
    public static var current: UserDefaults? { isolated }

    /// The app dir only when one was given explicitly (not the Application Support default).
    public static func explicitAppDirectory(arguments: [String] = ProcessInfo.processInfo.arguments,
                                            environment: [String: String] = ProcessInfo.processInfo.environment) -> URL? {
        let hasFlag = arguments.firstIndex(of: "--app-dir").map { arguments.indices.contains($0 + 1) } ?? false
        let hasEnv = !(environment["TESSERA_APP_DIR"] ?? "").isEmpty
        guard hasFlag || hasEnv else { return nil }
        return EngineLibrary.supportDirectory(arguments: arguments, environment: environment)
    }

    /// Defaults stored in `<directory>/Preferences.plist` only.
    public static func defaults(in directory: URL) -> UserDefaults? {
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let path = directory.appendingPathComponent(fileName).deletingPathExtension().path
        return UserDefaults(suiteName: path)
    }

    /// Redirects `UserDefaults.standard` to the app dir's store when an app dir was given. Call first
    /// thing at launch, before anything reads user defaults (`TesseraApp.init`). Idempotent.
    @discardableResult
    public static func installForLaunch(arguments: [String] = ProcessInfo.processInfo.arguments,
                                        environment: [String: String] = ProcessInfo.processInfo.environment) -> UserDefaults? {
        if let isolated { return isolated }
        guard let dir = explicitAppDirectory(arguments: arguments, environment: environment),
              let store = defaults(in: dir) else { return nil }
        isolated = store
        guard let standard = class_getClassMethod(UserDefaults.self, #selector(getter: UserDefaults.standard)),
              let replacement = class_getClassMethod(IsolatedStandardDefaults.self, #selector(IsolatedStandardDefaults.isolatedStandard))
        else { return store }
        method_setImplementation(standard, method_getImplementation(replacement))
        return store
    }
}

private final class IsolatedStandardDefaults: NSObject {
    @objc class func isolatedStandard() -> UserDefaults {
        AppDefaultsIsolation.current ?? UserDefaults(suiteName: nil)!
    }
}
