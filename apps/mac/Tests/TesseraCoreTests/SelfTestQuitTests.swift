import AppKit
import ImageIO
import UniformTypeIdentifiers
import XCTest
import TesseraFFI
@testable import Tessera
@testable import TesseraCore

/// `--export-selftest`, `--print-pdf-selftest` and `--timing-selftest` quit from their finish
/// callback. `applicationShouldTerminate` refuses while a saved-read gate is held or a Develop
/// session is open, so the quit must be requested only once those have settled. The terminate
/// closure below records what `applicationShouldTerminate` would answer at that moment.
@MainActor
final class SelfTestQuitTests: XCTestCase {
    func testExportSelfTestQuitWaitsForSaveGateRelease() async throws {
        let rig = try makeRig()
        let defaults = UserDefaults.standard
        let oldSettings = defaults.object(forKey: "ExportSettings")
        let oldPreset = defaults.object(forKey: "ExportPresetName")
        defer {
            restore(oldSettings, key: "ExportSettings", in: defaults)
            restore(oldPreset, key: "ExportPresetName", in: defaults)
        }
        let model = rig.model
        let export = ExportController()
        let target = ExportController.Target(
            kind: .selection, title: "One photo", target: .images(imageIds: [rig.imageID]),
            count: 1, firstName: "photo.jpg", firstDate: Date(timeIntervalSince1970: 0))
        export.prepare(engine: rig.library.engine, targets: [target], preferred: .selection)
        export.settings = ExportSettings()
        export.settings.destination = rig.output.path
        export.acquireSaveGate = { [library = rig.library] ids in
            model.prepareForRecipeRead(imageIDs: ids, library: library)
        }
        var gateHeldInCallback: Bool?
        var admitted: Bool?
        export.onFinish = { report, _ in
            XCTAssertEqual(report.exported, 1)
            gateHeldInCallback = model.developRecovery.hasActiveReservations
            model.quitAfterSelfTest { admitted = model.developRecovery.allowsTermination }
        }
        export.onFailure = { message in XCTFail("export failed: \(message)") }

        export.start()
        try await waitUntil { admitted != nil }
        XCTAssertEqual(gateHeldInCallback, true, "onFinish runs before the job's gate is released")
        XCTAssertEqual(admitted, true, "the self-test quit must not be refused by the export's own gate")
    }

    func testPrintPDFSelfTestQuitWaitsForSaveGateRelease() async throws {
        let rig = try makeRig()
        let defaults = UserDefaults.standard
        let oldPrint = defaults.object(forKey: PrintSettings.defaultsKey)
        defer { restore(oldPrint, key: PrintSettings.defaultsKey, in: defaults) }
        let model = rig.model
        let printing = PrintController()
        // Synthetic item: a nonempty layout without a thumbnail read; the gate is the real one.
        let item = PhotoItem(id: 0, url: nil, name: "print-test", kind: .synthetic,
                             captureDate: Date(timeIntervalSince1970: 0), pixelWidth: 8, pixelHeight: 8)
        printing.prepare(items: [item], title: "One page", loader: ThumbnailLoader())
        printing.settings = PrintSettings()
        printing.settings.colorHandling = .printer
        printing.acquireSaveGate = { [library = rig.library, imageID = rig.imageID] _ in
            model.prepareForRecipeRead(imageIDs: [imageID], library: library)
        }
        let pdf = rig.output.appendingPathComponent("selftest.pdf")
        var gateHeldInCallback: Bool?
        var admitted: Bool?
        printing.run(.pdf(pdf), engine: rig.library.engine, window: nil) { ok in
            XCTAssertTrue(ok)
            gateHeldInCallback = model.developRecovery.hasActiveReservations
            model.quitAfterSelfTest { admitted = model.developRecovery.allowsTermination }
        }
        try await waitUntil { admitted != nil }
        XCTAssertEqual(gateHeldInCallback, true, "completion runs before the job's gate is released")
        XCTAssertEqual(admitted, true, "the self-test quit must not be refused by the print's own gate")
    }

    func testTimingSelfTestQuitClosesOpenDevelopSession() async throws {
        let rig = try makeRig()
        let model = rig.model
        let item = try XCTUnwrap(rig.library.items.first)
        model.openDevelop(for: item)
        try await waitUntil { model.develop != nil }
        XCTAssertFalse(model.developRecovery.allowsTermination, "an open loupe session blocks quitting")

        var admitted: Bool?
        model.quitAfterSelfTest { admitted = model.developRecovery.allowsTermination }
        try await waitUntil { admitted != nil }
        XCTAssertEqual(admitted, true, "the self-test quit must close the loupe's Develop session first")
    }

    private struct Rig {
        let model: AppModel
        let library: EngineLibrary
        let imageID: String
        let output: URL
    }

    private func makeRig() throws -> Rig {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("selftest-quit-\(UUID().uuidString)", isDirectory: true)
        let photos = root.appendingPathComponent("photos", isDirectory: true)
        let support = root.appendingPathComponent("support", isDirectory: true)
        let output = root.appendingPathComponent("output", isDirectory: true)
        try FileManager.default.createDirectory(at: photos, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: root) }
        try writePhoto(photos.appendingPathComponent("photo.jpg"))
        let library = try EngineLibrary.scan(folder: photos, appSupport: support)
        let imageID = try XCTUnwrap(library.items.first?.engineImage?.imageID)
        let model = AppModel(agent: AgentController(arguments: ["--fake-planner"], supportDirectory: support))
        model.install(library)
        return Rig(model: model, library: library, imageID: imageID, output: output)
    }

    private func writePhoto(_ url: URL) throws {
        let width = 64, height = 48
        let pixels = [UInt8](repeating: 128, count: width * height * 4)
        let provider = try XCTUnwrap(CGDataProvider(data: Data(pixels) as CFData))
        let image = try XCTUnwrap(CGImage(
            width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: width * 4,
            space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
            provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent))
        let destination = try XCTUnwrap(CGImageDestinationCreateWithURL(
            url as CFURL, UTType.jpeg.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(destination, image, nil)
        XCTAssertTrue(CGImageDestinationFinalize(destination))
    }

    private func waitUntil(_ condition: @MainActor () -> Bool) async throws {
        for _ in 0..<2000 {
            if condition() { return }
            try await Task.sleep(nanoseconds: 5_000_000)
        }
        XCTFail("self-test quit did not settle within ten seconds")
    }

    private func restore(_ object: Any?, key: String, in defaults: UserDefaults) {
        if let object { defaults.set(object, forKey: key) }
        else { defaults.removeObject(forKey: key) }
    }
}
