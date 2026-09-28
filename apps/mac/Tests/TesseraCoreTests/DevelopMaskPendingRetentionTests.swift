import XCTest
import TesseraFFI
@testable import TesseraCore

// UNRUN source tests for A. No real session, image, render thread or GPU.
@MainActor
final class DevelopMaskPendingRetentionTests: XCTestCase {
    private func fixture() throws -> (DevelopController, MaskFaultSession) {
        let session = MaskFaultSession()
        let controller = try DevelopController(session: session, itemID: 0, imageID: "mask-test")
        controller.onNeedsFlush = {} // Coalesce until explicitly drained by the test.
        return (controller, session)
    }
    private func patch(_ amount: Float? = nil, name: String? = nil) -> MaskGroupPatch {
        MaskGroupPatch(name: name, enabled: nil, amount: amount, invert: nil)
    }

    func testRejectedComponentAndUnattemptedGroupsParamsSurvive() throws {
        let (c, s) = try fixture()
        c.setMaskComponent(1, 0, json: "old", interactive: true)
        c.updateMaskGroup(2, patch(0.5), interactive: true)
        c.setMaskParam(3, "exposure", 0.7, interactive: true)
        s.reject = "component:1"
        XCTAssertTrue(c.flushMaskPending(), "Bool is attempted work, not persistence success")
        XCTAssertEqual(s.calls, ["component:1"])
        XCTAssertEqual(c.pendingComponent?.json, "old")
        XCTAssertNotNil(c.pendingMaskGroup[2])
        XCTAssertEqual(c.pendingMaskParams[.init(group: 3, name: "exposure")], Float(0.7))
        s.reject = nil
        XCTAssertTrue(c.flushMaskPending())
        XCTAssertEqual(s.calls, ["component:1", "component:1", "group:2", "param:3:exposure"])
        XCTAssertFalse(c.hasPendingMaskChanges)
    }

    func testAcceptedPrefixAndBrushPointsNeverReplayOnLaterFailure() throws {
        let (c, s) = try fixture()
        c.pendingStroke = StrokeCoalescer(spacing: 0)
        c.addBrushSample(x: 0, y: 0, pressure: 1)
        c.setMaskComponent(1, 0, json: "component", interactive: true)
        c.updateMaskGroup(1, patch(0.2), interactive: true)
        c.updateMaskGroup(2, patch(0.3), interactive: true)
        c.setMaskParam(3, "a", 1, interactive: true)
        s.reject = "group:2"
        c.flushMaskPending()
        XCTAssertEqual(s.calls, ["brush", "component:1", "group:1", "group:2"])
        XCTAssertNil(c.pendingMaskGroup[1])
        XCTAssertNotNil(c.pendingMaskGroup[2])
        XCTAssertEqual(c.pendingStroke?.pendingCount, 0)
        s.reject = nil
        c.flushMaskPending()
        XCTAssertEqual(s.calls.filter { $0 == "brush" }.count, 1)
        XCTAssertEqual(s.calls.filter { $0 == "component:1" }.count, 1)
        XCTAssertEqual(s.calls.filter { $0 == "group:1" }.count, 1)
        XCTAssertFalse(c.hasPendingMaskChanges)
    }

    func testParameterFailureKeepsRejectedSuffixOnly() throws {
        let (c, s) = try fixture()
        for name in ["a", "b", "c"] { c.setMaskParam(1, name, 1, interactive: true) }
        s.reject = "param:1:b"
        c.flushMaskPending()
        XCTAssertNil(c.pendingMaskParams[.init(group: 1, name: "a")])
        XCTAssertEqual(c.pendingMaskParams.count, 2)
        s.reject = nil
        c.flushMaskPending()
        XCTAssertEqual(s.calls, ["param:1:a", "param:1:b", "param:1:b", "param:1:c"])
    }

    func testReentrantNewerComponentAndParamSurviveFailedAttempt() throws {
        let (c, s) = try fixture()
        defer { s.before = nil }
        c.setMaskComponent(1, 0, json: "old", interactive: true)
        s.reject = "component:1"
        s.before = { _ in
            c.setMaskComponent(1, 0, json: "new", interactive: true)
            XCTAssertFalse(c.flushMaskPending(), "must not recursively submit active work")
        }
        c.flushMaskPending()
        XCTAssertEqual(c.pendingComponent?.json, "new")
        s.before = nil; s.reject = nil
        c.flushMaskPending()
        c.setMaskParam(1, "exposure", 1, interactive: true)
        s.reject = "param:1:exposure"
        s.before = { _ in c.setMaskParam(1, "exposure", 2, interactive: true) }
        c.flushMaskPending()
        XCTAssertEqual(c.pendingMaskParams[.init(group: 1, name: "exposure")], 2)
    }

    func testRejectedGroupMergesOlderFieldsUnderNewerPatch() throws {
        let (c, s) = try fixture()
        defer { s.before = nil }
        c.updateMaskGroup(1, patch(0.2, name: "retain-name"), interactive: true)
        s.reject = "group:1"
        s.before = { _ in c.updateMaskGroup(1, self.patch(0.8), interactive: true) }
        c.flushMaskPending()
        XCTAssertEqual(c.pendingMaskGroup[1]?.patch.amount, 0.8)
        XCTAssertEqual(c.pendingMaskGroup[1]?.patch.name, "retain-name")
    }

    func testSuccessfulAcknowledgementKeepsNewerSameKeyValues() throws {
        let (c, s) = try fixture()
        defer { s.before = nil }
        c.setMaskComponent(1, 0, json: "old", interactive: true)
        c.updateMaskGroup(1, patch(0.2), interactive: true)
        c.setMaskParam(1, "exposure", 1, interactive: true)
        s.before = { operation in
            switch operation {
            case "component:1": c.setMaskComponent(1, 0, json: "new", interactive: true)
            case "group:1": c.updateMaskGroup(1, self.patch(0.8), interactive: true)
            case "param:1:exposure": c.setMaskParam(1, "exposure", 2, interactive: true)
            default: XCTFail("Unexpected operation")
            }
        }
        c.flushMaskPending()
        XCTAssertEqual(c.pendingComponent?.json, "new")
        XCTAssertEqual(c.pendingMaskGroup[1]?.patch.amount, 0.8)
        XCTAssertEqual(c.pendingMaskParams[.init(group: 1, name: "exposure")], 2)
        s.before = nil
        c.flushMaskPending()
        XCTAssertFalse(c.hasPendingMaskChanges)
        XCTAssertEqual(s.calls.count, 6)
    }

    func testBrushRejectionRetainsSamplesAndSuccessKeepsReentrantSample() throws {
        let (c, s) = try fixture()
        c.pendingStroke = StrokeCoalescer(spacing: 0)
        c.addBrushSample(x: 0, y: 0, pressure: 1)
        s.reject = "brush"
        c.flushMaskPending()
        XCTAssertEqual(c.pendingStroke?.pendingCount, 1)
        s.reject = nil
        s.before = { _ in c.addBrushSample(x: 1, y: 0, pressure: 1) }
        c.flushMaskPending()
        XCTAssertEqual(c.pendingStroke?.pendingCount, 1, "new sample must not be overwritten by acknowledgement")
        s.before = nil
        c.flushMaskPending()
        XCTAssertEqual(s.acceptedBrushPoints, [0, 1])
        XCTAssertFalse(c.hasPendingMaskChanges)
    }
}

// Accessed synchronously on MainActor by the controller; never passed to real
// UniFFI or a worker. Restate unchecked Sendable only to subclass generated fake.
private final class MaskFaultSession: DevelopSession, @unchecked Sendable {
    var reject: String?
    var before: (@MainActor (String) -> Void)?
    var calls: [String] = []
    var acceptedBrushPoints: [Float] = []
    init() { super.init(noHandle: .init()) }
    required init(unsafeFromHandle: UInt64) { fatalError("fake only") }
    private func attempt(_ name: String) throws {
        calls.append(name)
        MainActor.assumeIsolated { before?(name) }
        if reject == name { throw NSError(domain: "MaskFault", code: 1) }
    }
    override func info() -> DevelopInfo {
        DevelopInfo(imageId: "mask-test", width: 2, height: 2, orientation: 1,
                    asShotTemperature: 6500, asShotTint: 0, backend: "fake")
    }
    override func historyState() throws -> HistoryState {
        HistoryState(canUndo: false, canRedo: false, entries: 0, headLabel: nil, snapshots: [], uncommitted: false)
    }
    override func getSettingsJson() throws -> String { "{}" }
    override func ignoredSettings() throws -> [String] { [] }
    override func setListener(listener: DevelopListener?) {}
    override func setMaskListener(listener: MaskListener?) {}
    override func addBrushPoints(points: [BrushPoint]) throws {
        try attempt("brush")
        acceptedBrushPoints += points.map(\.x)
    }
    override func setMaskComponent(groupId: UInt32, index: UInt32, definitionJson: String, interactive: Bool) throws {
        try attempt("component:\(groupId)")
    }
    override func updateMaskGroup(groupId: UInt32, patch: MaskGroupPatch, interactive: Bool) throws {
        try attempt("group:\(groupId)")
    }
    override func setMaskParam(groupId: UInt32, name: String, value: Float, interactive: Bool) throws {
        try attempt("param:\(groupId):\(name)")
    }
}
