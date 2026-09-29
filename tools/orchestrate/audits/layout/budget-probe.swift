import AppKit
import SwiftUI
import TesseraCore
@testable import Tessera

// Counterfactual layout experiment, not a production view or screenshot.
struct BudgetFixture: View {
    let model: AppModel
    let status: Bool
    var body: some View {
        NavigationSplitView {
            SidebarView(model: model)
                .navigationSplitViewColumnWidth(min: 200, ideal: 220, max: 300)
        } detail: {
            VStack(spacing: 0) {
                GeometryReader { area in
                    DocumentView(workspace: model.documents)
                        .frame(width: area.size.width, height: area.size.height)
                }
                if status { DocumentStatusBar(model: model, workspace: model.documents) }
            }
        }
        .inspector(isPresented: .constant(true)) {
            DocumentInspector(workspace: model.documents)
                .inspectorColumnWidth(min: 288, ideal: 296, max: 380)
        }
        .frame(minWidth: 960, minHeight: 600)
    }
}
@main struct BudgetProbe {
    @MainActor static func main() {
        NSApplication.shared.setActivationPolicy(.prohibited)
        setbuf(stdout, nil)
        let model = AppModel()
        model.install(StubLibrary.synthetic(count: 40))
        model.documents.policy = .stub
        model.documents.newDocument(model.documents.newSettings)
        for status in [true, false] {
            let host = NSHostingView(rootView: BudgetFixture(model: model, status: status))
            host.frame = NSRect(x: 0, y: 0, width: 1280, height: 748)
            let window = NSWindow(contentRect: host.frame, styleMask: [.borderless], backing: .buffered, defer: false)
            window.contentView = host
            host.layoutSubtreeIfNeeded()
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
            print("counterfactual documentStatus=\(status), fitting=\(host.fittingSize)")
            window.contentView = nil
        }
        exit(0)
    }
}
