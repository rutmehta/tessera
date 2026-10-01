import AppKit
import SwiftUI
import XCTest
import TesseraFFI
@testable import Tessera
@testable import TesseraCore

/// Removing an identifier or accessible name from any realized document control must fail
/// with its scenario and AX path. These probes never make a window key or order it front.
@MainActor
final class DocumentAccessibilityTests: XCTestCase {
    private let interactive: Set<String> = [
        "AXButton", "AXCheckBox", "AXSlider", "AXTextField", "AXTextArea", "AXPopUpButton",
        "AXMenuButton", "AXComboBox", "AXRadioButton", "AXTabGroup", "AXColorWell",
        "AXDisclosureTriangle", "AXRow",
    ]

    private func audit(_ root: AnyObject, scenario: String) -> Int {
        var seen = Set<ObjectIdentifier>()
        var count = 0
        var identifiers: [String: String] = [:]
        var roles = Set<String>()
        func walk(_ node: AnyObject, path: String) {
            guard seen.insert(ObjectIdentifier(node)).inserted else { return }
            // AppKit's NSOutlineRow and cell-backed controls still expose legacy AX attributes.
            // SwiftUI virtual nodes expose modern selectors without protocol conformance.
            func legacy(_ name: String) -> Any? {
                node.accessibilityAttributeValue?(NSAccessibility.Attribute(rawValue: name))
            }
            let modernRole = node.accessibilityRole?()?.rawValue
            let role = (modernRole == "AXUnknown" ? nil : modernRole) ?? legacy("AXRole") as? String ?? "unknown"
            // AppKit's exported cell element uses metadata installed on its control view.
            // Direct legacy cell getters do not merge that metadata as the AX server does.
            let owner = (node as? NSCell)?.controlView
            let id = [node.accessibilityIdentifier?(), legacy("AXIdentifier") as? String, owner?.accessibilityIdentifier()]
                .compactMap { $0 }.first { !$0.isEmpty } ?? ""
            // AXTitle is the accessible name of standard AppKit/SwiftUI buttons;
            // AXLabel is used by custom controls. Values/placeholders/help are not names.
            let label = [node.accessibilityLabel?(), owner?.accessibilityLabel(), legacy("AXDescription") as? String, node.accessibilityTitle?(), legacy("AXTitle") as? String]
                .compactMap { $0 }.first { !$0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty } ?? ""
            let here = "\(path)/\(role)[\(id)]{\(label)}"
            // Scrollbar arrows/thumbs are AppKit implementation details, not document controls.
            if role == "AXScrollBar" { return }
            roles.insert(role)
            let hasPress = node.accessibilityActionNames?().contains(.press) ?? false
            if interactive.contains(role) || hasPress {
                count += 1
                // A native toolbar item and its hosted child can describe the same control.
                // Sibling controls must still have distinct identifiers.
                if !id.isEmpty {
                    if let first = identifiers[id], scenario != "toolbar" || !path.hasPrefix(first + "/") {
                        XCTFail("\(here) — duplicate interactive identifier")
                    } else if identifiers[id] == nil { identifiers[id] = here }
                }
                if ProcessInfo.processInfo.environment["TESSERA_AX_MAP"] == "1" {
                    print("AX MAP \(id)\t\(label)")
                }
                if id.range(of: #"^document\.[^.\s]+\.[^\s]+$"#, options: .regularExpression) == nil || label.isEmpty {
                    XCTFail("\(scenario): \(here) — \(id.isEmpty ? "missing identifier" : "identifier=" + id); \(label.isEmpty ? "missing label" : "label=" + label)")
                }
            }
            for (index, child) in (node.accessibilityChildren?() ?? legacy("AXChildren") as? [Any] ?? []).enumerated() {
                walk(child as AnyObject, path: "\(here)/\(index)")
            }
        }
        walk(root, path: scenario)
        if scenario == "window.stack" {
            XCTAssertTrue(roles.contains("AXRow"), "Layers rows must be reached through the AX tree")
            XCTAssertTrue(roles.contains("AXDisclosureTriangle"), "Layer row actions must be reached")
            XCTAssertNotNil(identifiers["document.layers.row.0.visibility"], "Layer visibility action must be reached")
            XCTAssertNotNil(identifiers["document.layers.row.0.maskLink"], "Layer mask link action must be reached")
            XCTAssertTrue(identifiers.keys.contains { $0.hasPrefix("document.layers.smartFilter.") && $0.hasSuffix(".blending") },
                          "Smart filter blending action must be reached")
            XCTAssertTrue(identifiers.keys.contains { $0.hasPrefix("document.layers.effect.") && $0.hasSuffix(".visibility") },
                          "Layer effect visibility action must be reached")
        }
        print("AX AUDIT \(scenario): \(count) interactive controls")
        return count
    }

    private func host<V: View>(_ view: V, scenario: String, check: (NSView) -> Void = { _ in }) async {
        let bounds = NSRect(x: 0, y: 0, width: 1500, height: 1800)
        let host = NSHostingView(rootView: LayoutProbeHarness.root(view))
        let window = LayoutProbeHarness.window(contentRect: bounds, styleMask: .titled, backing: .buffered, defer: false)
        window.contentView = host
        host.frame = bounds
        window.orderBack(nil)
        defer { LayoutProbeHarness.dispose(window) }
        let attribute = NSAccessibility.Attribute(rawValue: "AXEnhancedUserInterface")
        let previous = NSApp.accessibilityAttributeValue(attribute)
        NSApp.accessibilitySetValue(true, forAttribute: attribute)
        defer { NSApp.accessibilitySetValue(previous, forAttribute: attribute) }
        await LayoutProbeHarness.settleAsync(host)
        func expand(_ view: NSView) {
            if let outline = view as? NSOutlineView { outline.expandItem(nil, expandChildren: true) }
            for child in view.subviews { expand(child) }
        }
        expand(host)
        await LayoutProbeHarness.settleAsync(host)
        XCTAssertFalse(window.isKeyWindow)
        XCTAssertFalse(NSApp.isActive)
        XCTAssertGreaterThan(audit(host, scenario: scenario), 0, "\(scenario): empty control tree")
        check(host)
    }

    private func fixture() throws -> AppModel {
        LayoutProbeHarness.prepare()
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("document-ax-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        let engine = try Engine.open(appSupportDir: dir.appendingPathComponent("support").path)
        let model = AppModel()
        try model.documents.install(EngineDocumentBackend(session: try engine.newDocument(width: 64, height: 64, depth: .u8, profile: nil)))
        DocumentTools.shared.attach(model.documents)
        DocumentChannels.shared.attach(model.documents)
        DocumentText.shared.attach(model.documents)
        DocumentVector.shared.attach(model.documents)
        for name in ["History", "Color", "Brushes"] {
            let key = "InspectorPanel." + name
            let previous = UserDefaults.standard.object(forKey: key)
            UserDefaults.standard.set(true, forKey: key)
            addTeardownBlock {
                if let previous { UserDefaults.standard.set(previous, forKey: key) }
                else { UserDefaults.standard.removeObject(forKey: key) }
            }
        }
        return model
    }

    func testDocumentWindowInspectorPanelsAndToolbar() async throws {
        let model = try fixture()
        let ws = model.documents
        let doc = try XCTUnwrap(ws.current)
        doc.addLayer(.group(mode: .passThrough))
        doc.addLayer(.pixel)
        doc.addMask(.revealAll)
        let layer = try XCTUnwrap(doc.primary?.id)
        DocumentStyles.shared.addEffect(doc, layer, .dropShadow)
        let filters = try XCTUnwrap(doc.backend as? any DocumentFiltersBackend)
        _ = try filters.convertForSmartFilters(layer: layer)
        _ = try filters.applyFilter(layer: layer, filterJson: #"{"id":"gaussian_blur","params":{"radius":3}}"#)
        doc.reloadModel()
        doc.reloadHistory()
        doc.snapshot(named: "AX snapshot")
        DocumentChannels.shared.newChannel()
        DocumentChannels.shared.reload(doc, force: true)
        for tab in DocumentInspectorTab.allCases {
            ws.inspectorTab = tab
            await host(HStack {
                VStack {
                    DocumentTabs(workspace: ws)
                    DocumentView(workspace: ws)
                }
                DocumentInspector(workspace: ws).frame(width: 380)
            }, scenario: "window.\(tab.rawValue)")
        }
        if let channel = DocumentChannels.shared.records.first {
            DocumentChannels.shared.renaming = channel.id
            await host(ChannelsPanel(document: doc, channels: .shared), scenario: "channels.rename")
            DocumentChannels.shared.renaming = nil
        }
        // The real unified toolbar also has shared shell controls (Open Folder, Library).
        let (window, _) = ShellHarness.window(model, size: CGSize(width: 1440, height: 900), dark: true)
        defer { LayoutProbeHarness.dispose(window) }
        let attribute = NSAccessibility.Attribute(rawValue: "AXEnhancedUserInterface")
        let previous = NSApp.accessibilityAttributeValue(attribute)
        NSApp.accessibilitySetValue(true, forAttribute: attribute)
        defer { NSApp.accessibilitySetValue(previous, forAttribute: attribute) }
        await LayoutProbeHarness.settleAsync(try XCTUnwrap(window.contentView))
        var seen = Set<ObjectIdentifier>()
        func findToolbar(_ node: AnyObject) -> AnyObject? {
            guard seen.insert(ObjectIdentifier(node)).inserted else { return nil }
            if node.accessibilityRole?() == .toolbar { return node }
            for child in node.accessibilityChildren?() ?? [] {
                if let found = findToolbar(child as AnyObject) { return found }
            }
            return nil
        }
        let toolbar = try XCTUnwrap(findToolbar(window))
        XCTAssertGreaterThan(audit(toolbar as AnyObject, scenario: "toolbar"), 0)
        XCTAssertFalse(window.isKeyWindow)
    }

    func testEveryToolOptionsBar() async throws {
        let model = try fixture()
        let doc = try XCTUnwrap(model.documents.current)
        for tool in DocumentTool.allCases {
            doc.tool = tool
            await host(HStack { ToolsPalette(document: doc, tools: .shared); ToolOptionsBar(document: doc, tools: .shared) }, scenario: "options.\(tool.rawValue)")
        }
        await host(HStack { RemoveOptionsBar(document: doc, retouch: .shared) }, scenario: "options.remove")
        await host(HStack { ContentAwareOptionsBar(document: doc, cam: .shared) }, scenario: "options.contentAwareMove")
    }

    func testEveryAdjustmentFillAndTextProperties() async throws {
        let model = try fixture()
        let doc = try XCTUnwrap(model.documents.current)
        doc.tool = .brush
        await host(ToolInspectorSections(tools: .shared, document: doc).frame(width: 380), scenario: "inspector.brushes")
        for kind in AdjustmentModel.Kind.allCases {
            doc.addAdjustment(kind)
            await host(PropertiesPanel(document: doc).frame(width: 380), scenario: "properties.\(kind.rawValue)")
        }
        for kind in FillModel.Kind.allCases {
            doc.addFill(kind)
            await host(PropertiesPanel(document: doc).frame(width: 380), scenario: "fill.\(kind.rawValue)")
        }
        doc.addLayer(.group(mode: .passThrough))
        await host(PropertiesPanel(document: doc).frame(width: 380), scenario: "properties.group")
        let backend = try XCTUnwrap(doc.backend as? any DocumentTextBackend)
        let change = try backend.addTextLayer(name: "AX text", parent: nil, index: nil,
            model: .point("Audit", family: "Helvetica", size: 24), transform: .identity, interactive: false)
        doc.reloadModel()
        doc.selection = [try XCTUnwrap(change.created.first)]
        await host(PropertiesPanel(document: doc).frame(width: 380), scenario: "properties.text")
        XCTAssertTrue(DocumentText.shared.beginExisting(doc, layer: try XCTUnwrap(change.created.first)))
        await host(VStack {
            PropertiesPanel(document: doc)
            HStack { TextOptionsBar(document: doc, text: .shared) }
        }, scenario: "text.editing")
        DocumentText.shared.cancel()
        await DocumentText.shared.idle()
    }
    func testShapePropertiesAndTransformOptions() async throws {
        let model = try fixture()
        let doc = try XCTUnwrap(model.documents.current)
        let backend = try XCTUnwrap(doc.backend as? any DocumentVectorBackend)
        let source = ShapeSource(live: .rectangle(rect: ShapeRect(CGRect(x: 2, y: 2, width: 40, height: 30)),
                                                  radii: [2, 2, 2, 2]),
                                 fill: .solid([1, 0, 0, 1]), stroke: (ShapeStroke(), .solid([0, 0, 0, 1])))
        let change = try backend.addShapeLayer(name: "AX shape", parent: nil, index: nil, source: source, transform: .identity)
        doc.reloadModel()
        doc.select(try XCTUnwrap(change.created.first))
        DocumentVector.shared.invalidate()
        await host(PropertiesPanel(document: doc).frame(width: 380), scenario: "properties.shape")
        DocumentVector.shared.convertToPixels()
        await DocumentVector.shared.idle()
        DocumentTools.shared.beginFreeTransform()
        XCTAssertNotNil(DocumentTools.shared.transform)
        await host(ToolOptionsBar(document: doc, tools: .shared), scenario: "options.freeTransform")
        DocumentTools.shared.cancelTransform()
        await DocumentTools.shared.idle()
        doc.addFill(.solid)
        let transforms = DocumentTransforms.shared
        transforms.attach(model.documents)
        for tag in AdvancedTransformTag.allCases where tag.editable {
            transforms.begin(tag)
            await transforms.idle()
            XCTAssertNotNil(transforms.session, tag.rawValue)
            await host(HStack { TransformOptionsBar(document: doc, t: transforms) }, scenario: "transform.\(tag.rawValue)")
            transforms.cancel()
            await transforms.idle()
        }
    }

    func testNativeOutlineAXActionsKeepSelectionAndDisclosure() async throws {
        let model = try fixture()
        let doc = try XCTUnwrap(model.documents.current)
        doc.addLayer(.group(mode: .passThrough))
        doc.addLayer(.pixel)
        await host(LayersPanel(document: doc), scenario: "layers.actions") { root in
            func find(_ view: NSView) -> NSOutlineView? {
                if let outline = view as? NSOutlineView { return outline }
                return view.subviews.lazy.compactMap(find).first
            }
            guard let outline = find(root) else { return XCTFail("Missing Layers outline") }
            let rows = outline.accessibilityChildren()?.compactMap { $0 as? LayerRowView } ?? []
            XCTAssertEqual(rows.count, outline.numberOfRows)
            guard let row = rows.first else { return XCTFail("Missing accessible row") }
            outline.deselectAll(nil)
            row.setAccessibilitySelected(true)
            XCTAssertTrue(outline.isRowSelected(outline.row(for: row)))
            row.setAccessibilitySelected(false)
            XCTAssertFalse(outline.isRowSelected(outline.row(for: row)))
            guard let group = rows.first(where: { outline.isExpandable(outline.item(atRow: outline.row(for: $0))) }) else {
                return XCTFail("Missing expandable row")
            }
            group.setAccessibilityDisclosed(false)
            XCTAssertFalse(outline.isItemExpanded(outline.item(atRow: outline.row(for: group))))
            group.setAccessibilityDisclosed(true)
            XCTAssertTrue(outline.isItemExpanded(outline.item(atRow: outline.row(for: group))))
        }
    }

}
