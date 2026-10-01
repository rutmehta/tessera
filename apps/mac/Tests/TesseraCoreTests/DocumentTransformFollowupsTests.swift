import AppKit
import XCTest
import TesseraFFI
@testable import Tessera
@testable import TesseraCore

/// WP B5-12b: transform defects from the on-screen verification of B5-12, through the real app paths
/// (an engine document, a `DocumentViewportView` in an off-screen window, synthesized mouse events,
/// the options bar's field with real field-editor commands, `DocumentTools.handleKey`):
///  2. a refused perspective drag says why (status bar and options bar);
///  3. Expansion above 64 px is refused by the engine and the field keeps the rejected value;
///  4. Esc / Return while an options-bar field is editing;
///  5. a pending preview leaves the Layers / Properties model on the original layer;
///  6. no stale session instructions after Apply, Cancel or a tool change.
/// (Item 1, the native_stack workaround, is `crates/tessera-ffi/tests/document_transform_followups.rs`;
/// item 7, the options bar at 1440 pt, is checked by `--transform-selftest`.)
@MainActor
final class DocumentTransformFollowupsTests: XCTestCase {
    private var model: AppModel!
    private var doc: DocumentController!
    private var window: NSWindow!
    private var viewport: DocumentViewportView!
    private var layer: DocLayerID = 0
    private var t: DocumentTransforms { .shared }
    private var tools: DocumentTools { .shared }

    override func setUp() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("transform-fixes-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        let engine = try Engine.open(appSupportDir: dir.appendingPathComponent("support").path)
        let s = try engine.newDocument(width: 600, height: 400, depth: .u8, profile: nil)
        _ = try s.addLayer(kind: .fill(json: #"{"kind":"solid","color":[0.8,0.4,0.2]}"#), name: "Fill", parent: nil, index: nil)
        model = AppModel()
        try model.documents.install(EngineDocumentBackend(session: s))
        doc = try XCTUnwrap(model.documents.current)
        window = LayoutProbeHarness.window(contentRect: NSRect(x: 0, y: 0, width: 900, height: 600), styleMask: [.titled, .resizable],
                          backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        viewport = DocumentViewportView(frame: window.contentView!.bounds)
        viewport.autoresizingMask = [.width, .height]
        window.contentView!.addSubview(viewport)
        viewport.workspace = model.documents
        viewport.attach(doc)
        viewport.zoomToFit()
        tools.attach(model.documents)
        t.attach(model.documents)
        layer = try XCTUnwrap(doc.layers.first(where: { $0.kind == .fill })?.id)
        doc.select(layer)
        doc.tool = .move
    }

    override func tearDown() async throws {
        if t.session != nil { t.cancel() }
        await t.idle()
        await settle()
        viewport?.attach(nil)
        window?.close()
    }

    // MARK: Helpers

    private func settle(_ extra: Double = 0.15) async {
        await t.idle()
        await tools.idle()
        if let e = try? doc.backend.info().epoch {
            let end = Date().addingTimeInterval(20)
            while (doc.lastFrame?.epoch ?? 0) < e, Date() < end { try? await Task.sleep(for: .milliseconds(10)) }
        }
        // Listener callbacks (rows, history) hop to the main actor after the frame.
        try? await Task.sleep(for: .milliseconds(Int(extra * 1000)))
    }

    private func begin(_ tag: AdvancedTransformTag) async throws {
        t.begin(tag)
        let end = Date().addingTimeInterval(20)
        while t.session == nil, Date() < end { try? await Task.sleep(for: .milliseconds(10)) }
        XCTAssertNotNil(t.session, "\(tag.title) did not start: \(t.status ?? "-")")
        await settle()
    }

    private func finish(_ body: () -> Void) async {
        var done = false
        t.onFinished = { _ in done = true }
        body()
        let end = Date().addingTimeInterval(30)
        while !done, Date() < end { try? await Task.sleep(for: .milliseconds(10)) }
        t.onFinished = nil
        await settle()
    }

    private func mouse(_ type: NSEvent.EventType, _ p: CGPoint) -> NSEvent {
        NSEvent.mouseEvent(with: type, location: viewport.convert(p, to: nil), modifierFlags: [],
                           timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber, context: nil,
                           eventNumber: 0, clickCount: 1, pressure: 1)!
    }

    /// A drag between two child points (viewport events).
    private func drag(_ a: CGPoint, _ b: CGPoint, steps: Int = 6) async {
        let (va, vb) = (t.viewPoint(a, in: viewport), t.viewPoint(b, in: viewport))
        viewport.mouseDown(with: mouse(.leftMouseDown, va))
        for i in 1...steps {
            let s = CGFloat(i) / CGFloat(steps)
            viewport.mouseDragged(with: mouse(.leftMouseDragged, CGPoint(x: va.x + (vb.x - va.x) * s, y: va.y + (vb.y - va.y) * s)))
            try? await Task.sleep(for: .milliseconds(20))
        }
        viewport.mouseUp(with: mouse(.leftMouseUp, vb))
        await settle()
    }

    private func key(_ code: UInt16, _ chars: String) -> NSEvent {
        NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [], timestamp: 0, windowNumber: window.windowNumber,
                         context: nil, characters: chars, charactersIgnoringModifiers: chars, isARepeat: false, keyCode: code)!
    }

    /// The options-bar field in this window, wired like `TransformField`.
    private func field(value: Double, commit: @escaping (Double) -> Void) -> TransformNumberField {
        let f = TransformNumberField(frame: NSRect(x: 10, y: 10, width: 60, height: 22))
        f.onCommit = commit
        f.onReturn = { DocumentTransforms.shared.applyAfterFieldCommit() }
        f.onEscape = { DocumentTransforms.shared.cancel() }
        f.focusAfterEditing = { [weak viewport] in viewport }
        f.value = value
        window.contentView!.addSubview(f)
        return f
    }

    /// Focuses `f`, replaces its text as typed and sends `key` to the field editor.
    private func type(_ text: String?, then keyCode: UInt16, _ chars: String, into f: TransformNumberField) throws {
        if f.currentEditor() == nil { XCTAssertTrue(window.makeFirstResponder(f), "field takes the keyboard") }
        let editor = try XCTUnwrap(f.currentEditor() as? NSTextView)
        if let text { editor.string = text }
        editor.keyDown(with: key(keyCode, chars))
    }

    private var status: String { model.statusMessage ?? "" }

    // MARK: 2 — refused perspective drag

    func testRefusedPerspectiveDragSaysPlanesMustStayConvex() async throws {
        try await begin(.perspective)
        t.perspectiveLayout = false
        guard case .perspective(let p)? = t.session?.op else { return XCTFail("no perspective session") }
        let corner = p.destination[1][0]            // bottom left
        let opposite = p.destination[0][1]          // top right
        model.statusMessage = nil
        await drag(corner, CGPoint(x: opposite.x + 200, y: opposite.y - 150))
        guard case .perspective(let after)? = t.session?.op else { return XCTFail("session ended") }
        XCTAssertTrue(after.isValid, "the drag stops at the last convex shape")
        XCTAssertTrue(status.contains("convex"), "status bar: \(status)")
        XCTAssertTrue((t.refusal ?? "").contains("convex"), "options bar: \(t.refusal ?? "-")")
        // A valid drag clears the options-bar notice.
        await drag(after.destination[0][0], CGPoint(x: after.destination[0][0].x + 20, y: after.destination[0][0].y + 10))
        XCTAssertNil(t.refusal)
    }

    // MARK: 3 — Expansion out of range

    func testExpansionAboveTheEngineLimitIsRefusedAndStaysOnShowUntilCorrected() async throws {
        try await begin(.puppet)
        let accepted = t.puppetExpansion
        guard case .puppet(let before)? = t.session?.op else { return XCTFail("no puppet session") }
        let f = field(value: t.puppetExpansionShown) { DocumentTransforms.shared.setPuppetExpansion($0) }
        try type("80", then: 48, "\t", into: f)          // Tab ends editing and commits
        await settle()
        XCTAssertEqual(t.expansionRejected, 80, "the engine refused 80 px")
        XCTAssertEqual(t.puppetExpansion, accepted, "no silent clamp or revert of the accepted value")
        XCTAssertEqual(t.puppetExpansionShown, 80, "the field keeps the rejected value")
        XCTAssertTrue(status.contains("64"), "the engine's reason in the status bar: \(status)")
        XCTAssertTrue((t.refusal ?? "").contains("64"), "and the options bar: \(t.refusal ?? "-")")
        if case .puppet(let now)? = t.session?.op { XCTAssertEqual(now.restVertices, before.restVertices, "mesh unchanged") }
        // Return with the rejected value does not apply.
        f.value = t.puppetExpansionShown
        f.rejected = true
        XCTAssertEqual(f.stringValue, "80")
        try type(nil, then: 36, "\r", into: f)
        await settle()
        XCTAssertNotNil(t.session, "a refused value never applies")
        // Corrected: accepted, remeshed, the notice goes.
        try type("10", then: 48, "\t", into: f)
        await settle()
        XCTAssertNil(t.expansionRejected)
        XCTAssertEqual(t.puppetExpansion, 10)
        XCTAssertNil(t.refusal)
        XCTAssertEqual(t.puppetExpansionShown, 10)
    }

    // MARK: 4 — Esc / Return in a field

    func testEscInAnEditedFieldEndsEditingThenASecondEscCancels() async throws {
        try await begin(.warp)
        t.warpBend = 20
        t.applyWarpPreset("Arc")
        await settle()
        let f = field(value: t.warpBend) { DocumentTransforms.shared.setWarpBend($0) }
        try type("35", then: 53, "\u{1b}", into: f)
        await settle()
        XCTAssertNil(f.currentEditor(), "the first Esc ends field editing")
        XCTAssertEqual(f.stringValue, "20", "and reverts the typed value")
        XCTAssertNotNil(t.session, "the session stays after the first Esc")
        XCTAssertTrue(window.firstResponder === viewport, "the canvas has the keyboard")
        // Second Esc: through the canvas key path.
        XCTAssertTrue(tools.handleKey(key(53, "\u{1b}")))
        await settle()
        XCTAssertNil(t.session, "the second Esc cancels the session")
    }

    func testEscWithTheFieldUnchangedCancelsTheSession() async throws {
        try await begin(.warp)
        let nodes = doc.history.count
        let f = field(value: t.warpBend) { DocumentTransforms.shared.setWarpBend($0) }
        try type(nil, then: 53, "\u{1b}", into: f)
        await settle()
        XCTAssertNil(t.session, "Esc with nothing typed cancels at once")
        XCTAssertEqual(doc.history.count, nodes)
    }

    func testReturnInAFieldCommitsItThenApplies() async throws {
        // A smart object first (Apply needs no consent afterwards).
        try await begin(.warp)
        t.warpBend = 20
        t.applyWarpPreset("Arc")
        await settle()
        await finish { t.apply(); t.confirmConversion() }
        XCTAssertEqual(doc.node(layer)?.kind, .smartObject)
        try await begin(.warp)
        t.warpBend = 10
        t.applyWarpPreset("Flag")
        await settle()
        let nodes = doc.history.count
        let f = field(value: t.warpBend) { DocumentTransforms.shared.setWarpBend($0) }
        var finished = false
        t.onFinished = { _ in finished = true }
        try type("45", then: 36, "\r", into: f)
        let end = Date().addingTimeInterval(30)
        while !finished, Date() < end { try? await Task.sleep(for: .milliseconds(10)) }
        t.onFinished = nil
        await settle()
        XCTAssertEqual(t.warpBend, 45, "Return committed the field")
        XCTAssertNil(t.session, "then applied")
        XCTAssertEqual(doc.history.count, nodes + 1)
        XCTAssertEqual(doc.history.last?.label, "Warp")
        let stage = try XCTUnwrap((doc.backend as? any DocumentTransformsBackend)?.transformStages(layer: layer).last)
        let flag45 = try TransformBridge.preset(width: 600, height: 400, name: "Flag", bend: 0.45)
        guard case .warp(let m)? = TransformOperationModel.parse(stage.json, canvasWidth: 600, canvasHeight: 400)?.0 else {
            return XCTFail("stage")
        }
        XCTAssertEqual(m, flag45, "the applied stage carries the value typed before Return")
    }

    // MARK: 5 — pending preview keeps the original layer in the panels

    func testPendingPreviewLeavesLayersAndPropertiesOnTheOriginalLayer() async throws {
        let revision = doc.revision
        try await begin(.warp)
        t.warpBend = 40
        t.applyWarpPreset("Arc")
        await settle(0.4)
        XCTAssertEqual(t.previewLabel, "Warp (preview)")
        XCTAssertEqual(doc.node(layer)?.kind, .fill, "Properties / Layers: still the fill layer")
        XCTAssertEqual(doc.revision, revision, "the preview did not reload the rows")
        // The consent alert, then its Cancel: still the original.
        t.apply()
        XCTAssertTrue(t.consentPending)
        t.consentPending = false
        await settle(0.3)
        XCTAssertEqual(doc.node(layer)?.kind, .fill)
        XCTAssertNotNil(t.session)
        // Esc: nothing changed. Apply with consent: the smart object with its Warp row.
        await finish { t.cancel() }
        XCTAssertNil(t.previewLabel)
        XCTAssertEqual(doc.node(layer)?.kind, .fill)
        try await begin(.warp)
        t.warpBend = 40
        t.applyWarpPreset("Arc")
        await settle(0.3)
        XCTAssertEqual(doc.node(layer)?.kind, .fill)
        await finish { t.apply(); t.confirmConversion() }
        XCTAssertEqual(doc.node(layer)?.kind, .smartObject, "Apply shows the result")
    }

    // MARK: 6 — no stale session hints

    func testSessionInstructionsEndWithTheSession() async throws {
        try await begin(.warp)
        XCTAssertTrue(status.hasPrefix("Warp:"), status)
        await finish { t.cancel() }
        XCTAssertEqual(status, DocumentTool.move.idleHint, "Cancel: back to the tool's hint")

        try await begin(.contentAwareScale)
        XCTAssertTrue(status.hasPrefix("Content-Aware Scale:"), status)
        t.setScale(width: 500)
        await settle()
        await finish { t.apply(); t.confirmConversion() }
        XCTAssertFalse(status.contains("drag the right or bottom handle"), "no instructions after Apply: \(status)")
        XCTAssertEqual(status, "Content-Aware Scale applied")

        // A tool change during a session (a smart object now: it applies) shows the new tool's hint.
        try await begin(.warp)
        t.warpBend = 20
        t.applyWarpPreset("Arc")
        await settle()
        await finish { tools.select(.brush) }
        XCTAssertNil(t.session)
        XCTAssertEqual(status, DocumentTool.brush.idleHint, "the new tool's hint, not the Warp instructions")
        // And back to the tool that was on before the session: its hint again, never the Warp one.
        try await begin(.warp)
        await finish { tools.select(.brush) }
        XCTAssertEqual(status, DocumentTool.brush.idleHint)
    }

    // MARK: A review — non-finite input never reaches the session

    /// "nan" in W / H trapped in `UInt32(Double.nan)`; NaN / infinity also reached Rotate and Bend.
    func testNonFiniteTextIsNotANumber() {
        let f = field(value: 12) { _ in XCTFail("no commit") }
        f.fractionDigits = 1
        for s in ["nan", "NaN", "-nan", "inf", "-inf", "infinity", "-Infinity", "1e999", "-1e999"] {
            f.stringValue = s
            XCTAssertNil(f.parsed(), "\(s) is not a value")
        }
        f.stringValue = "12,5"
        XCTAssertEqual(f.parsed(), 12.5)
        f.stringValue = " 40 "
        XCTAssertEqual(f.parsed(), 40)
    }

    func testTypedNaNInWidthOrBendCommitsNothingAndRestoresTheValue() async throws {
        try await begin(.contentAwareScale)
        var commits: [Double] = []
        let w = field(value: 600) { commits.append($0); DocumentTransforms.shared.setScale(width: UInt32(min(max($0, 1), 2400))) }
        w.onReturn = nil   // the commit itself, not the Apply that follows Return
        try type("nan", then: 36, "\r", into: w)
        await settle()
        XCTAssertEqual(commits, [], "NaN is refused before it reaches the session")
        XCTAssertEqual(w.stringValue, "600", "the field shows the accepted value again")
        await finish { t.cancel() }

        try await begin(.warp)
        let bend = t.warpBend
        let b = field(value: bend) { commits.append($0); DocumentTransforms.shared.setWarpBend($0) }
        b.onReturn = nil
        for s in ["inf", "-infinity", "NaN"] {
            try type(s, then: 36, "\r", into: b)
            await settle()
        }
        XCTAssertEqual(commits, [])
        XCTAssertEqual(t.warpBend, bend)
        XCTAssertTrue(t.warpBend.isFinite)
    }
}
