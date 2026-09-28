import AppKit
import CoreGraphics
import Foundation
import ImageIO
import IOSurface
import QuartzCore
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
        // A caller asking for one slot still gets a live ring (display lease + writer).
        let plan = try controller.attachSurfaces(viewWidth: 640, viewHeight: 480, count: 1)
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
        XCTAssertTrue(bright.readout.hasPrefix("engine sink: L"))
        XCTAssertTrue(bright.readout.contains("not input-to-display"))
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

    /// The complete synchronous Swift setter boundary, including patch merge,
    /// encoding and FFI, while cold Auto Upright work runs on the worker.
    func testAutoUprightMainSetterAndFinalIdentity() async throws {
        let (folder, support) = try scratchRaw()
        let library = try EngineLibrary.scan(folder: folder, appSupport: support)
        let item = try XCTUnwrap(library.items.first)
        let controller = try await DevelopController.open(try XCTUnwrap(item.engineImage), itemID: item.id)
        _ = try controller.attachSurfaces(viewWidth: 1280, viewHeight: 900)
        let first = try await nextFrame(controller, after: 0)
        var samples: [Double] = []
        for index in 0...100 {
            let start = CACurrentMediaTime()
            controller.apply(patch: ["geometry": ["upright": ["mode": "auto"]],
                                     "tone": ["exposure": Double(index) / 100]],
                             interactive: index != 100)
            samples.append((CACurrentMediaTime() - start) * 1000)
        }
        var last = try await nextFrame(controller, after: first.generation)
        while last.inputID != 101 { last = try await nextFrame(controller, after: last.generation) }
        XCTAssertEqual(last.inputID, 101)
        XCTAssertEqual(controller.histogram?.generation, last.generation)
        samples.sort()
        let median = samples[50], p95 = samples[95], maximum = samples[100]
        print("M2-58 Auto Upright Swift setter: count=101 median_ms=\(median) p95_ms=\(p95) max_ms=\(maximum)")
        XCTAssertLessThan(p95, 2, "P10 main setter p95")
        XCTAssertLessThan(maximum, 8, "P10 main setter maximum")
        let detailSurface = try XCTUnwrap(DevelopController.makeDetailSurface(width: 160, height: 120))
        let preview = try DevelopController.renderDetail(session: controller.session, into: detailSurface,
                                                        centerX: 0.5, centerY: 0.5)
        XCTAssertEqual(preview.revision, controller.session.detailRevision())
        _ = try XCTUnwrap(controller.addMask(LinearGradientShape(start: (0.5, 0.1), end: (0.5, 0.9)).json))
        XCTAssertNotEqual(preview.revision, controller.session.detailRevision(),
                          "direct mask edits invalidate detail before their viewport callback")
        await controller.close()
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
            let expectedPosition = try XCTUnwrap(model.visible.firstIndex(of: item.id))
            model.select(id: item.id)
            try await settle {
                model.focusedItem?.id == item.id && model.selection == IndexSet(integer: expectedPosition)
            }
            model.openDevelop(for: item)
            try await settle { model.developStatus == .ready && model.develop?.itemID == item.id }
            XCTAssertEqual(model.developStatus, .ready, "\(name): \(model.developStatus)")
            XCTAssertEqual(model.develop?.itemID, item.id)
            if let close = model.closeDevelop() {
                switch await close.value {
                case .saved: break
                case .failed(_, let message): XCTFail("\(name) Develop close failed: \(message)")
                }
            }
            try await settle { model.develop == nil && model.developStatus == .none }
        }
    }

    private func settle(_ condition: @MainActor () -> Bool,
                        file: StaticString = #filePath, line: UInt = #line) async throws {
        let deadline = Date().addingTimeInterval(20)
        while !condition(), Date() < deadline {
            try await Task.sleep(for: .milliseconds(20))
        }
        XCTAssertTrue(condition(), "Develop state did not settle", file: file, line: line)
    }

    private func mean(_ bins: [UInt32]) -> Double {
        let n = bins.reduce(0.0) { $0 + Double($1) }
        return bins.enumerated().reduce(0.0) { $0 + Double($1.offset) * Double($1.element) } / max(n, 1)
    }
}
