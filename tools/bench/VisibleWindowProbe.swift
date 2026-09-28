import AppKit
import CoreGraphics
import Foundation

guard CommandLine.arguments.count == 2 else {
    fputs("usage: VisibleWindowProbe <bundle-id>\n", stderr)
    exit(2)
}

let bundleID = CommandLine.arguments[1]
let applications = NSRunningApplication.runningApplications(withBundleIdentifier: bundleID)
    .filter { !$0.isTerminated }
let bundlePIDs = applications.map(\.processIdentifier).sorted()
let frontmostPID = NSWorkspace.shared.frontmostApplication?.processIdentifier
let rawWindows = CGWindowListCopyWindowInfo(.optionOnScreenOnly, kCGNullWindowID) as? [[String: Any]] ?? []

func number(_ value: Any?) -> Double? { (value as? NSNumber)?.doubleValue }
func bounds(_ value: Any?) -> [String: Double]? {
    guard let box = value as? [String: Any],
          let x = number(box["X"]), let y = number(box["Y"]),
          let width = number(box["Width"]), let height = number(box["Height"]) else { return nil }
    return ["x": x, "y": y, "width": width, "height": height]
}

let windows: [[String: Any]] = rawWindows.compactMap { row in
    guard let ownerPID = row[kCGWindowOwnerPID as String] as? Int,
          let windowID = row[kCGWindowNumber as String] as? Int,
          let frame = bounds(row[kCGWindowBounds as String]) else { return nil }
    return ["owner_pid": ownerPID,
            "window_id": windowID,
            "layer": row[kCGWindowLayer as String] as? Int ?? -1,
            "alpha": number(row[kCGWindowAlpha as String]) ?? 1,
            "bounds": frame,
            "onscreen": true,
            "title": row[kCGWindowName as String] as? String ?? ""]
}

let frontmost: Any
if let frontmostPID { frontmost = Int(frontmostPID) } else { frontmost = NSNull() }
let result: [String: Any] = ["bundle_id": bundleID,
                             "bundle_pids": bundlePIDs,
                             "frontmost_pid": frontmost,
                             "windows": windows]
let data = try JSONSerialization.data(withJSONObject: result, options: [.sortedKeys])
FileHandle.standardOutput.write(data)
FileHandle.standardOutput.write(Data("\n".utf8))
