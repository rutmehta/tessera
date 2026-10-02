import AppKit
import SwiftUI
import XCTest
import TesseraCore
import TesseraFFI
@testable import Tessera

/// B5-49. Direct event delivery, never a key window. The JSONL deliberately identifies itself
/// as a hosted trace: production InspectorFocusTrace.routeEvent excludes non-key windows.
///
/// B5-49c: the checklist runs twice, with Full Keyboard Access pinned on and pinned off in process
/// (`KeyboardAccessHarness`), so neither run depends on the machine's own setting. Each variant
/// asserts that mode's behaviour; what the pin cannot control is reported N/A with the reason.
@MainActor
final class DocumentKeyboardChecklistTests: XCTestCase {
    private struct NotApplicable: Error { let reason: String }
    private struct Failure: Error, CustomStringConvertible { let description: String }
    private struct Row {
        let step: String, result: String, sequence: String, nativeType: String, handled: String, note: String
    }
    private struct Event: Encodable {
        let sequence: Int
        let step: String
        let source = "background-hosted-direct-delivery"
        let before: InspectorFocusTrace.Snapshot
        let handled: Bool
        let keyCode: UInt16
        let keyUp: Bool
        let repeatKey: Bool
        let document: Bool
        let keyWindow: Bool
        let layerCountBefore: Int
        let focusedIdentifierAfter: String
    }
    private var rows: [Row] = []
    private var events: [Event] = []
    private var step = ""
    private var fka = false
    private var layoutFailed = false
    private var window: NSWindow!
    private var host: NSView!
    private var model: AppModel!
    private var router: KeyRouter!

    private struct Fixture: View {
        let model: AppModel
        var body: some View {
            HStack(spacing: 0) {
                ZStack {
                    ThumbnailBrowser(model: model, style: .grid)
                        .opacity(model.viewMode == .grid ? 1 : 0)
                        .allowsHitTesting(model.viewMode == .grid)
                    if model.viewMode == .document { DocumentView(workspace: model.documents) }
                }
                if model.viewMode == .document, !model.documents.panelsHidden {
                    DocumentInspector(workspace: model.documents).frame(width: 288)
                }
            }
        }
    }

    private func require(_ condition: @autoclosure () throws -> Bool, _ message: String) throws {
        if try !condition() { throw Failure(description: message) }
    }
    private func views(_ root: NSView) -> [NSView] { [root] + root.subviews.flatMap { views($0) } }
    private func control<T: NSView>(_ id: String, _: T.Type = T.self) throws -> T {
        guard let found = views(host).first(where: { $0.accessibilityIdentifier() == id }) as? T else {
            throw Failure(description: "Missing required B5-42 control \(id) (\(T.self))")
        }
        return found
    }
    private func focus(_ view: NSView) throws {
        try require(view.window === window && !view.isHiddenOrHasHiddenAncestor, "Focus target is detached/hidden")
        try require(window.makeFirstResponder(view), "Could not establish responder \(view.accessibilityIdentifier())")
    }
    private func settle() {
        // The combined document/library host can span several display turns on a busy build
        // machine. Keep the harness's 50 ms quiet criterion, with a bounded scheduling budget.
        if !LayoutProbeHarness.settle(host, timeout: 5) { layoutFailed = true }
        window.recalculateKeyViewLoop()
    }
    private func doc() throws -> DocumentController { try XCTUnwrap(model.documents.current) }
    @discardableResult
    private func press(_ code: UInt16, _ characters: String, shift: Bool = false,
                       command: Bool = false, repeatKey: Bool = false, up: Bool = false) throws -> Bool {
        try require(!window.isKeyWindow && !window.isMainWindow && !NSApp.isActive, "Background invariant violated")
        let responder = window.firstResponder
        let before = InspectorFocusTrace.snapshot(focused: nil, root: AppKitFocusNode(window),
            window: ObjectIdentifier(window), nativeType: responder.map { String(reflecting: type(of: $0)) } ?? "nil",
            nativeIdentity: responder.map { String(describing: ObjectIdentifier($0)) } ?? "")
        let count = model.documents.current?.layers.count ?? 0
        let documentBefore = model.viewMode == .document
        var modifiers: NSEvent.ModifierFlags = []
        if shift { modifiers.insert(.shift) }; if command { modifiers.insert(.command) }
        let event = try XCTUnwrap(NSEvent.keyEvent(with: up ? .keyUp : .keyDown, location: .zero,
            modifierFlags: modifiers, timestamp: Double(events.count) / 30, windowNumber: window.windowNumber,
            context: nil, characters: characters, charactersIgnoringModifiers: characters,
            isARepeat: repeatKey, keyCode: code))
        let handled = up ? router.handleKeyUp(event) : router.handle(event)
        if !handled { window.sendEvent(event) }
        events.append(Event(sequence: events.count + 1, step: step, before: before, handled: handled,
            keyCode: code, keyUp: up, repeatKey: repeatKey, document: documentBefore,
            keyWindow: window.isKeyWindow, layerCountBefore: count,
            focusedIdentifierAfter: (window.firstResponder as? NSView)?.accessibilityIdentifier() ?? ""))
        return handled
    }
    private func run(_ id: String, success: String = "PASS", _ body: () throws -> String) {
        step = id
        let start = events.count
        layoutFailed = false
        var result = success, note = ""
        do { note = try body() } catch let unavailable as NotApplicable { result = "N/A"; note = unavailable.reason } catch { result = "FAIL"; note = String(describing: error); XCTFail("Step \(id): \(note)") }
        if layoutFailed { result = "FAIL"; note += " LayoutProbeHarness did not settle." }
        let trace = events.dropFirst(start)
        rows.append(Row(step: id, result: result,
            sequence: trace.map { String($0.sequence) }.joined(separator: ", "),
            nativeType: trace.map { $0.before.nativeType }.joined(separator: ", "),
            handled: trace.map { String($0.handled) }.joined(separator: ", "), note: note))
    }
    private func na(_ id: String, _ reason: String) {
        rows.append(Row(step: id, result: "N/A", sequence: "—", nativeType: "not observed",
                        handled: "not delivered", note: reason))
    }
    private func label(_ r: NSResponder?) -> String {
        guard let r else { return "nil" }
        let id = (r as? NSView)?.accessibilityIdentifier() ?? ""
        return id.isEmpty ? String(describing: type(of: r)) : id
    }
    private func canvas() throws -> DocumentViewportView { try control("document.viewport") }
    private func eye() throws -> NSButton { try control("document.layers.row.0.visibility") }
    private func tab(shift: Bool = false) throws -> Bool { try press(48, shift ? "\u{19}" : "\t", shift: shift) }
    private func mouse(_ type: NSEvent.EventType, at point: CGPoint, view: NSView) throws -> NSEvent {
        try XCTUnwrap(NSEvent.mouseEvent(with: type, location: view.convert(point, to: nil), modifierFlags: [],
            timestamp: 0, windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: 1))
    }

    func testCombinedKeyboardChecklistFKAOn() throws {
        try KeyboardAccessHarness.withMode(true) { try checklist(fka: true) }
    }

    func testCombinedKeyboardChecklistFKAOff() throws {
        try KeyboardAccessHarness.withMode(false) { try checklist(fka: false) }
    }

    private func checklist(fka: Bool) throws {
        rows = []; events = []; self.fka = fka
        XCTAssertEqual(KeyboardAccessPolicy.isEnabled, fka, "The app policy must follow the pinned mode")
        XCTAssertEqual(NSApplication.shared.isFullKeyboardAccessEnabled, fka, "AppKit's accessor must follow the pinned mode")
        do { try executeChecklist() }
        catch {
            let note = "Hosted setup or execution could not complete: \(error)"
            rows.append(Row(step: "setup", result: "FAIL", sequence: "—", nativeType: "not observed",
                            handled: "not delivered", note: note))
            XCTFail(note)
        }
        try writeResults(fka: KeyboardAccessPolicy.isEnabled)
        XCTAssertFalse(rows.contains { $0.result == "FAIL" })
        XCTAssertEqual(rows.first { $0.step == "22a" }?.result, "TEARDOWN")
        XCTAssertEqual(rows.first { $0.step == "17a" }?.result, "PASS")
        // The mode-dependent rows. 17b with FKA on additionally depends on the real setting (see its note).
        func result(_ step: String) -> String? { rows.first { $0.step == step }?.result }
        XCTAssertEqual(result("6a"), "PASS")
        for step in ["5", "16b"] { XCTAssertEqual(result(step), fka ? "PASS" : "N/A", "step \(step), FKA \(fka)") }
        if !fka { XCTAssertEqual(result("17b"), "N/A") }
    }

    private func executeChecklist() throws {
        LayoutProbeHarness.prepare()
        let keys = ["DocumentInspector.historyHeight", "InspectorPanel.History", DocumentWorkspace.inspectorTabKey]
        let preferences = keys.map { ($0, UserDefaults.standard.object(forKey: $0)) }
        let tools = DocumentTools.shared.workspace
        let channels = DocumentChannels.shared.workspace
        let vector = DocumentVector.shared.workspace
        let text = DocumentText.shared.workspace
        let transforms = DocumentTransforms.shared.workspace
        defer {
            if let window { LayoutProbeHarness.dispose(window) }
            DocumentTools.shared.workspace = tools; DocumentChannels.shared.workspace = channels
            DocumentVector.shared.workspace = vector; DocumentText.shared.workspace = text
            DocumentTransforms.shared.workspace = transforms
            for (key, value) in preferences {
                if let value { UserDefaults.standard.set(value, forKey: key) }
                else { UserDefaults.standard.removeObject(forKey: key) }
            }
        }
        UserDefaults.standard.set(Double(DocumentInspector.historyDefault), forKey: keys[0])
        UserDefaults.standard.set(true, forKey: keys[1])
        model = AppModel()
        let scratch = FileManager.default.temporaryDirectory.appendingPathComponent("B5-49-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: scratch, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: scratch) }
        for n in 0..<2 { try ShellHarness.writeJPEG(scratch.appendingPathComponent("image-\(n).jpg"), shade: 40 + n * 40) }
        model.install(try StubLibrary.scan(folder: scratch))
        model.documents.documentLoadExecutor = { engine, body, completion in
            completion(Result { try body(engine) })
        }
        model.documents.engine = StubDocumentEngine()
        model.documents.inspectorTab = .stack
        model.documents.newDocument(model.documents.newSettings)
        model.viewMode = .grid
        DocumentTools.shared.workspace = model.documents
        router = KeyRouter(model: model)
        let controller = NSHostingController(rootView: LayoutProbeHarness.root(Fixture(model: model)))
        let size = NSSize(width: 1100, height: 848)
        window = KeyboardTestWindow(contentRect: NSRect(origin: .zero, size: size),
            styleMask: .titled, backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.animationBehavior = .none
        window.contentViewController = controller
        window.setContentSize(size)
        host = controller.view
        host.frame = NSRect(origin: .zero, size: size)
        window.orderBack(nil)
        settle()
        try require(host.bounds.size == size && window.contentLayoutRect.size == size,
                    "The hosted content must be exactly \(size) pt on any display, got \(host.bounds.size)")
        try require(try doc().layers.count >= 3, "Fixture needs at least three layers")

        run("1a") {
            let grid = try XCTUnwrap(self.views(self.host).compactMap { $0 as? ThumbnailCollectionView }.first)
            try self.focus(grid)
            try self.require(self.press(14, "e", command: true), "Command-E not routed")
            try self.require(self.model.layeredCopyRequest != nil, "Command-E did not request layered copy")
            self.model.createRequestedLayeredCopy(); self.settle()
            try self.require(self.model.viewMode == .document, "Confirmed layered copy did not open document")
            while try self.doc().layers.count < 3 { try self.doc().addLayer(.pixel) }
            self.settle()
            try self.require(self.window.firstResponder === self.canvas(), "Entry did not transfer grid focus to viewport")
            try self.require(self.tab(), "First canvas Tab unhandled")
            try self.require(self.model.documents.panelsHidden, "Panels did not hide")
            self.settle()
            try self.require(self.tab(), "Second canvas Tab unhandled")
            try self.require(!self.model.documents.panelsHidden, "Panels did not return")
            self.settle()
            self.model.viewMode = .grid; self.settle(); try self.focus(grid)
            self.model.documents.newDocument(self.model.documents.newSettings); try self.doc().addLayer(.pixel); self.settle()
            try self.require(self.window.firstResponder === self.canvas(), "New document retained grid responder")
            try self.require(self.tab(), "New document Tab unhandled")
            try self.require(self.model.documents.panelsHidden, "New document panels did not hide")
            _ = try self.tab(); self.settle()
            return "Two-JPEG grid → injected Command-E → sheet confirmation action, and New Document action: automatic viewport focus, both Tab toggles; no canvas click."
        }
        na("1b", "Native ⌘N menu dispatch and visible sheet interaction require the application command scene; 1a injects ⌘E, invokes confirmation/New Document actions and checks focus handoff.")
        run("2") {
            let previous = try self.doc()
            self.model.documents.select(self.model.documents.documents[0]); self.settle()
            try self.require(self.window.firstResponder === self.canvas(), "Switch did not retain viewport focus")
            try self.require(self.tab(), "Switch Tab unhandled")
            try self.require(self.model.documents.panelsHidden, "Switch Tab did not hide panels")
            _ = try self.tab(); self.model.documents.select(previous); self.settle()
            return "Switch existing documents without a canvas click."
        }
        run("3") {
            try self.focus(self.canvas())
            try self.require(!self.tab(shift: true), "Shift-Tab consumed")
            try self.require(!self.model.documents.panelsHidden, "Shift-Tab hid panels")
            return "Canvas Shift-Tab stays native."
        }
        run("4") {
            let plus: NSButton = try self.control("document.history.height.increase")
            plus.performClick(nil); self.settle()
            for id in ["decrease", "increase", "reset"] {
                let button: NSButton = try self.control("document.history.height.\(id)")
                try self.require(button.isEnabled, "History \(id) disabled")
            }
            let readout: NSTextField = try self.control("document.history.height.value")
            return "Stack/History expanded; AX-identified + clicked once; H0 = \(readout.stringValue). No key event."
        }
        run("5") {
            let outline: LayersOutlineView = try self.control("document.layers.outline")
            outline.selectRowIndexes(IndexSet(integer: 0), byExtendingSelection: false)
            let eye = try self.eye()
            try self.require(outline.row(for: eye) == 0, "Row 0 eye identifier does not belong to row 0")
            try self.require(eye.canBecomeKeyView == self.fka, "Pinned FKA=\(self.fka) does not control the eye's key-view eligibility")
            try self.focus(outline)
            try self.require(!self.tab(), "List Tab consumed")
            let first = self.window.firstResponder
            try self.require(!self.model.documents.panelsHidden, "List Tab hid panels")
            guard self.fka else {
                try self.require(first !== eye && first !== outline, "FKA off: List Tab must skip the eye and leave the list; got \(self.label(first))")
                throw NotApplicable(reason: "FKA off (pinned): the row eye is not a key view (canBecomeKeyView=false), so no eye ring exists in this mode. Asserted instead: List Tab is unhandled, skips the eye, lands on \(self.label(first)), panels stay. Button key safety is automated with a forced responder in 9–10.")
            }
            try self.require(first === eye, "List Tab did not reach selected row eye; got \(self.label(first))")
            try self.require(!self.tab(), "Eye Tab consumed")
            let second = self.window.firstResponder
            try self.require(second !== eye && second !== outline, "Eye Tab did not move")
            try self.require(!self.model.documents.panelsHidden, "Traversal hid panels")
            try self.focus(eye)
            try self.require(!self.tab(shift: true), "Eye Shift-Tab consumed")
            try self.require(self.window.firstResponder === outline, "Eye Shift-Tab did not return to the Layers list; got \(self.label(self.window.firstResponder))")
            return "FKA on (pinned): selected first Layers row; Tab → its eye → Tab → \(self.label(second)); eye Shift-Tab → Layers list; panels stay."
        }
        run("6a") {
            // FKA on: continue from the eye. FKA off: the eye is not a key view, so the list is the entry.
            let start: NSView = self.fka ? try self.eye() : try self.control("document.layers.outline", LayersOutlineView.self)
            try self.focus(start)
            var reached: [String] = [], count = 0
            for _ in 0..<60 {
                try self.require(!self.tab(), "Panel Tab consumed")
                count += 1
                try self.require(!self.model.documents.panelsHidden, "Traversal hid panels")
                if let button = self.window.firstResponder as? HistoryHeightButton {
                    let id = button.accessibilityIdentifier()
                    if !reached.contains(id) { reached.append(id) }
                }
                if reached.count == 3 { break }
            }
            try self.require(reached == ["decrease", "increase", "reset"].map { "document.history.height.\($0)" }, "Wrong History order: \(reached)")
            try self.require(!self.tab(shift: true), "History Shift-Tab consumed")
            let plus: NSButton = try self.control("document.history.height.increase")
            try self.require(self.window.firstResponder === plus, "Reset Shift-Tab did not return to +")
            return "\(self.fka ? "FKA on (pinned): eye" : "FKA off (pinned): Layers list (the eye is not a key view)") → − → + → ↺ in \(count) Tabs, every Tab unhandled; Shift-Tab → +."
        }
        run("6b") {
            let minus: NSButton = try self.control("document.history.height.decrease")
            let plus: NSButton = try self.control("document.history.height.increase")
            let reset: NSButton = try self.control("document.history.height.reset")
            try self.focus(minus)
            try self.require(!self.tab() && self.window.firstResponder === plus, "History − Tab did not reach +")
            try self.require(!self.tab() && self.window.firstResponder === reset, "History + Tab did not reach reset")
            try self.require(!self.tab(shift: true) && self.window.firstResponder === plus, "History reset Shift-Tab did not reach +")
            try self.require(!self.model.documents.panelsHidden, "History traversal hid panels")
            return "Actual hosted History − → + → ↺ in two Tabs; Shift-Tab → +. Entry focused directly; does not substitute for eye-to-History traversal in 6a."
        }
        run("7") {
            let plus: NSButton = try self.control("document.history.height.increase")
            let readout: NSTextField = try self.control("document.history.height.value")
            let h0 = UserDefaults.standard.double(forKey: keys[0])
            try self.focus(plus)
            try self.require(!self.press(49, " "), "History Space consumed by router")
            self.settle()
            try self.require(UserDefaults.standard.double(forKey: keys[0]) == h0 + Double(Theme.Height.row), "Space must advance exactly one row")
            try self.require(!self.model.documents.spaceHeld, "History Space panned")
            let afterSpace = readout.stringValue
            try self.require(!self.press(36, "\r"), "History Return consumed")
            self.settle()
            let delta = UserDefaults.standard.double(forKey: keys[0]) - h0 - Double(Theme.Height.row)
            try self.require(delta == 0 || delta == Double(Theme.Height.row), "Return advanced more than once")
            let reset: NSButton = try self.control("document.history.height.reset")
            try self.focus(reset); _ = try self.press(49, " "); self.settle()
            try self.require(UserDefaults.standard.double(forKey: keys[0]) == Double(DocumentInspector.historyDefault), "Reset did not restore default")
            return "H0=\(h0), Space=\(afterSpace), Return delta=\(delta), reset=\(readout.stringValue)."
        }
        na("8", "Either mode: the titlebar sidebar toggle and the SwiftUI toolbar exist only in the application window (NSToolbar + SwiftUI focus proxies). Their key-view eligibility comes from the real system setting inside AppKit/SwiftUI, which the in-process pin does not control, and toolbar traversal needs a key window. Inspector traversal is automated in 5–6.")
        run("9") {
            let eye = try self.eye(); try self.focus(eye)
            let doc = try self.doc(), count = doc.layers.count
            for (code, chars) in [(UInt16(51), "\u{7f}"), (117, "\u{f728}")] {
                try self.require(self.press(code, chars), "Button Delete not swallowed")
                try self.require(doc.layers.count == count && self.window.firstResponder === eye, "Delete changed layers/focus")
            }
            return "L=\(count) before each backward/forward Delete; count and responder unchanged."
        }
        run("10") {
            let eye = try self.eye(); try self.focus(eye)
            let cell: LayerRowCell = try self.control("document.layers.row.0.cell")
            let id = try XCTUnwrap(cell.layerID), doc = try self.doc()
            let initial = try XCTUnwrap(doc.node(id)).visible
            try self.require(self.press(49, " "), "Eye Space unhandled")
            try self.require(doc.node(id)?.visible == !initial, "First Space did not toggle")
            for _ in 0..<30 { try self.require(self.press(49, " ", repeatKey: true), "Repeat unhandled") }
            try self.require(doc.node(id)?.visible == !initial, "Repeats toggled again")
            _ = try self.press(49, " ", up: true)
            try self.require(doc.node(id)?.visible == !initial, "Release toggled")
            try self.require(self.press(49, " "), "Second press unhandled")
            try self.require(doc.node(id)?.visible == initial && !self.model.documents.spaceHeld, "Second press/no-pan failed")
            _ = try self.press(49, " ", up: true)
            return "One press + 30 timestamped repeat events over one simulated second + release + second press; exactly one toggle per press, no pan."
        }
        na("11", "Either mode: the titlebar sidebar toggle uses NSApp.sendAction with no target (key-window responder chain), and SwiftUI.KeyViewProxy focus follows the real system setting and needs a key window; the in-process pin controls neither. No substitute proxy is counted as acceptance.")
        run("12") {
            let engine = try Engine.open(appSupportDir: scratch.appendingPathComponent("pixel-engine").path)
            try self.model.documents.install(EngineDocumentBackend(session: try engine.newDocument(width: 96, height: 64, depth: .u8, profile: nil)))
            try self.doc().addLayer(.pixel); try self.doc().addLayer(.pixel)
            self.settle()
            let canvas = try self.canvas(), doc = try self.doc()
            doc.tool = .marquee
            let a = CGPoint(x: canvas.bounds.midX - 30, y: canvas.bounds.midY - 30)
            let b = CGPoint(x: a.x + 60, y: a.y + 60)
            canvas.mouseDown(with: try self.mouse(.leftMouseDown, at: a, view: canvas))
            canvas.mouseDragged(with: try self.mouse(.leftMouseDragged, at: b, view: canvas))
            canvas.mouseUp(with: try self.mouse(.leftMouseUp, at: b, view: canvas))
            let selection = try XCTUnwrap(doc.marquee), count = doc.layers.count
            try self.focus(self.eye())
            try self.require(self.press(51, "\u{7f}"), "Button Delete unhandled")
            try self.require(doc.marquee == selection && doc.layers.count == count, "Button Delete changed selection/layers")
            try self.focus(canvas)
            let historyCount = doc.history.count
            try self.require(self.press(51, "\u{7f}"), "Canvas Delete unhandled")
            try self.require(doc.history.count == historyCount + 1 && doc.history.last?.label == "Clear", "Canvas Delete did not create exactly one Clear operation")
            try self.require(doc.layers.count == count, "Pixel Clear deleted a layer")
            try self.require(doc.marquee == selection, "Pixel Clear unexpectedly dismissed marquee")
            return "Real engine, injected marquee drag; L=\(count); panel Delete preserves selection; canvas Delete clears selected pixels (one Clear history operation), retaining marquee as on main. Panel entry set directly (canvas Tab hides panels)."
        }
        run("13") {
            let eye = try self.eye(); try self.focus(eye)
            for (code, letter, tool) in [(UInt16(11), "b", DocumentTool.brush), (9, "v", .move)] {
                try self.require(self.press(code, letter), "Tool key unhandled")
                try self.require(self.doc().tool == tool, "Wrong tool")
                try self.require(self.window.firstResponder === eye && !self.model.documents.panelsHidden, "Tool moved focus/panels")
            }
            return "B → Brush, V → Move; eye retains responder."
        }
        run("14") {
            let outline: LayersOutlineView = try self.control("document.layers.outline")
            outline.selectRowIndexes(IndexSet(integer: 0), byExtendingSelection: false)
            try self.focus(outline)
            let doc = try self.doc(), count = doc.layers.count, selected = doc.selection
            try self.require(!self.press(51, "\u{7f}"), "List Delete consumed by router")
            try self.require(doc.layers.count == count - 1, "List Delete not exactly one layer")
            doc.undo(); doc.selection = selected; self.settle()
            try self.require(doc.layers.count == count, "Undo did not restore layer")
            return "L=\(count) → \(count - 1) → \(count); Undo command action called directly."
        }
        run("15") {
            let canvas = try self.canvas(), doc = try self.doc()
            doc.tool = .move; doc.setMarquee(nil); try self.focus(canvas)
            let count = doc.layers.count
            try self.require(self.press(51, "\u{7f}"), "Canvas Delete unhandled")
            try self.require(doc.layers.count == count - 1, "Canvas Delete not exactly one layer")
            doc.undo(); self.settle()
            try self.require(doc.layers.count == count, "Undo did not restore")
            let before = doc.viewState?.center
            _ = try self.press(49, " ")
            let a = CGPoint(x: canvas.bounds.midX, y: canvas.bounds.midY), b = CGPoint(x: canvas.bounds.midX + 30, y: canvas.bounds.midY + 20)
            canvas.mouseDown(with: try self.mouse(.leftMouseDown, at: a, view: canvas))
            canvas.mouseDragged(with: try self.mouse(.leftMouseDragged, at: b, view: canvas))
            canvas.mouseUp(with: try self.mouse(.leftMouseUp, at: b, view: canvas))
            try self.require(doc.viewState?.center != before && self.model.documents.spaceHeld, "Space drag did not pan")
            _ = try self.press(49, " ", up: true)
            try self.require(!self.model.documents.spaceHeld, "Release did not end pan")
            return "L=\(count); Delete/Undo restored; injected Space-drag changed viewport center; release ended pan."
        }
        run("16a") {
            let cell: LayerRowCell = try self.control("document.layers.row.0.cell")
            let field: NSTextField = try self.control("document.layers.row.0.name")
            let original = field.stringValue, doc = try self.doc(), count = doc.layers.count
            cell.beginRename()
            guard let editor = field.currentEditor() else {
                throw NotApplicable(reason: "Layer rename requires native field editor; background window did not supply one.")
            }
            editor.selectedRange = NSRange(location: max(0, editor.string.count - 1), length: 1)
            try self.require(!self.press(51, "\u{7f}"), "Rename Delete consumed")
            try self.require(editor.string.count == original.count - 1, "Rename Delete did not remove one character")
            try self.require(!self.press(49, " "), "Rename Space consumed")
            try self.require(editor.string.contains(" "), "Rename Space did not type")
            try self.require(doc.layers.count == count && !self.model.documents.spaceHeld && !self.model.documents.panelsHidden, "Rename changed document ownership")
            // Escape is delivered before Tab, since Tab commits and ends the rename session.
            _ = try self.press(53, "\u{1b}")
            self.settle()
            try self.require(field.stringValue == original, "Escape did not cancel rename")
            cell.beginRename()
            try self.require(!self.tab(), "Rename Tab consumed by router")
            try self.require(!self.model.documents.panelsHidden && !self.model.documents.spaceHeld && doc.layers.count == count, "Rename Tab changed document ownership")
            try self.focus(self.canvas())
            return "Actual layer name field editor: Delete one character, Space types, Escape cancels; reopened rename Tab stays native; L=\(count)."
        }
        run("16b") {
            let doc = try self.doc(), count = doc.layers.count
            doc.addAdjustment(.colorLookup)
            self.model.documents.inspectorTab = .properties
            self.settle()
            defer {
                _ = self.window.makeFirstResponder(try? self.canvas())
                while doc.layers.count > count { doc.undo() }
                self.model.documents.inspectorTab = .stack; self.settle()
            }
            let loadID = "document.properties.colorLookup.load", ditherID = "document.properties.colorLookup.dither"
            let load: NSButton = try self.control(loadID), dither: NSButton = try self.control(ditherID)
            try self.require(load.isEnabled && dither.isEnabled, "Load 3D LUT / Dither disabled")
            try self.require(load.canBecomeKeyView == self.fka && dither.canBecomeKeyView == self.fka,
                             "Pinned FKA=\(self.fka) does not control Load 3D LUT / Dither key-view eligibility")
            // SwiftUI's TextField("Name") is an AppKit text field without a view-level identifier.
            let fields = self.views(self.host).compactMap { $0 as? NSTextField }.filter {
                $0.isEditable && $0.placeholderString == "Name" && !$0.isHiddenOrHasHiddenAncestor
            }
            try self.require(fields.count == 1, "Expected one Properties Name field, found \(fields.count)")
            try self.focus(fields[0])
            try self.require(self.window.firstResponder is NSText, "Properties Name did not begin editing")
            guard self.fka else {
                try self.require(!self.tab(), "Name Tab consumed")
                let landed = self.window.firstResponder
                try self.require(landed !== load && landed !== dither && !(landed is NSText), "FKA off: Name Tab must leave the field and skip Load 3D LUT and Dither; got \(self.label(landed))")
                try self.require(!self.model.documents.panelsHidden, "Name Tab hid panels")
                throw NotApplicable(reason: "FKA off (pinned): Load 3D LUT and Dither are not key views (canBecomeKeyView=false), so this traversal does not exist in this mode. Asserted instead: Name Tab is unhandled, skips both, lands on \(self.label(landed)), panels stay.")
            }
            var reached: [String] = [], tabs = 0
            for _ in 0..<12 {
                try self.require(!self.tab(), "Properties Tab consumed")
                tabs += 1
                try self.require(!self.model.documents.panelsHidden, "Properties traversal hid panels")
                let id = (self.window.firstResponder as? NSView)?.accessibilityIdentifier() ?? ""
                if [loadID, ditherID].contains(id), !reached.contains(id) { reached.append(id) }
                if reached.count == 2 { break }
            }
            try self.require(reached == [loadID, ditherID], "Wrong Properties order: \(reached)")
            return "FKA on (pinned): Properties Name field → Load 3D LUT → Dither in \(tabs) Tabs, every Tab unhandled; panels stay. Responder contract; no visible ring claimed."
        }
        run("17a") {
            let doc = try self.doc()
            doc.addAdjustment(.colorLookup)
            let id = try XCTUnwrap(doc.primary?.id)
            let initial = try XCTUnwrap(doc.adjustment(of: id))
            let head = doc.info.historyHead, historyCount = doc.history.count
            self.model.documents.inspectorTab = .properties
            self.settle()
            defer { self.model.documents.inspectorTab = .stack; self.settle() }
            let box: DocumentDitherNativeCheckbox = try self.control("document.properties.colorLookup.dither")
            try self.focus(box)
            let initialState = box.state
            try self.require(!self.press(49, " "), "Dither Space must reach its native control")
            self.settle()
            try self.require(box.state != initialState && doc.adjustment(of: id) != initial, "Dither did not toggle")
            try self.require(doc.history.count == historyCount + 1 && doc.info.historyHead != head, "Dither must add exactly one history entry")
            let toggled = doc.adjustment(of: id), toggledHead = doc.info.historyHead
            for _ in 0..<30 { _ = try self.press(49, " ", repeatKey: true) }
            _ = try self.press(49, " ", up: true)
            try self.require(doc.adjustment(of: id) == toggled && doc.info.historyHead == toggledHead && doc.history.count == historyCount + 1, "Repeat/release changed Dither history")
            try self.require(!self.model.documents.spaceHeld && self.window.firstResponder === box, "Dither lost ownership or started pan")
            doc.undo(); self.settle()
            try self.require(doc.adjustment(of: id) == initial && doc.info.historyHead == head && box.state == initialState, "Undo did not restore Dither and history head")
            return "Direct focus on real Dither; Space toggles once and adds one history entry; 30 repeats/release add none; no pan; Undo restores value, checkbox and history head."
        }
        run("17b") {
            let doc = try self.doc(), count = doc.layers.count
            doc.addAdjustment(.colorLookup)
            self.model.documents.inspectorTab = .properties
            self.settle()
            defer {
                _ = self.window.makeFirstResponder(try? self.canvas())
                while doc.layers.count > count { doc.undo() }
                self.model.documents.inspectorTab = .stack; self.settle()
            }
            let box: DocumentDitherNativeCheckbox = try self.control("document.properties.colorLookup.dither")
            try self.require(box.canBecomeKeyView == self.fka, "Pinned FKA=\(self.fka) does not control Dither key-view eligibility")
            guard self.fka else {
                throw NotApplicable(reason: "FKA off (pinned): Dither is not a key view (canBecomeKeyView=false); there is no native traversal to or from it in this mode. Activation, history and Undo are automated in 17a.")
            }
            try self.focus(box)
            try self.require(!self.tab(), "Dither Tab consumed")
            let next = self.window.firstResponder
            try self.require(next !== box, "Dither Tab did not move")
            try self.require(!self.model.documents.panelsHidden, "Dither Tab hid panels")
            let proxy = !(next is NSControl) && !(next is KeyOwningControl)
            try self.require(!self.tab(shift: true), "Shift-Tab back consumed")
            let returned = self.window.firstResponder === box
            let observed = "Dither Tab is unhandled and moves to \(self.label(next)), panels stay; Shift-Tab \(returned ? "returns to Dither" : "lands on \(self.label(self.window.firstResponder))")"
            if !returned {
                // A hop that starts on a SwiftUI focus proxy is SwiftUI's own focus movement, which needs
                // a key window; only native-to-native hops are judged in the background host.
                try self.require(proxy, "Shift-Tab from \(self.label(next)) did not return to Dither")
                throw NotApplicable(reason: "FKA on (pinned): \(observed). The return hop starts on a SwiftUI focus proxy, whose focus movement needs a key window; not judged in the background host.")
            }
            guard KeyboardAccessHarness.systemFullKeyboardAccess else {
                throw NotApplicable(reason: "FKA on (pinned) on a machine whose real setting is off: SwiftUI focus proxies (the Color header) take key-view eligibility from the real setting inside SwiftUI, which no in-process pin controls, so Dither → Color header cannot be reproduced here. Asserted on this machine: \(observed).")
            }
            return "FKA on (pinned, real setting on): \(observed)."
        }
        for path in ["18", "19a"] {
            run(path) {
                let eye = try self.eye(); try self.focus(eye)
                if path == "18" { self.model.documents.togglePanels() }
                else { _ = try self.press(3, "f"); _ = try self.press(3, "f") }
                self.settle()
                try self.require(self.model.documents.panelsHidden, "Panels not hidden")
                try self.require(self.window.firstResponder !== eye, "Removed eye retains focus")
                for _ in 0..<60 {
                    let previous = self.window.firstResponder
                    let handled = try self.tab(); self.settle()
                    if !self.model.documents.panelsHidden { break }
                    try self.require(!handled && self.window.firstResponder !== previous, "Tab made no progress")
                }
                try self.require(!self.model.documents.panelsHidden, "Tabs never restored panels")
                if let view = self.window.firstResponder as? NSView {
                    try self.require(view.window === self.window && !view.isHiddenOrHasHiddenAncestor, "Invisible/detached responder")
                }
                let doc = try self.doc(), count = doc.layers.count
                let isCanvas = self.window.firstResponder is DocumentViewportView
                _ = try self.press(51, "\u{7f}")
                try self.require(doc.layers.count == count || (isCanvas && doc.layers.count == count - 1), "Non-canvas Delete removed layer")
                if doc.layers.count != count { doc.undo() }
                if path == "19a" { _ = try self.press(3, "f"); try self.require(self.model.documents.screenMode == 0, "F failed to return standard") }
                self.settle()
                return "\(path == "18" ? "Hide Panels command action" : "F twice, then F to standard (workspace has no mainWindow)"); bounded Tab restoration and post-restore Delete checked, L=\(count)."
            }
        }
        na("19b", "macOS full-screen window/Space transition requires a real application window and can take focus; 19a tests the F routing and panel state only.")
        run("20") {
            let canvas = try self.canvas(); try self.focus(canvas)
            _ = try self.tab(); self.settle()
            try self.require(self.model.documents.panelsHidden, "Canvas Tab did not hide panels")
            let expanded = try self.canvas()
            let point = CGPoint(x: expanded.bounds.maxX - 20, y: expanded.bounds.midY)
            expanded.mouseDown(with: try self.mouse(.leftMouseDown, at: point, view: expanded))
            expanded.mouseUp(with: try self.mouse(.leftMouseUp, at: point, view: expanded))
            try self.require(self.window.firstResponder === expanded, "Former inspector area did not focus canvas")
            try self.require(self.tab(), "Restore Tab unhandled")
            try self.require(!self.model.documents.panelsHidden, "Panels not restored")
            self.settle()
            return "Direct click in expanded canvas at former inspector coordinates; Tab restores."
        }
        run("21a") {
            self.model.requestLibraryViewMode(.grid); self.settle()
            try self.require(self.model.viewMode == .grid, "Library action did not leave document")
            let grid = try XCTUnwrap(self.views(self.host).compactMap { $0 as? ThumbnailCollectionView }.first)
            self.model.setSelectionFromUI(IndexSet(integer: 0), clicked: 0)
            try self.focus(grid)
            try self.require(self.press(124, "\u{f703}"), "Grid Right arrow unhandled")
            try self.require(self.model.focus == 1, "Grid Right arrow did not select the second image")
            try self.require(self.model.viewMode != .document, "Grid navigation reentered document")
            try self.require(self.press(14, "e", command: true), "Reentry Command-E not routed")
            try self.require(self.model.layeredCopyRequest != nil, "Reentry sheet request missing")
            self.model.createRequestedLayeredCopy(); self.settle()
            while try self.doc().layers.count < 3 { try self.doc().addLayer(.pixel) }
            self.settle()
            try self.require(self.window.firstResponder === self.canvas(), "Reentry retained grid focus")
            try self.require(self.tab() && self.model.documents.panelsHidden, "Reentry Tab did not hide")
            _ = try self.tab(); self.settle()
            return "Library action → grid Right arrow (document=false in trace) → Command-E and sheet confirmation action; automatic focus and Tab restored."
        }
        na("21b", "Toolbar Library click and visible sheet UI require application command scene; Library action, ⌘E routing and confirmed reentry automated in 21a.")
        run("22a", success: "TEARDOWN") {
            LayoutProbeHarness.dispose(self.window)
            try self.require(!self.window.isVisible && !self.window.isKeyWindow, "Hosted window did not close cleanly")
            return "Hosted window disposed; trace and step-to-sequence table archived."
        }
        na("22b", "Application Quit/save prompts require the running application lifecycle; terminating the XCTest host is not an application Quit check.")
    }

    func testResultArtifactsRequireExplicitOptIn() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        try writeResults(fka: false, directory: directory, environment: [:])
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: directory.path), [])
    }

    private static func normalizeAddresses(_ trace: String) -> String {
        trace.replacingOccurrences(of: "0x[0-9a-fA-F]+", with: "<address>", options: .regularExpression)
    }

    func testTraceAddressNormalizationIsDeterministic() {
        let first = "ObjectIdentifier(0x00000001) ObjectIdentifier(0xABCDEF)"
        let second = "ObjectIdentifier(0x98765432) ObjectIdentifier(0x123456)"
        XCTAssertEqual(Self.normalizeAddresses(first), Self.normalizeAddresses(second))
        XCTAssertEqual(Self.normalizeAddresses(first), "ObjectIdentifier(<address>) ObjectIdentifier(<address>)")
    }

    private func writeResults(fka: Bool,
                              directory: URL = ShellHarness.repoRoot.appendingPathComponent("tools/orchestrate/wp/B5-49"),
                              environment: [String: String] = ProcessInfo.processInfo.environment) throws {
        guard environment["TESSERA_REGENERATE_KEYBOARD_RESULTS"] == "1" else { return }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let defaults = Process(), pipe = Pipe()
        defaults.executableURL = URL(fileURLWithPath: "/usr/bin/defaults")
        defaults.arguments = ["read", "-g", "AppleKeyboardUIMode"]
        defaults.standardOutput = pipe; defaults.standardError = pipe
        try defaults.run(); defaults.waitUntilExit()
        let mode = String(decoding: pipe.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
        let modeSummary = defaults.terminationStatus == 0 ? mode.trimmingCharacters(in: .whitespacesAndNewlines) : "unset (defaults exit \(defaults.terminationStatus))"
        func clean(_ value: String) -> String { value.replacingOccurrences(of: "|", with: "\\|").replacingOccurrences(of: "\n", with: " ") }
        let header = """
        # B5-49 automated keyboard checklist

        Generated by DocumentKeyboardChecklistTests, once per pinned Full Keyboard Access (FKA) mode: `testCombinedKeyboardChecklistFKAOn` and `testCombinedKeyboardChecklistFKAOff`. Both run on every machine, whatever its own setting.

        The pin is in-process and test-only. The app's `KeyboardAccessPolicy`, `NSApplication.isFullKeyboardAccessEnabled` and `NSButton.canBecomeKeyView` follow the pinned mode; no user default or system setting is written, and none decides a result. SwiftUI focus proxies and AppKit control classes other than buttons still take their key-view eligibility from the machine's real setting; steps that need them are N/A with the reason. Generating machine, for information only: AppleKeyboardUIMode \(modeSummary); real AppKit FKA \(KeyboardAccessHarness.systemFullKeyboardAccess).

        Isolated LayoutProbeHarness preferences; StubLibrary scanning two generated JPEGs, StubDocumentEngine with ≥3 layers; step 12 onward uses a real 96×64 engine document for pixel Clear and ownership checks. The window has a fixed 1100×848 pt content size that is not constrained to the screen; no assertion depends on screen size or backing scale. Windows ordered back, activation prohibited.

        TEARDOWN is cleanup only and is excluded from PASS counts. Memory addresses are normalized to `<address>`. Regenerate only with `TESSERA_REGENERATE_KEYBOARD_RESULTS=1`. PASS covers the action/responder contract described in the note, not a visible ring or physical key delivery. Split rows explicitly retain native application work. FAIL includes missing required hosted preconditions (never silently skipped). Traces are `focus-hosted-fka-on.jsonl` and `focus-hosted-fka-off.jsonl`: actual InspectorFocusTrace snapshots, direct KeyRouter handled values, all key types including Delete/tool letters/up/repeats, and layer count before every event. They do not claim production owned-key-window eligibility. Non-key steps have no sequence.

        """
        let tag = fka ? "ON" : "OFF"
        func count(_ result: String) -> Int { rows.filter { $0.result == result }.count }
        let table = rows.map { row in "| \([row.step, row.result, row.sequence.isEmpty ? "—" : row.sequence, row.nativeType.isEmpty ? "not observed" : row.nativeType, row.handled.isEmpty ? "not delivered" : row.handled, row.note].map(clean).joined(separator: " | ")) |" }.joined(separator: "\n")
        let section = Self.sectionStart(tag) + "\n## FKA \(tag) (pinned)\n\nPASS \(count("PASS")), N/A \(count("N/A")), FAIL \(count("FAIL")), TEARDOWN \(count("TEARDOWN")); \(events.count) key events in `focus-hosted-fka-\(tag.lowercased()).jsonl`.\n\n| Step | Result | Sequence | before.nativeType | handled | Note |\n| --- | --- | --- | --- | --- | --- |\n" + table + "\n" + Self.sectionEnd(tag) + "\n"
        // Each variant replaces only its own table, so the file is the same whichever runs first.
        let existing = (try? String(contentsOf: directory.appendingPathComponent("RESULTS.md"), encoding: .utf8)) ?? ""
        let sections = ["ON", "OFF"].compactMap { $0 == tag ? section : Self.section($0, in: existing) }
        for filename in ["RESULTS.md", "GUI-RESULTS.md"] {
            try (header + "\n" + sections.joined(separator: "\n")).write(to: directory.appendingPathComponent(filename), atomically: true, encoding: .utf8)
        }
        let encoder = JSONEncoder(); encoder.outputFormatting = [.sortedKeys]
        var data = Data()
        for event in events { data.append(try encoder.encode(event)); data.append(0x0a) }
        let normalized = Self.normalizeAddresses(String(decoding: data, as: UTF8.self))
        try Data(normalized.utf8).write(to: directory.appendingPathComponent("focus-hosted-fka-\(tag.lowercased()).jsonl"), options: .atomic)
    }

    private static func sectionStart(_ tag: String) -> String { "<!-- FKA-\(tag):BEGIN -->" }
    private static func sectionEnd(_ tag: String) -> String { "<!-- FKA-\(tag):END -->" }
    private static func section(_ tag: String, in text: String) -> String? {
        guard let start = text.range(of: sectionStart(tag)), let end = text.range(of: sectionEnd(tag)),
              start.lowerBound < end.lowerBound else { return nil }
        return String(text[start.lowerBound..<end.upperBound]) + "\n"
    }

    func testEachVariantReplacesOnlyItsOwnResultTable() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let optIn = ["TESSERA_REGENERATE_KEYBOARD_RESULTS": "1"]
        func row(_ note: String) -> Row { Row(step: "1a", result: "PASS", sequence: "", nativeType: "", handled: "", note: note) }
        var outputs: [String] = []
        for order in [[true, false], [false, true]] {
            try? FileManager.default.removeItem(at: directory)
            for fka in order + order {
                rows = [row(fka ? "on-note" : "off-note")]; events = []
                try writeResults(fka: fka, directory: directory, environment: optIn)
            }
            outputs.append(try String(contentsOf: directory.appendingPathComponent("RESULTS.md"), encoding: .utf8))
            XCTAssertEqual(outputs.last, try String(contentsOf: directory.appendingPathComponent("GUI-RESULTS.md"), encoding: .utf8))
            XCTAssertEqual(Set(try FileManager.default.contentsOfDirectory(atPath: directory.path)),
                           ["RESULTS.md", "GUI-RESULTS.md", "focus-hosted-fka-on.jsonl", "focus-hosted-fka-off.jsonl"])
        }
        XCTAssertEqual(outputs[0], outputs[1], "The result file must not depend on which variant ran first")
        let text = outputs[0]
        let on = try XCTUnwrap(text.range(of: "## FKA ON (pinned)")), off = try XCTUnwrap(text.range(of: "## FKA OFF (pinned)"))
        XCTAssertTrue(on.lowerBound < off.lowerBound)
        XCTAssertEqual(text.components(separatedBy: "on-note").count, 2)
        XCTAssertEqual(text.components(separatedBy: "off-note").count, 2)
    }
}
