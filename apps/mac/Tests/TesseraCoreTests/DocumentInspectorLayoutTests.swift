import AppKit
import SwiftUI
import XCTest
import TesseraFFI
@testable import Tessera
@testable import TesseraCore

/// The document inspector fits its column (WP B5-10, the 1440-pt clipping defect): for every layer kind
/// the Properties editors can show (pixel, text, group, every adjustment and fill kind) and with all
/// panels expanded, the inspector's width for a proposed column width never exceeds it — at the
/// inspector column widths a 1440-pt window gives and the adjacent ones (min 288, ideal 296, 320, max 380).
@MainActor
final class DocumentInspectorLayoutTests: XCTestCase {
    private static let widths: [CGFloat] = [Theme.Width.inspectorMin, Theme.Width.inspectorIdeal, 320, Theme.Width.inspectorMax]

    private func workspace() throws -> (AppModel, DocumentController) {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("inspector-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        let engine = try Engine.open(appSupportDir: dir.appendingPathComponent("support").path)
        let model = AppModel()
        try model.documents.install(EngineDocumentBackend(session: try engine.newDocument(width: 1200, height: 800, depth: .u8, profile: nil)))
        for p in ["Properties", "History", "Channels", "Color", "Brushes"] { UserDefaults.standard.set(true, forKey: "InspectorPanel." + p) }
        return (model, try XCTUnwrap(model.documents.current))
    }

    /// Width each inspector section takes when offered `width`. The sections are measured outside the
    /// inspector's scroll views: a vertical ScrollView reports the offered width and silently clips
    /// wider content, which is how the 1440-pt defect hid.
    private func measured(_ model: AppModel, _ doc: DocumentController, width: CGFloat) -> [(String, CGFloat)] {
        func fit<V: View>(_ v: V) -> CGFloat {
            NSHostingController(rootView: v).sizeThatFits(in: CGSize(width: width, height: 20000)).width
        }
        return [
            ("inspector", fit(DocumentInspector(workspace: model.documents))),
            ("properties", fit(PanelSection("Properties") { PropertiesPanel(document: doc) })),
            ("tool sections", fit(ToolInspectorSections(tools: DocumentTools.shared, document: doc))),
            ("layers", fit(LayersPanel(document: doc))),
            ("channels", fit(PanelSection("Channels") { ChannelsPanel(document: doc, channels: DocumentChannels.shared) })),
            ("history", fit(PanelSection("History") { DocumentHistoryPanel(document: doc, workspace: model.documents) })),
        ]
    }

    /// Adds one layer of every Properties kind and returns them with a name.
    private func populate(_ doc: DocumentController) throws -> [(String, DocLayerID)] {
        var out: [(String, DocLayerID)] = []
        if let p = doc.layers.first { out.append(("pixel", p.id)) }
        for k in AdjustmentModel.Kind.allCases {
            doc.addAdjustment(k)
            if let id = doc.selection.last { out.append(("adjustment \(k.title)", id)) }
        }
        for k in FillModel.Kind.allCases {
            doc.addFill(k)
            if let id = doc.selection.last { out.append(("fill \(k.title)", id)) }
        }
        doc.addLayer(.group(mode: .passThrough))
        if let id = doc.selection.last { out.append(("group", id)) }
        let t = try XCTUnwrap(doc.backend as? any DocumentTextBackend)
        var m = TextSourceModel(runs: [TextRunModel(text: "Inspector", family: "Helvetica", size: 40),
                                       TextRunModel(text: " mixed styles with a very long run name", family: "Helvetica", weight: 700, size: 20)])
        m.warp.amount = 0.2   // limitations + the source editor: the longest text state
        let c = try t.addTextLayer(name: "", parent: nil, index: nil, model: m, transform: .translation(10, 60), interactive: false)
        doc.reloadModel()
        out.append(("text (warped, limitations)", try XCTUnwrap(c.created.first)))
        let c2 = try t.addTextLayer(name: "", parent: nil, index: nil, model: .point("Plain", family: "Helvetica", size: 30),
                                    transform: .translation(10, 160), interactive: false)
        doc.reloadModel()
        out.append(("text", try XCTUnwrap(c2.created.first)))
        return out
    }

    func testInspectorFitsItsColumnForEveryLayerKindAt1440AndAdjacentWidths() throws {
        let (model, doc) = try workspace()
        let layers = try populate(doc)
        XCTAssertGreaterThan(layers.count, 25)
        var overflows: [String] = []
        for (name, id) in layers {
            doc.selection = [id]
            for w in Self.widths {
                for (section, got) in measured(model, doc, width: w) where got > w + 0.5 {
                    overflows.append("\(name) · \(section) @\(Int(w)): \(Int(got.rounded()))")
                }
            }
        }
        XCTAssert(overflows.isEmpty, "inspector wider than its column:\n" + overflows.joined(separator: "\n"))
    }

    /// The split view budget (macOS 26): the floating sidebar and the inspector overlay the detail, and
    /// the split view requires detail minimum + sidebar + inspector, plus the inspector column again.
    /// With document mode's detail minimum that fits 1440 pt and the adjacent widths, with the inspector
    /// at its ideal and its maximum; the inspector's minimum is unchanged (no widening).
    func testColumnBudgetAt1440AndAdjacentWidths() {
        XCTAssertEqual(Theme.Width.inspectorMin, 288, "the fix does not widen the inspector")
        let detail = ContentView.documentDetailMinWidth
        for w in [1280.0, 1366, 1440, 1512] as [CGFloat] {
            XCTAssertLessThanOrEqual(detail + Theme.Width.sidebarIdeal + 2 * Theme.Width.inspectorIdeal, w, "\(w) ideal")
        }
        XCTAssertLessThanOrEqual(detail + Theme.Width.sidebarIdeal + 2 * Theme.Width.inspectorMax, 1440, "1440 with the inspector at its maximum")
        XCTAssertGreaterThanOrEqual(detail, Theme.Width.inspectorMin, "the canvas keeps a usable width")
    }
}
