import AppKit
import TesseraCore
import SwiftUI
import Sparkle

@main
struct TesseraApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
    private let model = AppModel.shared

    var body: some Scene {
        Window("Tessera", id: "main") {
            ContentView(model: model)
                .frame(minWidth: 960, minHeight: 600)
        }
        .defaultSize(width: 1440, height: 900)
        .commands {
            AppCommands(model: model)
            CommandGroup(after: .appInfo) {
                Button("Check for Updates…") { appDelegate.updaterController?.checkForUpdates(nil) }
                    .disabled(!appDelegate.updatesConfigured)
                    .help(appDelegate.updatesConfigured ? "Check for updates" : "updates not configured for this build")
            }
        }
    }
}

/// Launch handling: activation (also when run as a bare SwiftPM executable), dark appearance,
/// the culling key monitor, and the initial library.
///
/// Launch arguments (used by the acceptance script and benchmarks):
///   --folder <path>   open this folder instead of the remembered one
///   --app-dir <path>  store the index and caches here (overrides TESSERA_APP_DIR)
///   --stub <count>    load <count> synthetic items (e.g. 20000)
///   --benchmark       run the grid scroll benchmark after loading
///   --seed-scores     (hidden test aid) write deterministic synthetic focus / closed-eyes scores
///                     into the index after opening a folder, for the defect sweep
///   --import-lrcat <catalog.lrcat>  open File ▸ Import Lightroom Catalog… with this catalog chosen
///   --front           order the window front without activating (screenshots while another app is active)
///   --develop-selftest  once a develop session opens, drag Exposure 0 → +1.5 through the slider path
///                     (60 display-rate steps, then mouse-up) and print frame timings to stderr
///   --develop-panels-selftest  open the first photo in the loupe and drag one control of each develop
///                     panel likewise (TESSERA_SELFTEST_CROP=1|commit also opens/applies a crop)
///   --keys "<k> <k>…" after loading, feed these keys through the culling key map (self-test aid);
///                     tokens: single characters, left right up down return esc delete,
///                     prefixes "opt-" / "shift-" / "cmd-" (⌘ tokens go to the menu bar); "wait" idles one step
@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    // Sparkle reads the feed, public key, and daily-check defaults from Info.plist.
    lazy var updatesConfigured: Bool = {
        let info = Bundle.main.infoDictionary
        let configured = ["SUPublicEDKey", "SUFeedURL"].allSatisfy {
            guard let value = info?[$0] as? String else { return false }
            return !value.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        }
        if !configured { NSLog("Sparkle updates not configured for this build") }
        return configured
    }()
    lazy var updaterController: SPUStandardUpdaterController? = {
        guard updatesConfigured else { return nil }
        return SPUStandardUpdaterController(
            startingUpdater: true, updaterDelegate: nil, userDriverDelegate: nil)
    }()
    private var keyRouter: KeyRouter?

    func applicationWillFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.regular)
        NSApp.appearance = NSAppearance(named: .darkAqua)
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        _ = updaterController // Start scheduled checks only for configured builds.
        let model = AppModel.shared
        keyRouter = KeyRouter(model: model)
        keyRouter?.install()
        NSApp.activate()

        let args = ProcessInfo.processInfo.arguments
        if args.contains("--develop-selftest"), args.contains("--bundle-selftest") {
            // Packaging smoke test: exercise the real launch path without a photo fixture.
            DispatchQueue.main.asyncAfter(deadline: .now() + 1) {
                FileHandle.standardError.write(Data("bundle-selftest: launched\n".utf8))
            }
            DispatchQueue.main.asyncAfter(deadline: .now() + 3) { NSApp.terminate(nil) }
        }
        func value(after flag: String) -> String? {
            guard let i = args.firstIndex(of: flag), i + 1 < args.count else { return nil }
            return args[i + 1]
        }
        if let n = value(after: "--stub").flatMap(Int.init) {
            model.loadStubItems(count: n)
        } else if let path = value(after: "--folder") {
            model.openFolder(URL(fileURLWithPath: (path as NSString).expandingTildeInPath, isDirectory: true))
        } else if let last = model.lastFolder, FileManager.default.fileExists(atPath: last.path) {
            model.openFolder(last)
        }
        if let catalog = value(after: "--import-lrcat") {
            // Test aid: open File ▸ Import Lightroom Catalog… with this catalog already chosen.
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.8) {
                MainActor.assumeIsolated {
                    model.presentLightroomImport(catalog: URL(fileURLWithPath: (catalog as NSString).expandingTildeInPath))
                }
            }
        }
        if args.contains("--front") {   // test aid: show the window without activating the app
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
                MainActor.assumeIsolated { NSApp.windows.first { !($0 is NSPanel) }?.orderFrontRegardless() }
            }
        }
        if let keys = value(after: "--keys") {
            // After the library has loaded; one token every 0.3 s so menus and views update.
            waitForLibrary(then: keys.split(separator: " ").map(String.init))
        }
        if args.contains("--benchmark") {
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) {
                MainActor.assumeIsolated { model.requestScrollBenchmark() }
            }
        }
    }

    private func waitForLibrary(then tokens: [String], polls: Int = 0) {
        DispatchQueue.main.asyncAfter(deadline: .now() + (polls == 0 ? 1.0 : 0.25)) {
            MainActor.assumeIsolated {
                let model = AppModel.shared
                if (model.isLoading || model.library.items.isEmpty) && polls < 60 {
                    self.waitForLibrary(then: tokens, polls: polls + 1)
                } else {
                    self.simulate(tokens: tokens[...])
                }
            }
        }
    }

    private func simulate(tokens: ArraySlice<String>) {
        guard let token = tokens.first else { return }
        simulate(keys: token)
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) {
            MainActor.assumeIsolated { self.simulate(tokens: tokens.dropFirst()) }
        }
    }

    private func simulate(keys: String) {
        guard let window = NSApp.windows.first(where: { $0.isVisible && !($0 is NSPanel) }) else { return }
        let named: [String: (UInt16, String)] = [
            "left": (123, "\u{F702}"), "right": (124, "\u{F703}"), "down": (125, "\u{F701}"), "up": (126, "\u{F700}"),
            "return": (36, "\r"), "esc": (53, "\u{1B}"), "delete": (51, "\u{7F}"),
        ]
        for token in keys.split(separator: " ") where token != "wait" {   // "wait": one idle 0.3 s step
            var t = String(token)
            var flags: NSEvent.ModifierFlags = []
            while let dash = t.firstIndex(of: "-"), t.count > 1, dash != t.startIndex {
                switch t[..<dash] {
                case "opt": flags.insert(.option)
                case "shift": flags.insert(.shift)
                case "cmd": flags.insert(.command)
                default: break
                }
                t = String(t[t.index(after: dash)...])
            }
            var (code, chars) = named[t] ?? (0, t)
            if flags.contains(.shift), flags.contains(.command) { chars = chars.uppercased() }
            if let e = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: flags, timestamp: 0,
                                        windowNumber: window.windowNumber, context: nil, characters: chars,
                                        charactersIgnoringModifiers: chars, isARepeat: false, keyCode: code) {
                // ⌘ shortcuts belong to the menu bar. An inactive app ignores synthetic key
                // equivalents, so trigger the matching menu item directly.
                if flags.contains(.command) {
                    if !performMenuItem(key: t.lowercased(), flags: flags) { NSLog("--keys: no menu item for %@", token as NSString) }
                } else { _ = keyRouter?.handle(e) }
            }
        }
    }

    private func performMenuItem(key: String, flags: NSEvent.ModifierFlags, in menu: NSMenu? = NSApp.mainMenu) -> Bool {
        guard let menu else { return false }
        menu.update()
        let wanted = flags.intersection([.command, .shift, .option, .control])
        let named: [String: String] = ["delete": "\u{8}"]
        for (i, item) in menu.items.enumerated() {
            if let sub = item.submenu, performMenuItem(key: key, flags: flags, in: sub) { return true }
            var mask = item.keyEquivalentModifierMask.intersection([.command, .shift, .option, .control])
            let equivalent = item.keyEquivalent
            if equivalent != equivalent.lowercased() { mask.insert(.shift) }
            let target = named[key] ?? key
            guard !equivalent.isEmpty, equivalent.lowercased() == target || (key == "delete" && equivalent == "\u{7F}"),
                  mask == wanted else { continue }
            // SwiftUI may not refresh enabled state while the app is inactive; the model guards
            // every command itself, so trigger the item regardless.
            let enabled = item.isEnabled
            item.isEnabled = true
            menu.performActionForItem(at: i)
            item.isEnabled = enabled
            return true
        }
        return false
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
}
