import AppKit
import CoreGraphics
import Foundation
import ImageIO
import IOSurface
import XCTest
import TesseraFFI
@testable import TesseraCore
@testable import Tessera

/// Develop session through the bridge on a real RAW (the Sony ARW fixture, copied to scratch).
@MainActor
final class DevelopTests: XCTestCase {
    private var root: URL {
        URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    }

    /// One RAW in a scratch folder. Fails (rather than skips) without fixtures, like BridgeTests.
    private func scratchRaw() throws -> (folder: URL, support: URL) {
        let fixtures = root.appendingPathComponent("../../fixtures/raw").standardizedFileURL
        let raw = try XCTUnwrap(try FileManager.default.contentsOfDirectory(at: fixtures, includingPropertiesForKeys: nil)
            .first { $0.pathExtension.lowercased() == "arw" }, "fetch fixtures/raw first")
        let temp = root.appendingPathComponent("build/develop-test-\(UUID().uuidString)")
        let folder = temp.appendingPathComponent("raw")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: temp) }
        try FileManager.default.copyItem(at: raw, to: folder.appendingPathComponent(raw.lastPathComponent))
        return (folder, temp.appendingPathComponent("support"))
    }

    /// Waits for the final frame of a render newer than `after`.
    private func nextFrame(_ c: DevelopController, after generation: UInt64) async throws -> DevelopFrame {
        let deadline = Date().addingTimeInterval(60)
        while Date() < deadline {
            if let f = c.lastFrame, f.isFinal, f.generation > generation { return f }
            try await Task.sleep(for: .milliseconds(10))
        }
        struct Timeout: Error {}
        XCTFail("no frame within 60 s")
        throw Timeout()
    }

    func testSettingsChangeRendersIntoTheSurfaceAndReopenRestoresExposure() async throws {
        let (folder, support) = try scratchRaw()
        let library = try EngineLibrary.scan(folder: folder, appSupport: support)
        let item = try XCTUnwrap(library.items.first)
        let controller = try await DevelopController.open(try XCTUnwrap(item.engineImage), itemID: item.id)
        var frames: [DevelopFrame] = []
        controller.onFrame = { frames.append($0) }
        XCTAssertGreaterThan(controller.info.width, 1000)
        XCTAssertEqual(controller.value(.exposure), 0)
        XCTAssertTrue(controller.isAsShotWhiteBalance)
        XCTAssertEqual(controller.value(.temperature), Double(controller.info.asShotTemperature))

        // Surfaces are allocated at the planned level and the first frame lands in one of them.
        let plan = try controller.attachSurfaces(viewWidth: 640, viewHeight: 480)
        XCTAssertGreaterThanOrEqual(Int(plan.width), 480)
        let first = try await nextFrame(controller, after: 0)
        let surface = try XCTUnwrap(controller.surface(first.surfaceID), "frame names an attached surface")
        XCTAssertEqual(IOSurfaceGetWidth(surface), Int(plan.width))
        XCTAssertEqual(first.width, Int(plan.width))
        let before = try XCTUnwrap(controller.histogram)
        XCTAssertEqual(before.luminance.count, 256)

        // A coalesced interactive change is sent on the next flush and produces frame_ready.
        var ticksRequested = 0
        controller.onNeedsFlush = { ticksRequested += 1 }
        controller.set(.exposure, 0.5, interactive: true)
        controller.set(.exposure, 1.0, interactive: true)
        XCTAssertEqual(ticksRequested, 2)
        XCTAssertEqual(controller.value(.exposure), 1.0, "the model shows the latest value at once")
        XCTAssertTrue(controller.flushPending())
        XCTAssertFalse(controller.flushPending(), "nothing left to send")
        let bright = try await nextFrame(controller, after: first.generation)
        XCTAssertEqual(bright.dirtyStage, "Tone")
        XCTAssertTrue(frames.contains(bright))
        XCTAssertTrue(bright.readout.hasPrefix("render: L"))
        let after = try XCTUnwrap(controller.histogram)
        XCTAssertGreaterThan(mean(after.luminance), mean(before.luminance) + 10)
        XCTAssertTrue(controller.commit(label: "Exposure +1.00"))
        XCTAssertTrue(controller.history.canUndo)
        XCTAssertEqual(controller.history.headLabel, "Exposure +1.00")

        // Undo / redo go through the session.
        XCTAssertTrue(try controller.undo())
        XCTAssertEqual(controller.value(.exposure), 0)
        XCTAssertTrue(try controller.redo())
        XCTAssertEqual(controller.value(.exposure), 1)

        // Moving Temperature leaves As Shot and keeps the displayed tint.
        controller.set(.temperature, 4000, interactive: false)
        XCTAssertFalse(controller.isAsShotWhiteBalance)
        XCTAssertEqual(controller.value(.temperature), 4000)
        _ = try await nextFrame(controller, after: bright.generation)
        XCTAssertTrue(try controller.undo(), "uncommitted change is committed, then undone")
        XCTAssertTrue(controller.isAsShotWhiteBalance)

        var saved = false
        controller.onSaved = { _ in saved = true }
        await controller.close()   // flushes the recipe + XMP

        // Reopen the library (a new engine, as after relaunch): the edit is restored.
        let reopened = try EngineLibrary.scan(folder: folder, appSupport: support)
        let again = try XCTUnwrap(reopened.items.first)
        XCTAssertEqual(reopened.makeCullController().statuses[again.id].phase, .edited)
        let restored = try await DevelopController.open(try XCTUnwrap(again.engineImage), itemID: again.id)
        XCTAssertEqual(restored.value(.exposure), 1)
        XCTAssertTrue(restored.history.canUndo)
        await restored.close()
        _ = saved
    }

    func testJpegIsDevelopable() async throws {
        let temp = root.appendingPathComponent("build/develop-jpeg-\(UUID().uuidString)")
        addTeardownBlock { try? FileManager.default.removeItem(at: temp) }
        let folder = temp.appendingPathComponent("shoot")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let ctx = CGContext(data: nil, width: 8, height: 8, bitsPerComponent: 8, bytesPerRow: 0,
                            space: CGColorSpace(name: CGColorSpace.sRGB)!,
                            bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)!
        let dest = CGImageDestinationCreateWithURL(folder.appendingPathComponent("a.jpg") as CFURL,
                                                   "public.jpeg" as CFString, 1, nil)!
        CGImageDestinationAddImage(dest, ctx.makeImage()!, nil)
        XCTAssertTrue(CGImageDestinationFinalize(dest))
        let library = try EngineLibrary.scan(folder: folder, appSupport: temp.appendingPathComponent("support"))
        let item = try XCTUnwrap(library.items.first)
        let controller = try await DevelopController.open(try XCTUnwrap(item.engineImage), itemID: item.id)
        XCTAssertEqual(controller.info.width, 8)
        XCTAssertEqual(controller.info.height, 8)
        _ = try controller.attachSurfaces(viewWidth: 320, viewHeight: 320)
        let mask = try XCTUnwrap(controller.addMask(LinearGradientShape(start: (0.5, 0.1), end: (0.5, 0.9)).json))
        controller.setMaskParam(mask, "exposure", 1, interactive: false)
        XCTAssertEqual(controller.maskGroups().first?.params.first { $0.name == "exposure" }?.value, 1)
        XCTAssertNotNil(controller.addAIMask(group: nil, .subject, combine: .add),
                        "AI mask requests are accepted on indexed RGB; model availability is reported asynchronously")
        await controller.close()
    }

    func testAppModelOpensRenderedJpegAndHeic() async throws {
        _ = NSApplication.shared
        let temp = root.appendingPathComponent("build/develop-app-rendered-\(UUID().uuidString)")
        addTeardownBlock { try? FileManager.default.removeItem(at: temp) }
        let folder = temp.appendingPathComponent("shoot")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let jpeg = folder.appendingPathComponent("photo.jpg")
        let heic = folder.appendingPathComponent("photo.heic")
        let png = folder.appendingPathComponent("photo.png")
        let tiff = folder.appendingPathComponent("photo.tiff")
        let ctx = CGContext(data: nil, width: 32, height: 32, bitsPerComponent: 8, bytesPerRow: 0,
                            space: CGColorSpace(name: CGColorSpace.sRGB)!,
                            bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)!
        ctx.setFillColor(CGColor(gray: 0.3, alpha: 1))
        ctx.fill(CGRect(x: 0, y: 0, width: 32, height: 32))
        for (url, type) in [(jpeg, "public.jpeg"), (png, "public.png"), (tiff, "public.tiff")] {
            let destination = try XCTUnwrap(CGImageDestinationCreateWithURL(url as CFURL, type as CFString, 1, nil))
            CGImageDestinationAddImage(destination, try XCTUnwrap(ctx.makeImage()), nil)
            XCTAssertTrue(CGImageDestinationFinalize(destination))
        }
        let converter = Process()
        converter.executableURL = URL(fileURLWithPath: "/usr/bin/sips")
        converter.arguments = ["-s", "format", "heic", jpeg.path, "--out", heic.path]
        try converter.run()
        converter.waitUntilExit()
        XCTAssertEqual(converter.terminationStatus, 0)

        let library = try EngineLibrary.scan(folder: folder, appSupport: temp.appendingPathComponent("support"))
        let model = AppModel()
        model.install(library)
        model.viewMode = .loupe
        XCTAssertEqual(library.items.count, 4)
        for (name, kind) in [("photo.jpg", PhotoKind.jpeg), ("photo.heic", .heif),
                             ("photo.png", .png), ("photo.tiff", .tiff)] {
            let item = try XCTUnwrap(library.items.first { $0.name == name })
            XCTAssertEqual(item.kind, kind)
            XCTAssertNotNil(item.engineImage)
            model.select(id: item.id)
            model.openDevelop(for: item)
            let deadline = Date().addingTimeInterval(20)
            while model.developStatus == .loading && Date() < deadline {
                try await Task.sleep(for: .milliseconds(20))
            }
            XCTAssertEqual(model.developStatus, .ready, "\(name): \(model.developStatus)")
            XCTAssertEqual(model.develop?.itemID, item.id)
            model.closeDevelop()
        }
    }

    private func mean(_ bins: [UInt32]) -> Double {
        let n = bins.reduce(0.0) { $0 + Double($1) }
        return bins.enumerated().reduce(0.0) { $0 + Double($1.offset) * Double($1.element) } / max(n, 1)
    }
}
