import AppKit
import ObjectiveC
import SwiftUI
import XCTest
import TesseraFFI
@testable import Tessera
@testable import TesseraCore

// AppKit implements these modern accessors for AXEnhancedUserInterface but does not
// expose them in the public NSAccessibility protocol. Keep this test-only bridge
// so hosted SwiftUI trees are materialized without deprecated attribute APIs.
@objc private protocol LibraryHostedAccessibilityApplication {
    @objc optional func isAccessibilityEnhancedUserInterface() -> Bool
    @objc optional func setAccessibilityEnhancedUserInterface(_ enabled: Bool)
}

/// Removing an identifier or accessible name from any realized document control must fail
/// with its scenario and AX path. These probes never make a window key or order it front.
@MainActor
final class LibraryDevelopAccessibilityTests: XCTestCase {
    private let interactive: Set<String> = [
        "AXButton", "AXCheckBox", "AXSlider", "AXTextField", "AXTextArea", "AXPopUpButton",
        "AXMenuButton", "AXComboBox", "AXRadioButton", "AXTabGroup", "AXColorWell",
        "AXDisclosureTriangle", "AXRow",
    ]

    private func offersAccessibilityPress(_ node: AnyObject) -> Bool {
        let selector = #selector(NSAccessibilityProtocol.accessibilityPerformPress)
        guard (node as? NSObjectProtocol)?.responds(to: selector) == true,
              node.isAccessibilitySelectorAllowed?(selector) ?? true else { return false }
        // These bases inherit default/compatibility dispatch methods even on passive elements.
        // A selector alone does not advertise a press action. Count actual overrides
        // (including nonstandard roles); the explicit role set still covers native controls.
        let implementation = class_getMethodImplementation(object_getClass(node), selector)
        for base: AnyClass in [NSView.self, NSCell.self, NSAccessibilityElement.self] {
            if unsafeBitCast(implementation, to: UInt.self)
                == unsafeBitCast(class_getMethodImplementation(base, selector), to: UInt.self) { return false }
        }
        return true
    }

    private func audit(_ root: AnyObject, scenario: String) -> Int {
        var seen = Set<ObjectIdentifier>()
        var count = 0
        var identifiers: [String: String] = [:]
        var names: [String: String] = [:]
        func walk(_ node: AnyObject, path: String) {
            guard seen.insert(ObjectIdentifier(node)).inserted else { return }
            // SwiftUI virtual nodes expose modern selectors without protocol conformance.
            // Cell-backed controls keep their accessible metadata on the owning view.
            let owner = (node as? NSCell)?.controlView
            let modernRole = node.accessibilityRole?()
            let role = (modernRole == .unknown ? owner?.accessibilityRole() : modernRole)?.rawValue ?? "unknown"
            let id = [node.accessibilityIdentifier?(), owner?.accessibilityIdentifier()]
                .compactMap { $0 }.first { !$0.isEmpty } ?? ""
            // AXTitle is the accessible name of standard AppKit/SwiftUI buttons;
            // AXLabel is used by custom controls. Values/placeholders/help are not names.
            let label = [node.accessibilityLabel?(), owner?.accessibilityLabel(), node.accessibilityTitle?()]
                .compactMap { $0 }.first { !$0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty } ?? ""
            if !id.isEmpty { names[id] = label }
            let here = "\(path)/\(role)[\(id)]{\(label)}"
            // Scrollbar arrows/thumbs are AppKit implementation details, not document controls.
            if role == "AXScrollBar" { return }
            // macOS-owned window chrome is outside the app's identifier namespace.
            if let subrole = node.accessibilitySubrole?()?.rawValue,
               ["AXCloseButton", "AXMinimizeButton", "AXZoomButton", "AXFullScreenButton"].contains(subrole) { return }
            let offersPress = offersAccessibilityPress(node)
            if interactive.contains(role) || offersPress {
                count += 1
                // A native toolbar item and its hosted child can describe the same control.
                // Sibling controls must still have distinct identifiers.
                if !id.isEmpty {
                    if let first = identifiers[id], !path.hasPrefix(first + "/") {
                        XCTFail("\(here) — duplicate interactive identifier")
                    } else if identifiers[id] == nil { identifiers[id] = here }
                }
                if ProcessInfo.processInfo.environment["TESSERA_AX_MAP"] == "1" {
                    print("AX MAP \(id)\t\(label)")
                }
                if id.range(of: #"^(library|develop)\.[^.\s]+\.[^\s]+$"#, options: .regularExpression) == nil || label.isEmpty {
                    XCTFail("\(scenario): \(here) — \(id.isEmpty ? "missing identifier" : "identifier=" + id); \(label.isEmpty ? "missing label" : "label=" + label)")
                }
            }
            for (index, child) in (node.accessibilityChildren?() ?? []).enumerated() {
                walk(child as AnyObject, path: "\(here)/\(index)")
            }
        }
        walk(root, path: scenario)
        if scenario == "library.populated" {
            XCTAssertTrue(names.keys.contains { $0.hasPrefix("library.thumbnail.grid.") }, "Thumbnail cells must be reachable")
        }
        let required: [String]
        switch scenario {
        case "library.populated": required = ["library.filter.rule", "library.toolbar.open", "library.sidebar.row.src:all"]
        case "develop.basic": required = ["develop.basic.exposure"]
        case "develop.selectedMask": required = ["develop.masks.amount", "develop.masks.reset"]
        case "library.import.reportGroups": required = ["library.import.report.warnings", "library.import.report.approximate", "library.import.report.markdown"]
        default: required = []
        }
        for id in required { XCTAssertFalse((names[id] ?? "").isEmpty, "\(scenario): required control or report group unreachable: \(id)") }
        print("AX AUDIT \(scenario): \(count) interactive controls")
        return count
    }


    private func inspect(_ window: NSWindow, scenario: String, press: [String] = []) async throws {
        defer { LayoutProbeHarness.dispose(window) }
        let application = NSApp as AnyObject
        let previous = try XCTUnwrap(application.isAccessibilityEnhancedUserInterface?())
        application.setAccessibilityEnhancedUserInterface?(true)
        defer { application.setAccessibilityEnhancedUserInterface?(previous) }
        await LayoutProbeHarness.settleAsync(try XCTUnwrap(window.contentView))
        for identifier in press {
            var visited = Set<ObjectIdentifier>()
            func find(_ node: AnyObject) -> AnyObject? {
                guard visited.insert(ObjectIdentifier(node)).inserted else { return nil }
                if node.accessibilityIdentifier?() == identifier { return node }
                for child in node.accessibilityChildren?() ?? [] {
                    if let result = find(child as AnyObject) { return result }
                }
                return nil
            }
            let control = try XCTUnwrap(find(window), "\(scenario): missing action \(identifier)")
            XCTAssertTrue(control.accessibilityPerformPress?() ?? false, "\(scenario): press failed: \(identifier)")
            await LayoutProbeHarness.settleAsync(try XCTUnwrap(window.contentView))
        }
        XCTAssertFalse(window.isKeyWindow)
        XCTAssertFalse(NSApp.isActive)
        XCTAssertGreaterThan(audit(window, scenario: scenario), 0)
    }

    private func host<V: View>(_ view: V, scenario: String, press: [String] = []) async throws {
        let bounds = NSRect(x: 0, y: 0, width: 1000, height: 1800)
        let host = NSHostingView(rootView: LayoutProbeHarness.root(view))
        let window = LayoutProbeHarness.window(contentRect: bounds, styleMask: .titled, backing: .buffered, defer: false)
        window.contentView = host
        host.frame = bounds
        window.orderBack(nil)
        try await inspect(window, scenario: scenario, press: press)
    }

    func testLibraryAndDevelopWindows() async throws {
        LayoutProbeHarness.prepare()
        let scratch = FileManager.default.temporaryDirectory.appendingPathComponent("library-develop-ax-\(UUID().uuidString)")
        let folder = scratch.appendingPathComponent("photos")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: scratch) }
        let model = AppModel()
        defer { model.closeDevelop() }
        let empty = ShellHarness.window(model, size: CGSize(width: 2200, height: 900), dark: true).0
        try await inspect(empty, scenario: "library.empty")
        try ShellHarness.writeJPEG(folder.appendingPathComponent("photo.jpg"), shade: 100)
        model.install(try EngineLibrary.scan(folder: folder, appSupport: scratch.appendingPathComponent("support")))
        let populated = ShellHarness.window(model, size: CGSize(width: 2200, height: 900), dark: true).0
        try await inspect(populated, scenario: "library.populated")
        // A fresh model avoids retained SwiftUI loupe observers starting a competing open.
        let editingModel = AppModel()
        editingModel.install(try EngineLibrary.scan(folder: folder, appSupport: scratch.appendingPathComponent("develop-support")))
        try await auditDevelop(editingModel)
    }

    private func auditDevelop(_ model: AppModel) async throws {
        defer { model.closeDevelop() }
        model.enterPhotoEdit()
        model.openDevelop(for: try XCTUnwrap(model.focusedItem))
        let deadline = Date().addingTimeInterval(30)
        while model.developStatus == .loading, Date() < deadline { try await Task.sleep(for: .milliseconds(20)) }
        XCTAssertEqual(model.developStatus, .ready)
        let controller = try XCTUnwrap(model.develop)
        controller.set(.exposure, 0.5, interactive: false)
        try controller.snapshot(named: "AX snapshot")
        let support = FileManager.default.temporaryDirectory.appendingPathComponent("ax-presets-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: support) }
        let tools: DevelopTools = {
            let previous = ProcessInfo.processInfo.environment["TESSERA_APP_DIR"]
            setenv("TESSERA_APP_DIR", support.path, 1)
            defer {
                if let previous { setenv("TESSERA_APP_DIR", previous, 1) }
                else { unsetenv("TESSERA_APP_DIR") }
            }
            return DevelopTools(model: model)
        }()
        guard tools.presetStore.folder.standardizedFileURL.path == support.appendingPathComponent("Presets").standardizedFileURL.path else {
            return XCTFail("Preset fixtures must use the disposable support directory")
        }
        tools.savePreset(name: "AX preset", groups: [.basicTone])
        XCTAssertEqual(tools.presets.count, 1)
        tools.refreshHistory()
        for title in ["Editing target", "Histogram", "Basic", "Tone Curve", "HSL / Color", "Color Grading", "Detail", "Transform", "Effects", "Lens Blur", "Crop & Straighten", "HDR", "Soft Proofing", "Presets", "Snapshots", "History", "Masks"] {
            let key = "InspectorPanel." + title
            let previous = UserDefaults.standard.object(forKey: key)
            UserDefaults.standard.set(true, forKey: key)
            addTeardownBlock {
                if let previous { UserDefaults.standard.set(previous, forKey: key) }
                else { UserDefaults.standard.removeObject(forKey: key) }
            }
        }
        let develop = ShellHarness.window(model, size: CGSize(width: 2200, height: 1800), dark: true).0
        try await inspect(develop, scenario: "develop.window")
        let panels: [(String, AnyView)] = [
            ("basic", AnyView(BasicPanel(model: model))),
            ("tone", AnyView(ToneCurvePanel(model: model, tools: tools))),
            ("colour", AnyView(HSLPanel(model: model, tools: tools))),
            ("grading", AnyView(ColorGradingPanel(model: model, tools: tools))),
            ("detail", AnyView(DetailPanel(model: model, tools: tools))),
            ("transform", AnyView(TransformPanel(model: model, tools: tools, guideTool: .shared))),
            ("effects", AnyView(EffectsPanel(model: model, tools: tools))),
            ("lensBlur", AnyView(LensBlurPanel(model: model, tools: tools))),
            ("crop", AnyView(CropPanel(model: model, tools: tools))),
            ("hdr", AnyView(HDRPanel(model: model, tools: tools))),
            ("proof", AnyView(SoftProofPanel(proof: .shared))),
            ("presets", AnyView(PresetsPanel(model: model, tools: tools))),
            ("snapshots", AnyView(SnapshotsPanel(model: model))),
            ("history", AnyView(HistoryPanel(model: model, tools: tools)))
        ]
        for (name, panel) in panels {
            try await host(panel.frame(width: 380).developContext(model, tools), scenario: "develop." + name)
        }
        for index in 0..<3 {
            try await host(HSLPanel(model: model, tools: tools).developContext(model, tools),
                           scenario: "develop.hsl.\(index)", press: ["develop.hsl.property.\(index)"])
        }
        for index in 1..<5 {
            try await host(ColorGradingPanel(model: model, tools: tools).developContext(model, tools),
                           scenario: "develop.grading.\(index)", press: ["develop.grading.mode.\(index)"])
        }
        tools.beginCrop()
        try await host(CropPanel(model: model, tools: tools).developContext(model, tools), scenario: "develop.cropActive")
        tools.cancelCrop()
        tools.curveMode = .point
        try await host(ToneCurvePanel(model: model, tools: tools).developContext(model, tools), scenario: "develop.pointCurve")
        let masks = MaskTools(model: model)
        let group = try XCTUnwrap(controller.addMask(LinearGradientShape(start: (0.2, 0.2), end: (0.8, 0.8)).json))
        controller.addMaskComponent(group, LinearGradientShape(start: (0.1, 0.8), end: (0.7, 0.2)).json,
                                    combine: .subtract)
        masks.refresh()
        masks.select(group)
        XCTAssertNotNil(masks.selected)
        masks.setActive(true)
        masks.tool = .brush
        try await host(MaskToolbar(model: model, masks: masks), scenario: "develop.maskToolbar")
        try await host(MasksPanel(model: model, masks: masks).frame(width: 380), scenario: "develop.selectedMask")
    }

    func testSidebarNativeSelectionAndDisclosureArePreserved() async throws {
        LayoutProbeHarness.prepare()
        let model = AppModel()
        model.install(StubLibrary.synthetic(count: 4))
        let controller = SidebarController(model: model)
        var snapshot = SidebarSnapshot(model: model)
        snapshot.engineBacked = true
        snapshot.nodes = CollectionNode.tree([
            LibraryNode(id: 1, kind: .group, name: "AX group", parent: nil, depth: 0, handle: nil, imageCount: 0, rule: nil, scoped: false),
            LibraryNode(id: 2, kind: .album, name: "AX album", parent: 1, depth: 1, handle: "AX album", imageCount: 0, rule: nil, scoped: false),
        ])
        controller.apply(snapshot)
        let bounds = NSRect(x: 0, y: 0, width: 380, height: 900)
        let window = LayoutProbeHarness.window(contentRect: bounds, styleMask: .titled, backing: .buffered, defer: false)
        window.contentView = controller.scrollView
        controller.scrollView.frame = bounds
        window.orderBack(nil)
        defer { LayoutProbeHarness.dispose(window) }
        await LayoutProbeHarness.settleAsync(controller.scrollView)
        let outline = controller.outline
        let rows = outline.accessibilityChildren()?.compactMap { $0 as? SidebarRowView } ?? []
        let all = try XCTUnwrap(rows.first { $0.accessibilityIdentifier() == "library.sidebar.row.src:all" })
        outline.deselectAll(nil)
        all.setAccessibilitySelected(true)
        XCTAssertTrue(outline.isRowSelected(outline.row(for: all)))
        all.setAccessibilitySelected(false)
        XCTAssertFalse(outline.isRowSelected(outline.row(for: all)))
        let header = try XCTUnwrap(rows.first { $0.accessibilityIdentifier() == "library.sidebar.row.node:1" })
        let item = try XCTUnwrap(outline.item(atRow: outline.row(for: header)))
        header.setAccessibilityDisclosed(false)
        XCTAssertFalse(outline.isItemExpanded(item))
        header.setAccessibilityDisclosed(true)
        XCTAssertTrue(outline.isItemExpanded(item))
        XCTAssertFalse(window.isKeyWindow)
        XCTAssertFalse(NSApp.isActive)
    }

    func testPressActionIsAuditedRegardlessOfRole() {
        XCTAssertEqual(audit(LibraryPressableFixture(), scenario: "library.pressable"), 1)
        XCTAssertFalse(offersAccessibilityPress(NSView()))
        XCTAssertFalse(offersAccessibilityPress(NSAccessibilityElement()))
        let element = LibraryPressableElement()
        element.setAccessibilityRole(.group)
        element.setAccessibilityIdentifier("library.test.pressableElement")
        element.setAccessibilityLabel("Pressable element")
        XCTAssertEqual(audit(element, scenario: "library.pressableElement"), 1)
        XCTAssertFalse(offersAccessibilityPress(NSTextField(labelWithString: "Passive")))
    }

    func testImportStepsAndSyntheticReport() async throws {
        LayoutProbeHarness.prepare()
        let importer = LightroomImportController()
        importer.folders = FolderMappingTable(options: LrcatOptions(libraryFolder: "/Synthetic Photos",
            relocations: [LrcatRelocation(from: "/Synthetic Old", to: "/Synthetic New")], marks: [], overwriteExistingEdits: false))
        importer.marks = MarkMappingTable(rows: [LrcatMarkRow(label: "Client choice", mark: "Client choice", count: 2)])
        importer.fidelity = FidelityGrid(samples: [LrcatFidelitySample(catalogId: 1, name: "fixture.jpg", path: "/fixture.jpg",
            status: .failed, message: "Synthetic renderer warning", deltaEMean: 0, deltaEP95: 0, lightroomJpeg: Data(), tesseraJpeg: Data())])
        for step in LightroomImportController.Step.allCases {
            importer.step = step
            try await host(LightroomImportSheet(importer: importer), scenario: "library.import." + String(describing: step))
        }
        let report = LrcatReport(catalogPath: "/Fixture.lrcat", cancelled: false, imported: 4, resumed: 1,
            virtualCopies: 2, skipped: [LrcatSkip(name: "lost.jpg", path: "/lost.jpg", reason: "original not found")],
            unsupported: [LrcatIssue(category: "Develop", reason: "Synthetic unsupported setting", count: 9, examples: ["/photos/portrait.jpg", "/photos/two.jpg", "/photos/three.jpg", "/photos/four.jpg", "/photos/five.jpg"])],
            approximate: [LrcatIssue(category: "PointColors", reason: "hue range semantics unverified", count: 3,
                                     examples: ["/photos/portrait.jpg", "/photos/two.jpg"])],
            albums: 5, albumGroups: 6, smartAlbums: 7, keywords: 8,
            selection: LrcatSelectionCounts(rejects: 0, keeps: 0, undecided: 0, grade1: 0, grade2: 0, grade3: 0, marked: 0),
            libraryPath: "/Photos/library.json", bundlePath: "/Photos/bundle", indexed: 5, seconds: 0.1)
        importer.publishReport(report, markdown: "Synthetic report", url: URL(fileURLWithPath: "/Synthetic import-report.md"))
        try await host(LightroomImportSheet(importer: importer), scenario: "library.import.reportGroups")
        var cancelled = report
        cancelled.cancelled = true
        importer.publishReport(cancelled, markdown: "Synthetic cancelled report", url: nil)
        try await host(LightroomImportSheet(importer: importer), scenario: "library.import.cancelledReport")
    }
}

private final class LibraryPressableFixture: NSObject {
    @objc func accessibilityRole() -> NSAccessibility.Role { .group }
    @objc func accessibilityIdentifier() -> String { "library.test.pressable" }
    @objc func accessibilityLabel() -> String { "Pressable group" }
    @objc func accessibilityPerformPress() -> Bool { true }
}

private final class LibraryPressableElement: NSAccessibilityElement {
    override func accessibilityPerformPress() -> Bool { true }
}
