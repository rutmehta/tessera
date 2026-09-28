import XCTest
@testable import Tessera

final class TimingVisibleProtocolTests: XCTestCase {
    private func handshake(nonce: String = "run-a", pid: Int32 = 41, window: Int = 8,
                           session: String = "session-a", time: Double = 10,
                           bundleURL: String = "/tmp/Test.app", launchDate: Double = 1_800_000_000) -> TimingVisibleHandshake {
        TimingVisibleHandshake(nonce: nonce, pid: pid, bundleID: "dev.tessera.test",
                              bundleURL: bundleURL, launchDate: launchDate,
                              windowNumber: window, session: session, time: time)
    }

    func testAcceptsOneMatchingStartAndEndInterval() {
        let ready = handshake()
        var protocolState = TimingVisibleIntervalProtocol(ready: ready)
        XCTAssertTrue(protocolState.acceptStart(handshake(time: 10.5), now: 10.6, visible: true))
        XCTAssertEqual(protocolState.phase, .measuring)
        XCTAssertTrue(protocolState.end(now: 12, identity: ready, visible: true))
        XCTAssertEqual(protocolState.phase, .ended)
        XCTAssertFalse(protocolState.end(now: 13, identity: ready, visible: true))
        XCTAssertEqual(protocolState.measurementStart, 10.6)
        XCTAssertEqual(protocolState.measurementEnd, 12)
    }

    func testRejectsStaleNonceAndWrongProcessWindowOrSession() {
        for permit in [handshake(nonce: "stale", time: 10.5), handshake(pid: 42, time: 10.5),
                       handshake(window: 9, time: 10.5), handshake(session: "other", time: 10.5),
                       handshake(bundleURL: "/tmp/Other.app", time: 10.5),
                       handshake(launchDate: 1_800_000_001, time: 10.5)] {
            var protocolState = TimingVisibleIntervalProtocol(ready: handshake())
            XCTAssertFalse(protocolState.acceptStart(permit, now: 10.6, visible: true))
            XCTAssertEqual(protocolState.phase, .failed)
        }
    }

    func testRejectsDuplicateStartEarlyEndAndTimeout() {
        let ready = handshake()
        var duplicate = TimingVisibleIntervalProtocol(ready: ready)
        XCTAssertTrue(duplicate.acceptStart(handshake(time: 10.5), now: 10.6, visible: true))
        XCTAssertFalse(duplicate.acceptStart(handshake(time: 10.7), now: 10.8, visible: true))
        XCTAssertEqual(duplicate.phase, .failed)

        var earlyEnd = TimingVisibleIntervalProtocol(ready: ready)
        XCTAssertFalse(earlyEnd.end(now: 11, identity: ready, visible: true))
        XCTAssertEqual(earlyEnd.phase, .failed)

        var expired = TimingVisibleIntervalProtocol(ready: ready)
        expired.expire(now: 55.1)
        XCTAssertEqual(expired.phase, .failed)

        var timedOutMeasurement = TimingVisibleIntervalProtocol(ready: ready)
        XCTAssertTrue(timedOutMeasurement.acceptStart(handshake(time: 10.5), now: 10.6, visible: true))
        timedOutMeasurement.expire(now: 55.7)
        XCTAssertEqual(timedOutMeasurement.phase, .failed)
    }

    func testHandshakeUsesStableSnakeCaseRunnerFields() throws {
        let record = handshake()
        let data = try JSONEncoder().encode(record)
        let object = try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? [String: Any])
        XCTAssertEqual(object["bundle_id"] as? String, record.bundleID)
        XCTAssertEqual(object["bundle_url"] as? String, record.bundleURL)
        XCTAssertEqual(object["launch_date"] as? Double, record.launchDate)
        XCTAssertEqual(object["window_number"] as? Int, record.windowNumber)
        XCTAssertNil(object["bundleID"])
        XCTAssertEqual(try JSONDecoder().decode(TimingVisibleHandshake.self, from: data), record)
    }

    func testRejectsNonfinitePermitAndIntervalTimes() {
        let ready = handshake()
        var invalidPermit = TimingVisibleIntervalProtocol(ready: ready)
        XCTAssertFalse(invalidPermit.acceptStart(handshake(time: .infinity), now: 10.7, visible: true))
        XCTAssertEqual(invalidPermit.phase, .failed)

        var invalidNow = TimingVisibleIntervalProtocol(ready: ready)
        XCTAssertFalse(invalidNow.acceptStart(handshake(time: 10.5), now: .infinity, visible: true))
        XCTAssertEqual(invalidNow.phase, .failed)

        var invalidEnd = TimingVisibleIntervalProtocol(ready: ready)
        XCTAssertTrue(invalidEnd.acceptStart(handshake(time: 10.5), now: 10.6, visible: true))
        XCTAssertFalse(invalidEnd.end(now: .infinity, identity: ready, visible: true))
        XCTAssertEqual(invalidEnd.phase, .failed)
    }
}
