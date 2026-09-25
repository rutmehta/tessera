import AppKit
import TesseraCore
import SwiftUI

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
        .commands { AppCommands(model: model) }
    }
}

/// Launch handling: activation (also when run as a bare SwiftPM executable), dark appearance,
/// the culling key monitor, and the initial library.
///
/// Launch arguments (used by the acceptance script and benchmarks):
///   --folder <path>   open this folder instead of the remembered one
///   --stub <count>    load <count> synthetic items (e.g. 20000)
///   --benchmark       run the grid scroll benchmark after loading
///   --front           order the window front without activating (screenshots while another app is active)
///   --keys "<k> <k>…" after loading, feed these keys through the culling key map (self-test aid);
///                     tokens: single characters, left right up down return esc, prefix "opt-" / "shift-"
@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    private var keyRouter: KeyRouter?

    func applicationWillFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.regular)
        NSApp.appearance = NSAppearance(named: .darkAqua)
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        let model = AppModel.shared
        keyRouter = KeyRouter(model: model)
        keyRouter?.install()
        NSApp.activate()

        let args = ProcessInfo.processInfo.arguments
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
        if args.contains("--front") {   // test aid: show the window without activating the app
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
                MainActor.assumeIsolated { NSApp.windows.first { !($0 is NSPanel) }?.orderFrontRegardless() }
            }
        }
        if let keys = value(after: "--keys") {
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) {
                MainActor.assumeIsolated { self.simulate(keys: keys) }
            }
        }
        if args.contains("--benchmark") {
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) {
                MainActor.assumeIsolated { model.requestScrollBenchmark() }
            }
        }
    }

    private func simulate(keys: String) {
        guard let window = NSApp.windows.first(where: { $0.isVisible && !($0 is NSPanel) }) else { return }
        let named: [String: (UInt16, String)] = [
            "left": (123, "\u{F702}"), "right": (124, "\u{F703}"), "down": (125, "\u{F701}"), "up": (126, "\u{F700}"),
            "return": (36, "\r"), "esc": (53, "\u{1B}"),
        ]
        for token in keys.split(separator: " ") {
            var t = String(token)
            var flags: NSEvent.ModifierFlags = []
            while let dash = t.firstIndex(of: "-"), t.count > 1, dash != t.startIndex {
                switch t[..<dash] {
                case "opt": flags.insert(.option)
                case "shift": flags.insert(.shift)
                default: break
                }
                t = String(t[t.index(after: dash)...])
            }
            let (code, chars) = named[t] ?? (0, t)
            if let e = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: flags, timestamp: 0,
                                        windowNumber: window.windowNumber, context: nil, characters: chars,
                                        charactersIgnoringModifiers: chars, isARepeat: false, keyCode: code) {
                _ = keyRouter?.handle(e)
            }
        }
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
}
