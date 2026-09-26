import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers
import XCTest
import TesseraFFI
@testable import TesseraCore
@testable import Tessera

/// The tether naming template (Swift mirror of crates/tether/src/naming.rs).
final class TetherNamingTests: XCTestCase {
    func testRendersTokensWithFourDigitSequence() throws {
        XCTAssertEqual(try TetherNaming.render("{sequence}_{original}.{ext}", original: "DSC01234.ARW", sequence: 7),
                       "0007_DSC01234.ARW")
        XCTAssertEqual(try TetherNaming.render("studio-{original}-{sequence}.{ext}", original: "a.b.jpg", sequence: 12345),
                       "studio-a.b-12345.jpg")
        XCTAssertEqual(try TetherNaming.example("{sequence}.{ext}").get(), "0001.ARW")
    }

    func testTokenMenuInsertsBeforeTheExtension() {
        XCTAssertEqual(TetherNaming.inserting("{original}", into: "studio_{sequence}.{ext}"), "studio_{sequence}_{original}.{ext}")
        XCTAssertEqual(TetherNaming.inserting("{ext}", into: "studio_{sequence}."), "studio_{sequence}.{ext}")
        XCTAssertEqual(TetherNaming.inserting("{sequence}", into: "shoot"), "shoot{sequence}")
    }

    func testRejectsWhatTheEngineRejects() {
        func problem(_ t: String) -> TetherNaming.Problem? {
            if case .failure(let p) = TetherNaming.example(t) { return p }
            return nil
        }
        XCTAssertEqual(problem(""), .empty)
        XCTAssertEqual(problem(".{sequence}.{ext}"), .hidden)
        XCTAssertEqual(problem("../{sequence}.{ext}"), .hidden)
        XCTAssertEqual(problem("a/{sequence}.{ext}"), .pathOrUnknownToken)
        XCTAssertEqual(problem("{sequence}_{camera}.{ext}"), .pathOrUnknownToken)
        XCTAssertEqual(problem("{sequence}:{ext}.{ext}"), .pathOrUnknownToken)
        XCTAssertEqual(problem("{sequence}.tif"), .extensionChanged)
        XCTAssertEqual(problem("{sequence}"), .extensionChanged)
        XCTAssertEqual(problem(String(repeating: "x", count: 240) + ".{ext}"), .tooLong)
        XCTAssertNil(problem("{original}.{ext}"))
        XCTAssertThrowsError(try TetherNaming.render("{sequence}.{ext}", original: "NOEXT", sequence: 1))
    }

    func testSessionNames() {
        let date = ISO8601DateFormatter().date(from: "2026-09-25T10:00:00Z")!
        let taken: Set = ["Tether 2026-09-25", "Tether 2026-09-25 (2)"]
        let name = TetherNaming.defaultSessionName(date: date) { taken.contains($0) }
        XCTAssertTrue(name.hasPrefix("Tether 2026-09-2"))
        XCTAssertEqual(TetherNaming.sessionFolderName(" Smith / Jones: day 1 "), "Smith - Jones- day 1")
        XCTAssertEqual(TetherNaming.sessionFolderName(".."), "Tether session")
    }
}

/// The incoming strip and interval schedule (value types behind the Tether panel).
final class IncomingStripTests: XCTestCase {
    private func frame(_ seq: UInt64, ok: Bool = true, sharpness: Double? = 0.7, faces: Int? = nil,
                       faceFocus: Double? = nil, eyes: Double? = nil) -> IncomingFrame {
        IncomingFrame(sequence: seq, url: URL(fileURLWithPath: "/s/\(seq).jpg"), imageID: ok ? "id\(seq)" : nil,
                      sharpness: sharpness, faces: faces, faceFocus: faceFocus, eyesOpen: eyes,
                      error: ok ? nil : "no embedded JPEG preview")
    }

    func testNewestFirstCappedWithPendingRequests() {
        var strip = IncomingStrip(capacity: 3)
        strip.captureRequested()
        strip.captureRequested()
        XCTAssertEqual(strip.pending, 2)
        XCTAssertEqual(strip.summary, "0 frames · 2 on the way")
        let target = strip.receive([frame(2), frame(1)])
        XCTAssertEqual(target?.sequence, 2, "auto-advance goes to the newest frame")
        XCTAssertEqual(strip.frames.map(\.sequence), [2, 1])
        XCTAssertEqual(strip.pending, 0)
        strip.receive([frame(3), frame(4)])   // physical shutter: no request, pending stays 0
        XCTAssertEqual(strip.pending, 0)
        XCTAssertEqual(strip.frames.map(\.sequence), [4, 3, 2])
        XCTAssertEqual(strip.received, 4)
    }

    func testFailedFramesAreKeptButNotAdvancedTo() {
        var strip = IncomingStrip()
        strip.captureRequested()
        strip.captureFailed()
        strip.captureFailed()
        XCTAssertEqual(strip.pending, 0)
        let target = strip.receive([frame(1), frame(2, ok: false)])
        XCTAssertEqual(target?.sequence, 1)
        XCTAssertEqual(strip.latest?.sequence, 2)
        XCTAssertEqual(strip.summary, "2 frames · 1 failed")
        XCTAssertNil(strip.receive([frame(3, ok: false)]))
    }

    func testBadgesPreferTheSubjectsFaceAndNeverInventEyes() {
        let sharp = frame(1, sharpness: 0.8)
        XCTAssertEqual(sharp.focusLevel, .good)
        XCTAssertFalse(sharp.hasFaces)
        XCTAssertEqual(sharp.eyesLevel, .unknown)
        let softFace = frame(2, sharpness: 0.9, faces: 2, faceFocus: 0.4, eyes: 0.1)
        XCTAssertTrue(softFace.focusIsFace)
        XCTAssertEqual(softFace.focusLevel, .fair)
        XCTAssertEqual(softFace.eyesLevel, .poor)
        XCTAssertEqual(frame(3, sharpness: 0.1).focusLevel, .poor)
        XCTAssertEqual(frame(4, sharpness: nil).focusLevel, .unknown)
    }

    func testIntervalPlanCountsDown() {
        var plan = IntervalPlan(seconds: 0.2, count: 2)
        XCTAssertEqual(plan.seconds, 1, "at least a second between shots")
        XCTAssertTrue(plan.shoot())
        XCTAssertEqual(plan.remaining, 1)
        XCTAssertTrue(plan.shoot())
        XCTAssertTrue(plan.isFinished)
        XCTAssertFalse(plan.shoot())
        var open = IntervalPlan(seconds: 5, count: 0)
        for _ in 0..<50 { XCTAssertTrue(open.shoot()) }
        XCTAssertNil(open.remaining)
    }
}

/// The engine's tether session with the test camera, from Swift (what `--fake-tether` drives).
final class TetherBridgeTests: XCTestCase {
    private func jpeg(_ url: URL, shade: UInt8) throws {
        let w = 64, h = 48
        var pixels = [UInt8](repeating: 0, count: w * h * 4)
        for y in 0..<h { for x in 0..<w {
            let i = (y * w + x) * 4
            pixels[i] = shade &+ UInt8(x * 3); pixels[i + 1] = UInt8(y * 4); pixels[i + 2] = UInt8((x ^ y) & 0xFF); pixels[i + 3] = 255
        } }
        let ctx = try XCTUnwrap(CGContext(data: &pixels, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
                                          space: CGColorSpace(name: CGColorSpace.sRGB)!,
                                          bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
        let image = try XCTUnwrap(ctx.makeImage())
        let dest = try XCTUnwrap(CGImageDestinationCreateWithURL(url as CFURL, UTType.jpeg.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(dest, image, nil)
        XCTAssertTrue(CGImageDestinationFinalize(dest))
    }

    @MainActor func testFakeCameraFrameArrivesNamedIndexedAndScored() throws {
        let temp = FileManager.default.temporaryDirectory.appendingPathComponent("tether-test-\(UUID().uuidString)")
        addTeardownBlock { try? FileManager.default.removeItem(at: temp) }
        let card = temp.appendingPathComponent("card")
        try FileManager.default.createDirectory(at: card, withIntermediateDirectories: true)
        try jpeg(card.appendingPathComponent("IMG_0001.jpg"), shade: 10)
        let engine = try Engine.open(appSupportDir: temp.appendingPathComponent("support").path)
        try engine.tetherUseFake(sourceFolder: card.path, intervalMs: 0)
        defer { try? engine.tetherUseFake(sourceFolder: nil, intervalMs: 0) }
        XCTAssertEqual(try engine.tetherDevices().first?.shotsRemaining, 1)
        let session = temp.appendingPathComponent("shoot/Tether test")
        try FileManager.default.createDirectory(at: session, withIntermediateDirectories: true)
        try engine.tetherStart(sessionFolder: session.path, naming: "{sequence}_{original}.{ext}")
        try engine.tetherCapture()
        var strip = IncomingStrip()
        strip.captureRequested()
        let deadline = Date().addingTimeInterval(30)
        while strip.received == 0, Date() < deadline {
            strip.receive(try engine.tetherPoll().map(IncomingFrame.init))
            RunLoop.current.run(until: Date().addingTimeInterval(0.05))
        }
        _ = try engine.tetherStop()
        let frame = try XCTUnwrap(strip.latest)
        XCTAssertNil(frame.error)
        XCTAssertEqual(frame.name, "0001_IMG_0001.jpg")
        XCTAssertNotNil(frame.imageID)
        XCTAssertNotNil(frame.sharpness)
        XCTAssertNil(frame.faces, "no face models: not analysed, not zero faces")
        XCTAssertNotNil(frame.faceWarning)
        XCTAssertNotNil(TetherController.thumbnail(try XCTUnwrap(frame.preview)))
        XCTAssertFalse(engine.tetherActive())
    }

    @MainActor func testLaunchArguments() {
        let t = TetherController(arguments: ["Tessera", "--fake-tether", "/tmp/card", "--fake-tether-interval", "0"])
        XCTAssertEqual(t.fakeSource?.path, "/tmp/card")
        XCTAssertEqual(t.fakeInterval, 0)
        XCTAssertFalse(TetherController(arguments: ["Tessera"]).isFake)
    }
}
