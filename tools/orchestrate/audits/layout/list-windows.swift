import AppKit
import CoreGraphics
let front = NSWorkspace.shared.frontmostApplication
print("frontmost pid=\(front?.processIdentifier ?? -1)")
let windows = CGWindowListCopyWindowInfo([.optionAll, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
let selected = windows.filter { ($0[kCGWindowOwnerName as String] as? String ?? "").localizedCaseInsensitiveContains("Tessera") }
let data = try JSONSerialization.data(withJSONObject: selected, options: [.prettyPrinted, .sortedKeys])
print(String(data: data, encoding: .utf8)!)
