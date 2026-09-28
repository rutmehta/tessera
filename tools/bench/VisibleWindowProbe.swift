import AppKit
import CoreGraphics
import Darwin
import Foundation

func emit(_ value: [String: Any]) {
    do {
        let data = try JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])
        FileHandle.standardOutput.write(data)
        FileHandle.standardOutput.write(Data("\n".utf8))
    } catch {
        fputs("probe JSON failure: \(error)\n", stderr)
        exit(4)
    }
}

func number(_ value: Any?) -> Double? { (value as? NSNumber)?.doubleValue }
func bounds(_ value: Any?) -> [String: Double]? {
    guard let box = value as? [String: Any],
          let x = number(box["X"]), let y = number(box["Y"]),
          let width = number(box["Width"]), let height = number(box["Height"]) else { return nil }
    return ["x": x, "y": y, "width": width, "height": height]
}

func snapshot(bundleID: String) -> [String: Any] {
    let applications = NSRunningApplication.runningApplications(withBundleIdentifier: bundleID)
        .filter { !$0.isTerminated }
    let bundleApps: [[String: Any]] = applications.map { app in
        let launchDate: Any
        if let date = app.launchDate { launchDate = date.timeIntervalSince1970 } else { launchDate = NSNull() }
        return ["pid": Int(app.processIdentifier),
         "bundle_url": app.bundleURL?.standardizedFileURL.path ?? "",
         "bundle_id": app.bundleIdentifier ?? "",
         "launch_date": launchDate,
         "name": app.localizedName ?? ""]
    }
    let frontmostPID = NSWorkspace.shared.frontmostApplication?.processIdentifier
    let rawWindows = CGWindowListCopyWindowInfo(.optionOnScreenOnly, kCGNullWindowID) as? [[String: Any]] ?? []
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
    return ["bundle_id": bundleID, "bundle_apps": bundleApps,
            "bundle_pids": applications.map(\.processIdentifier).sorted().map(Int.init),
            "frontmost_pid": frontmost, "windows": windows]
}

let args = CommandLine.arguments
if args.count == 2, args[1] != "--terminate" {
    emit(snapshot(bundleID: args[1]))
    exit(0)
}

guard args.count == 6, args[1] == "--terminate", let pid = Int32(args[2]),
      let expectedLaunchDate = Double(args[5]) else {
    fputs("usage: VisibleWindowProbe <bundle-id> | --terminate <pid> <bundle-id> <bundle-url> <launch-date>\n", stderr)
    exit(2)
}
let bundleID = args[3]
let expectedURL = URL(fileURLWithPath: args[4]).standardizedFileURL.path
guard let app = NSRunningApplication(processIdentifier: pid),
      !app.isTerminated,
      app.bundleIdentifier == bundleID,
      app.bundleURL?.standardizedFileURL.path == expectedURL,
      app.launchDate?.timeIntervalSince1970 == expectedLaunchDate else {
    fputs("refusing termination: PID, bundle ID, bundle URL, or launch date does not match the owned process\n", stderr)
    exit(3)
}
let requested = app.terminate()
emit(["pid": Int(pid), "bundle_id": bundleID, "bundle_url": expectedURL,
      "launch_date": expectedLaunchDate,
      "graceful_terminate_requested": requested])
exit(requested ? 0 : 1)
