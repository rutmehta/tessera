import AppKit
import Foundation

/// Temporary diagnostic only. Off by default; bounded stderr, no file names or paths.
@MainActor
enum DocumentSaveLifecycleTrace {
    static let enabled = ProcessInfo.processInfo.environment["TESSERA_TRACE_SAVE_AS"] == "1"
    private static var sequence = 0
    private static let limit = 2000

    static func window(_ window: NSWindow?) -> String {
        guard let window else { return "nil" }
        let parent = window.sheetParent.map { String(describing: ObjectIdentifier($0)) } ?? "nil"
        let attached = window.attachedSheet.map { String(describing: ObjectIdentifier($0)) } ?? "nil"
        return "\(ObjectIdentifier(window)):parent=\(parent):attached=\(attached):visible=\(window.isVisible)"
    }

    static func emit(_ event: String, _ id: UUID?, _ detail: @autoclosure () -> String = "") {
        guard enabled, sequence < limit else { return }
        sequence += 1
        let line = "SAVE-LIFECYCLE seq=\(sequence) uptime=\(ProcessInfo.processInfo.systemUptime) event=\(event) request=\(id?.uuidString ?? "nil") \(detail())"
        FileHandle.standardError.write(Data((String(line.prefix(3000)) + "\n").utf8))
        if sequence == limit {
            FileHandle.standardError.write(Data("SAVE-LIFECYCLE trace-limit=2000 reached\n".utf8))
        }
    }
}
