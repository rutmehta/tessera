import AppKit
import SwiftUI

@MainActor protocol KeyOwningControl: AnyObject {}

@main struct LayoutProbe {
    @MainActor static func main() throws {
        let app = NSApplication.shared
        app.setActivationPolicy(.prohibited)
        let output = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
        var metrics: [[String: Any]] = []
        for appearance in ["dark", "light"] {
            app.appearance = NSAppearance(named: appearance == "dark" ? .darkAqua : .aqua)
            let view = NSView(frame: NSRect(x: 0, y: 0, width: 600, height: 330))
            view.wantsLayer = true
            view.layer?.backgroundColor = Theme.Palette.panel.cgColor(for: view)
            for (i, width) in [80.0, 82.6666667, 110.6666667].enumerated() {
                for (j, title) in ["Shadows", "Midtones", "Highlights"].enumerated() {
                    let slider = ValueSlider(frame: NSRect(x: 16 + Double(j)*190, y: 230 - Double(i)*90, width: width, height: 32))
                    slider.title = title
                    slider.doubleValue = -100
                    view.addSubview(slider)
                    let tw = (title as NSString).size(withAttributes: [.font: Theme.NSFonts.caption]).width
                    let vw = ceil(("-100" as NSString).size(withAttributes: [.font: Theme.NSFonts.captionNumericMedium]).width)
                    metrics.append(["appearance": appearance, "width": width, "title": title, "titleWidth": tw, "valueWidth": vw, "gap": width-tw-vw])
                    let label = NSTextField(labelWithString: "\(title) / \(String(format: "%.2f", width)) pt")
                    label.font = NSFont.systemFont(ofSize: 11)
                    label.frame = NSRect(x: slider.frame.minX, y: slider.frame.maxY+7, width: 182, height: 18)
                    view.addSubview(label)
                }
            }
            let window = NSWindow(contentRect: view.bounds, styleMask: [.borderless], backing: .buffered, defer: false)
            window.contentView = view
            window.appearance = app.appearance
            view.layoutSubtreeIfNeeded()
            let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds)!
            view.cacheDisplay(in: view.bounds, to: rep)
            try rep.representation(using: .png, properties: [:])!.write(to: output.appendingPathComponent("probe-sliders-\(appearance).png"))
            window.contentView = nil
        }
        let data = try JSONSerialization.data(withJSONObject: metrics, options: [.prettyPrinted, .sortedKeys])
        try data.write(to: output.deletingLastPathComponent().appendingPathComponent("font-metrics.json"))
        print(String(data: data, encoding: .utf8)!)
    }
}
