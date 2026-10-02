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
            for secret in ["AXPrivatePreset731", "AXPrivateSnapshot731", "AXPrivateKeyword731", "AXPrivateRoot731"] {
                XCTAssertFalse((id.removingPercentEncoding ?? id).contains(secret), "User data leaked into identifier: \(id)")
            }
            let here = "\(path)/\(role)[\(id)]{\(label)}"
            // Scrollbar arrows/thumbs are AppKit implementation details, not document controls.
            if role == "AXScrollBar" { return }
            // macOS-owned window chrome is outside the app's identifier namespace.
            if let subrole = node.accessibilitySubrole?()?.rawValue,
               ["AXCloseButton", "AXMinimizeButton", "AXZoomButton", "AXFullScreenButton"].contains(subrole) { return }
            // The native sidebar toggle is owned by SwiftUI/AppKit and has no identifier hook.
            if id.isEmpty && ["Toggle Sidebar", "Show Sidebar", "Hide Sidebar"].contains(label) { return }
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
                if id.isEmpty || label.isEmpty {
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
        case "library.populated": required = ["ruleTextField", "library.toolbar.open", "library.sidebar.row.src:all"]
        case "develop.basic": required = ["develop.basic.exposure"]
        case "develop.selectedMask": required = ["develop.masks.amount", "develop.masks.reset"]
        case "library.import.reportGroups": required = ["document.import.report.warnings", "document.import.report.approximate", "document.import.report.markdown"]
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
        try controller.snapshot(named: "AXPrivateSnapshot731")
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
        tools.savePreset(name: "AXPrivatePreset731", groups: [.basicTone])
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
        let other = try XCTUnwrap(rows.first { $0 !== all && outline.row(for: $0) >= 0 })
        outline.selectRowIndexes(IndexSet(integer: outline.row(for: other)), byExtendingSelection: false)
        all.setAccessibilitySelected(true)
        XCTAssertEqual(outline.selectedRowIndexes, IndexSet(integer: outline.row(for: all)))
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


    func testEstablishedIdentifiersFromMainRemainPresent() throws {
        let tests = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        let sources = tests.deletingLastPathComponent().deletingLastPathComponent().appendingPathComponent("Sources")
        let enumerator = try XCTUnwrap(FileManager.default.enumerator(at: sources, includingPropertiesForKeys: nil))
        var source = ""
        for case let url as URL in enumerator where url.pathExtension == "swift" {
            source += try String(contentsOf: url, encoding: .utf8)
        }
        // Compare actual identifier expressions, not comments or arbitrary prefix matches.
        let calls = try NSRegularExpression(pattern: #"(?:accessibilityIdentifier(?:IfPresent)?|setAccessibilityIdentifier)\([^\n]*"#)
        let strings = try NSRegularExpression(pattern: #""((?:\\.|[^"\\])*)""#)
        var present = Set<String>()
        for match in calls.matches(in: source, range: NSRange(source.startIndex..., in: source)) {
            let call = String(source[try XCTUnwrap(Range(match.range, in: source))])
            for string in strings.matches(in: call, range: NSRange(call.startIndex..., in: call)) {
                let value = String(call[try XCTUnwrap(Range(string.range(at: 1), in: call))])
                present.insert(value.components(separatedBy: #"\("#)[0])
            }
        }
        for stem in EstablishedAccessibilityIdentifiers.stems {
            XCTAssertTrue(present.contains(stem), "Established identifier missing: \(stem)")
        }
    }

    func testSidebarFolderIdentifierPrivacy() {
        let row = SidebarRow(.folder(URL(fileURLWithPath: "/AXPrivateRoot731")), key: "folder:/AXPrivateRoot731", title: "AXPrivateRoot731")
        let cell = SidebarCell()
        cell.configure(row)
        for child in cell.subviews {
            XCTAssertFalse((child.accessibilityIdentifier().removingPercentEncoding ?? "").contains("AXPrivateRoot731"))
        }
    }

    func testMetadataUpdatesAreConditionalAndSidebarRemainsNative() throws {
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().appendingPathComponent("Sources/Tessera")
        let slider = try String(contentsOf: root.appendingPathComponent("Inspector/DevelopPanels.swift"), encoding: .utf8)
        let thumbnail = try String(contentsOf: root.appendingPathComponent("Grid/ThumbnailCell.swift"), encoding: .utf8)
        let shell = try String(contentsOf: root.appendingPathComponent("Shell/ContentView.swift"), encoding: .utf8)
        XCTAssertTrue(slider.contains("if s.accessibilityIdentifier() != identifier"))
        XCTAssertTrue(slider.contains("if s.accessibilityLabel() != label"))
        XCTAssertTrue(thumbnail.contains("if v.accessibilityIdentifier() != identifier"))
        XCTAssertTrue(thumbnail.contains("if cellView.accessibilityLabel() != label"))
        XCTAssertTrue(shell.contains(".toolbar(removing: model.viewMode == .document ? .sidebarToggle : nil)"))
    }

    func testKeywordIdentifierPrivacy() async throws {
        LayoutProbeHarness.prepare()
        try await host(KeywordChip(name: "AXPrivateKeyword731", mixed: false) {}, scenario: "library.keywordPrivacy")
        let model = AppModel()
        let understanding = UnderstandingController()
        understanding.chips.replace(with: [SuggestedKeyword(keyword: "AXPrivateKeyword731", confidence: 0.9)])
        try await host(SuggestedKeywordsSection(model: model, understanding: understanding), scenario: "library.suggestionPrivacy")
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
            relocations: [LrcatRelocation(from: "/AXPrivateRoot731", to: "/Synthetic New")], marks: [], overwriteExistingEdits: false))
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

// Identifier stems captured from origin/main before B5-50b. Dynamic payloads may use private indexes.
enum EstablishedAccessibilityIdentifiers {
    static let stems = [
        "agent-group",
        "agent-group-amount",
        "agent-group-amount-readout",
        "agent-group-instruction",
        "agent-group-redo",
        "agent-provenance",
        "agent-review-accept",
        "agent-review-confidence",
        "agent-review-inspector",
        "agent-review-instruction",
        "agent-review-list",
        "agent-review-redo",
        "agent-review-revert",
        "agent-review-row",
        "agent-step-rationale",
        "agent-step-toggle-",
        "ai-allow-model-downloads",
        "ai-auto-suggest",
        "ai-caption-models",
        "ai-default-provider",
        "ai-key-",
        "ai-key-status-",
        "ai-profile-status",
        "ai-write-suggested-xmp",
        "analysis-progress",
        "assist-confirm-all",
        "assist-explanation",
        "assist-panel-toggle",
        "assist-pkeep",
        "assist-status",
        "autoedit-blocker",
        "autoedit-cancel",
        "autoedit-progress",
        "autoedit-provider",
        "autoedit-scope",
        "autoedit-start",
        "cached-preview-library-mode",
        "caption-draft-discard",
        "caption-draft-save",
        "defect-count",
        "detail-ai-denoise",
        "detail-ai-denoise-amount",
        "detail-ai-denoise-ignored",
        "develop-save-recovery",
        "develop-source-badge",
        "document-export-progress",
        "document.brushes",
        "document.brushes.import",
        "document.brushes.preset.",
        "document.cam.apply",
        "document.cam.cancel",
        "document.cam.color",
        "document.cam.error",
        "document.cam.mode",
        "document.cam.offset",
        "document.cameraRaw.busy",
        "document.channels",
        "document.channels.",
        "document.channels.add",
        "document.channels.delete",
        "document.channels.list",
        "document.channels.load",
        "document.channels.load.invert",
        "document.channels.options.color",
        "document.channels.options.indicates",
        "document.channels.options.name",
        "document.channels.quickMask",
        "document.channels.save",
        "document.channels.save.name",
        "document.channels.sheet.ok",
        "document.channels.spot.color",
        "document.channels.spot.fromSelection",
        "document.channels.spot.name",
        "document.channels.spotNote",
        "document.color.default",
        "document.color.swap",
        "document.colorPanel",
        "document.colors",
        "document.colors.background",
        "document.colors.default",
        "document.colors.foreground",
        "document.colors.swap",
        "document.empty.new",
        "document.empty.open",
        "document.export.color",
        "document.export.format",
        "document.export.quality",
        "document.export.start",
        "document.globalLight.dial",
        "document.globalLight.done",
        "document.history",
        "document.history.height.value",
        "document.history.memory",
        "document.history.newSnapshot",
        "document.history.resize",
        "document.history.row.",
        "document.history.snapshot.",
        "document.history.toggle",
        "document.import.report.approximate",
        "document.import.report.fidelity",
        "document.import.report.markdown",
        "document.import.report.summary",
        "document.import.report.warnings",
        "document.inspector.shortcut.",
        "document.inspector.stack",
        "document.inspector.tabs",
        "document.layerStyle.",
        "document.layerStyle.add.",
        "document.layerStyle.addMenu",
        "document.layerStyle.delete",
        "document.layerStyle.detail",
        "document.layerStyle.done",
        "document.layerStyle.editor.",
        "document.layerStyle.enable.",
        "document.layerStyle.globalLight",
        "document.layerStyle.list",
        "document.layerStyle.row.",
        "document.layers",
        "document.layers.add",
        "document.layers.addAdjustment",
        "document.layers.addMask",
        "document.layers.addStyle",
        "document.layers.blendMode",
        "document.layers.delete",
        "document.layers.filter",
        "document.layers.fx.",
        "document.layers.group",
        "document.layers.lock.",
        "document.layers.outline",
        "document.layers.row.",
        "document.liquify.apply",
        "document.liquify.cancel",
        "document.liquify.canvas",
        "document.liquify.error",
        "document.liquify.latency",
        "document.liquify.output",
        "document.liquify.reconstruct",
        "document.liquify.restoreAll",
        "document.liquify.showMask",
        "document.liquify.showMesh",
        "document.liquify.showOriginal",
        "document.liquify.thawAll",
        "document.liquify.tool.",
        "document.neural.apply",
        "document.neural.cancel",
        "document.neural.error",
        "document.neural.filter.",
        "document.neural.missing",
        "document.neural.output",
        "document.neural.reset",
        "document.neural.settings",
        "document.neural.stopping",
        "document.new.create",
        "document.new.depth",
        "document.new.height",
        "document.new.preset",
        "document.new.profile",
        "document.new.width",
        "document.option.",
        "document.option.blendMode",
        "document.option.brushPresets",
        "document.option.combine",
        "document.option.eyedropperRadius",
        "document.option.pressureOpacity",
        "document.option.pressureSize",
        "document.option.selectAndMask",
        "document.option.selectSubject",
        "document.option.symmetry",
        "document.option.textFamily",
        "document.option.zoomActual",
        "document.option.zoomFit",
        "document.optionsBar",
        "document.properties",
        "document.properties.",
        "document.properties.auto.black",
        "document.properties.auto.gamma",
        "document.properties.auto.mode",
        "document.properties.auto.white",
        "document.properties.bounds",
        "document.properties.channel",
        "document.properties.channelMixer.monochrome",
        "document.properties.channelMixer.output",
        "document.properties.colorBalance.tone",
        "document.properties.colorLookup.dither",
        "document.properties.colorLookup.file",
        "document.properties.curves.reset",
        "document.properties.editContents",
        "document.properties.fill.gradientKind",
        "document.properties.groupMode",
        "document.properties.hueSaturation.colorize",
        "document.properties.kind",
        "document.properties.name",
        "document.properties.selectiveColor.method",
        "document.properties.style.",
        "document.properties.style.edit",
        "document.remove.backend",
        "document.remove.cancel",
        "document.remove.distractions",
        "document.remove.download",
        "document.remove.downloads-off",
        "document.remove.error",
        "document.remove.notice",
        "document.remove.review.all",
        "document.remove.review.apply",
        "document.remove.review.cancel",
        "document.remove.review.none",
        "document.remove.review.summary",
        "document.remove.selection",
        "document.remove.settings",
        "document.remove.stopping",
        "document.remove.waiting",
        "document.saveAs.cancel",
        "document.saveAs.choose",
        "document.saveAs.folder",
        "document.saveAs.format",
        "document.saveAs.name",
        "document.saveAs.save",
        "document.shape.",
        "document.shape.convert",
        "document.shape.fillRule",
        "document.shape.inspector",
        "document.shape.mask.add",
        "document.shape.mask.delete",
        "document.shape.mask.enabled",
        "document.shape.mask.fromSelection",
        "document.shape.mask.linked",
        "document.shape.polygon.star",
        "document.shape.rect.linkRadii",
        "document.shape.stroke.alignment",
        "document.shape.stroke.cap",
        "document.shape.stroke.dashes",
        "document.shape.stroke.join",
        "document.smartFilter.blending.mode",
        "document.smartFilter.blending.ok",
        "document.status.canvas",
        "document.status.message",
        "document.status.render",
        "document.status.selection",
        "document.status.stroke",
        "document.status.zoom",
        "document.tabs",
        "document.tabs.",
        "document.tabs.new",
        "document.tabs.overflow",
        "document.text.alignment",
        "document.text.apply",
        "document.text.cancel",
        "document.text.convert",
        "document.text.family",
        "document.text.kerning",
        "document.text.latency",
        "document.text.limitation",
        "document.text.optionsApply",
        "document.text.optionsCancel",
        "document.text.source",
        "document.text.style",
        "document.text.toggleBox",
        "document.tool.",
        "document.tool.contentAwareMove",
        "document.tool.remove",
        "document.toolbar.inspector",
        "document.toolbar.library",
        "document.toolbar.open",
        "document.toolbar.sidebar",
        "document.tools",
        "document.transform.apply",
        "document.transform.cancel",
        "document.transform.commit",
        "document.transform.grid",
        "document.transform.interpolation",
        "document.transform.latency",
        "document.transform.previewState",
        "document.transform.protectChannel",
        "document.transform.puppetMode",
        "document.transform.refusal",
        "document.transform.reset",
        "document.transform.splitHorizontal",
        "document.transform.splitVertical",
        "document.transform.warpPreset",
        "document.viewport",
        "document.zoomHUD",
        "enhance-allow-download",
        "enhance-cancel",
        "enhance-denoise",
        "enhance-error",
        "enhance-last-error",
        "enhance-output",
        "enhance-preview-note",
        "enhance-problem",
        "enhance-raw-details",
        "enhance-sheet",
        "enhance-start",
        "enhance-super-resolution",
        "export-avif-speed",
        "export-bit-depth",
        "export-cancel",
        "export-color-space",
        "export-color-space-note",
        "export-destination",
        "export-dng-note",
        "export-error",
        "export-format",
        "export-hdr",
        "export-jxl-lossless",
        "export-long-edge",
        "export-naming",
        "export-naming-example",
        "export-preset",
        "export-progress",
        "export-quality",
        "export-size-limit",
        "export-size-limit-kb",
        "export-start",
        "export-summary",
        "export-target",
        "export-watermark-anchor",
        "export-watermark-anchor-",
        "export-watermark-choose",
        "export-watermark-color",
        "export-watermark-font",
        "export-watermark-graphic",
        "export-watermark-kind",
        "export-watermark-preview",
        "export-watermark-problem",
        "export-watermark-render",
        "export-watermark-rotation",
        "export-watermark-text",
        "export-watermark-unavailable",
        "face-chip-",
        "face-confirm-",
        "face-filter-eyes-closed",
        "face-filter-person",
        "face-member-",
        "face-name-person",
        "face-strip",
        "face-zoom",
        "facet",
        "facetAlbum",
        "facetPerson",
        "filmstrip",
        "filterDiagnostic",
        "filterMatchCount",
        "grid",
        "hdr-headroom",
        "hdr-status",
        "hdr-toggle",
        "iptc-",
        "keyword-suggest-selection",
        "keyword-suggestion-",
        "keyword-suggestion-reject-",
        "keyword-suggestion-threshold",
        "keyword-suggestions",
        "keyword-suggestions-accept-all",
        "keywordEntry",
        "lensblur-amount",
        "lensblur-apply",
        "lensblur-bokeh",
        "lensblur-busy",
        "lensblur-error",
        "lensblur-focal-range",
        "lensblur-refine-blur",
        "lensblur-refine-focus",
        "lensblur-refine-unavailable",
        "lensblur-subject",
        "lensblur-visualize-depth",
        "loupe-display-info",
        "loupe-display-info-details",
        "loupe-display-photo-name",
        "loupe-shortcut-details",
        "loupe-shortcuts",
        "lrimport-cancel",
        "lrimport-folders",
        "lrimport-keywords",
        "lrimport-library-folder",
        "lrimport-locate",
        "lrimport-lock-warning",
        "lrimport-looks-different",
        "lrimport-mark-",
        "lrimport-plan-line",
        "lrimport-progress",
        "lrimport-report-path",
        "lrimport-report-status",
        "lrimport-selection",
        "lrimport-steps",
        "lrimport-summary-counts",
        "metadata-generate-caption",
        "ocr-detect",
        "ocr-find",
        "ocr-text",
        "people-analyze-faces",
        "people-approximate-note",
        "people-clear-filter",
        "people-count",
        "people-eyes-closed-",
        "people-grid",
        "people-refit",
        "people-refreshing",
        "people-setting-person-keywords",
        "people-setting-write-regions",
        "people-view",
        "person-confirm-all",
        "person-confirmed-",
        "person-detail-back",
        "person-detail-counts",
        "person-detail-name",
        "person-faces",
        "person-move-target-",
        "person-move-targets",
        "person-name-",
        "person-name-field-",
        "person-show-photos",
        "person-split",
        "person-suggestions-",
        "person-tile-",
        "photo-edit-inspector-tabs",
        "photo-edit-target",
        "photo-job-cancel",
        "photo-job-progress",
        "photo-merge-auto-align",
        "photo-merge-auto-tone",
        "photo-merge-bracket-size",
        "photo-merge-cancel",
        "photo-merge-create-stack",
        "photo-merge-deghost",
        "photo-merge-error",
        "photo-merge-fill-edges",
        "photo-merge-focal",
        "photo-merge-output",
        "photo-merge-preview",
        "photo-merge-preview-busy",
        "photo-merge-preview-error",
        "photo-merge-problem",
        "photo-merge-projection",
        "photo-merge-sheet",
        "photo-merge-start",
        "photo-merge-warnings",
        "photomerge-fill",
        "photomerge-into-current",
        "photomerge-layout",
        "photomerge-sheet",
        "photomerge-sources",
        "print-cancel",
        "print-error",
        "print-layout",
        "print-page-count",
        "print-paper",
        "print-progress",
        "print-save-pdf",
        "renderReadout",
        "reopen-cached-preview-library",
        "review-accept-next",
        "review-critic-status",
        "review-current-preview",
        "review-edit-photo",
        "review-empty",
        "review-persistence-warning",
        "review-photo-",
        "review-revert",
        "review-row-busy",
        "review-user-status",
        "ruleDiagnostic",
        "ruleGroupKind",
        "ruleTextField",
        "sidebar-people",
        "sidebarAddMenu",
        "sidebarOutline",
        "smart-preview-",
        "smart-preview-check-status",
        "smart-preview-local-save-warning",
        "smart-preview-thumbnail-limitation",
        "smartAlbumMatchCount",
        "softproof-badge",
        "softproof-status",
        "softproof-toggle",
        "src:people",
        "stack-align-layout",
        "stack-align-reference",
        "stack-align-sheet",
        "stack-blend-fill",
        "stack-blend-method",
        "stack-blend-sheet",
        "stack-blend-tones",
        "stack-busy",
        "stack-busy-cancel",
        "stack-lens",
        "status-person-filter",
        "tether-album",
        "tether-auto-advance",
        "tether-capture",
        "tether-close",
        "tether-connect",
        "tether-connected",
        "tether-device-",
        "tether-device-list",
        "tether-device-readouts",
        "tether-disconnect",
        "tether-error",
        "tether-folder",
        "tether-folder-choose",
        "tether-frame-",
        "tether-incoming",
        "tether-interval-count",
        "tether-interval-seconds",
        "tether-interval-status",
        "tether-interval-toggle",
        "tether-naming",
        "tether-naming-example",
        "tether-naming-token",
        "tether-no-camera",
        "tether-notice",
        "tether-panel",
        "tether-pending",
        "tether-refresh",
        "tether-session-name",
        "tether-smart-album",
        "tether-summary",
        "toast",
        "toolbar-agent-review",
        "toolbar-assist",
        "toolbar-assist-menu",
        "toolbar-auto-edit",
        "toolbar-people-merge",
        "transform-",
        "transform-constrain-crop",
        "transform-constrain-crop-unavailable",
        "transform-guides-clear",
        "transform-guides-count",
        "transform-guides-done",
        "transform-guides-uncorrected",
        "transform-preview-note",
        "transform-upright",
        "transform-upright-",
        "understanding-cancel",
        "understanding-progress",
        "use-existing-offline-smart-preview",
        "use-smart-previews",
        "workspace-back-to-library",
        "workspace-command-scope",
        "workspace-create-layered-copy",
        "workspace-edit-photo",
        "workspace-photo-target",
    ]
}
