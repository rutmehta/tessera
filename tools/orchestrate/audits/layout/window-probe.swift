import AppKit
import SwiftUI
@testable import Tessera
import TesseraCore

@main struct WindowProbe {
    @MainActor static func main() throws {
        let app = NSApplication.shared
        app.setActivationPolicy(.prohibited)
        setbuf(stdout, nil)
        let output = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
        for mode in ["library", "raw", "document"] {
            let model = AppModel()
            if mode == "document" {
                model.install(StubLibrary.synthetic(count: 40))
            } else {
                let folder = mode == "raw" ? URL(fileURLWithPath: CommandLine.arguments[2]) : output.deletingLastPathComponent().appendingPathComponent("sample-shoot")
                model.openFolder(folder)
                let deadline = Date().addingTimeInterval(45)
                while model.isLoading && Date() < deadline { RunLoop.main.run(until: Date().addingTimeInterval(0.1)) }
                print("DATA \(mode) engine=\(model.isEngineBacked) items=\(model.library.items.count) loading=\(model.isLoading)")
            }
            print("FIT status=\(NSHostingView(rootView: StatusBar(model: model)).fittingSize) loupe=\(NSHostingView(rootView: LoupeView(model: model)).fittingSize) grid=\(NSHostingView(rootView: ThumbnailBrowser(model: model, style: .grid)).fittingSize)")
            if mode == "document" {
                model.documents.policy = .stub
                model.documents.newDocument(model.documents.newSettings)
            }
            if ProcessInfo.processInfo.environment["TESSERA_AUDIT_MEASURE_ONLY"] == "1" {
                if mode == "document" {
                    print("FIT documentStatus=\(NSHostingView(rootView: DocumentStatusBar(model: model, workspace: model.documents)).fittingSize) documentInspector=\(NSHostingView(rootView: DocumentInspector(workspace: model.documents)).fittingSize) documentView=\(NSHostingView(rootView: DocumentView(workspace: model.documents)).fittingSize)")
                }
                continue
            }
            for appearance in ["dark", "light"] {
                app.appearance = NSAppearance(named: appearance == "dark" ? .darkAqua : .aqua)
                for (w,h) in [(1280,800), (1440,900), (1728,1117)] {
                    let host = NSHostingView(rootView: ContentView(model: model).frame(minWidth: 960, minHeight: 600).environment(\.colorScheme, appearance == "dark" ? .dark : .light))
                    host.frame = NSRect(x: 0, y: 0, width: w, height: h)
                    let window = NSWindow(contentRect: host.frame, styleMask: [.titled,.resizable], backing: .buffered, defer: false)
                    window.contentView = host
                    window.appearance = app.appearance
                    window.orderBack(nil)
                    host.layoutSubtreeIfNeeded()
                    RunLoop.main.run(until: Date().addingTimeInterval(1.0))
                    window.setFrame(NSRect(x: 40, y: 40, width: w, height: h), display: true)
                    RunLoop.main.run(until: Date().addingTimeInterval(0.3))
                    host.layoutSubtreeIfNeeded()
                    var frames: [[String: String]] = []
                    func walk(_ v: NSView) {
                        let name = String(describing: type(of: v))
                        if v is NSControl || name.contains("Hosting") || name.contains("Split") {
                            frames.append(["class": name, "frameInHost": NSStringFromRect(v.convert(v.bounds, to: host)), "bounds": NSStringFromRect(v.bounds)])
                        }
                        for child in v.subviews { walk(child) }
                    }
                    walk(host)
                    let data = try JSONSerialization.data(withJSONObject: frames, options: [.prettyPrinted,.sortedKeys])
                    try data.write(to: output.deletingLastPathComponent().appendingPathComponent("frames-\(mode)-\(w)x\(h)-\(appearance).json"))
                    let capture = Process()
                    capture.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
                    capture.arguments = ["-l", String(window.windowNumber), "-x", output.appendingPathComponent("window-\(mode)-\(w)x\(h)-\(appearance).png").path]
                    try capture.run()
                    capture.waitUntilExit()
                    print("capture=\(capture.terminationStatus) window=\(window.windowNumber) frame=\(window.frame)")
                    print("frontmostIsProbe=\(NSWorkspace.shared.frontmostApplication?.processIdentifier == ProcessInfo.processInfo.processIdentifier)")
                    print("\(mode) \(appearance) requested=\(w)x\(h) host=\(host.bounds) fitting=\(host.fittingSize)")
                    window.orderOut(nil)
                    window.contentView = nil
                }
            }
        }
        exit(0)
    }
}
