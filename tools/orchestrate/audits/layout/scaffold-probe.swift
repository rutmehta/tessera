import AppKit
import SwiftUI

// A geometry-only fixture, NOT the real engine-backed FilterSheet.
// Uses the exact shared SheetScaffold and ThemeButtonStyle implementations.
struct ScaffoldFixture: View {
    var body: some View {
        SheetScaffold(title: "Gaussian Blur", subtitle: "Layer ‘sample’") {
            EmptyView()
        } content: {
            HStack(alignment: .top, spacing: Theme.Space.l) {
                Rectangle().fill(Theme.well).overlay(Text("180 × 180 preview fixture").font(Theme.Fonts.caption))
                    .frame(width: 180, height: 180)
                VStack(alignment: .leading, spacing: Theme.Space.xs) {
                    Text("Radius").font(Theme.Fonts.caption)
                    Slider(value: .constant(12), in: 0...100).frame(height: Theme.Height.slider)
                    Spacer(minLength: 0)
                }.frame(maxWidth: .infinity, alignment: .topLeading)
            }.padding(Theme.Space.l)
        } leading: {
            Toggle("Preview", isOn: .constant(true)).toggleStyle(.checkbox).font(Theme.Fonts.caption)
        } actions: {
            Button("Reset") {}.sheetButton()
            Button("Cancel") {}.sheetButton()
            Button("OK") {}.sheetButton(primary: true)
        }.frame(width: 560, height: 300)
    }
}
@main struct Probe {
    @MainActor static func main() throws {
        let app = NSApplication.shared
        app.setActivationPolicy(.prohibited)
        for name in ["dark", "light"] {
            app.appearance = NSAppearance(named: name == "dark" ? .darkAqua : .aqua)
            let host = NSHostingView(rootView: ScaffoldFixture().environment(\.colorScheme, name == "dark" ? .dark : .light))
            host.sizingOptions = []
            host.frame = NSRect(x: 0, y: 0, width: 560, height: 300)
            let window = NSWindow(contentRect: host.frame, styleMask: [.borderless], backing: .buffered, defer: false)
            window.contentView = host
            window.appearance = app.appearance
            host.layoutSubtreeIfNeeded()
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
            let rep = host.bitmapImageRepForCachingDisplay(in: host.bounds)!
            host.cacheDisplay(in: host.bounds, to: rep)
            let url = URL(fileURLWithPath: CommandLine.arguments[1]).appendingPathComponent("probe-scaffold-\(name).png")
            try rep.representation(using: .png, properties: [:])!.write(to: url)
            print("\(name): host=\(host.bounds), fitting=\(host.fittingSize)")
            window.contentView = nil
        }
    }
}
