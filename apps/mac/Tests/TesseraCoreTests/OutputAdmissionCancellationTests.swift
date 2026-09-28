import AppKit
import ImageIO
import UniformTypeIdentifiers
import XCTest
import TesseraFFI
@testable import Tessera
@testable import TesseraCore

/// Draft for TesseraCoreTests. These are admission tests: start/run and cancel execute
/// synchronously on MainActor, before their unstructured Task can begin native output work.
@MainActor
final class OutputAdmissionCancellationTests: XCTestCase {
    func testExportCancelBeforeTaskStartsSettlesAndReleasesReservation() async throws {
        let rig = try makeRig()
        let defaults = UserDefaults.standard
        let oldSettings = defaults.object(forKey: "ExportSettings")
        let oldPreset = defaults.object(forKey: "ExportPresetName")
        defer {
            restore(oldSettings, key: "ExportSettings", in: defaults)
            restore(oldPreset, key: "ExportPresetName", in: defaults)
        }

        let export = ExportController()
        let target = ExportController.Target(
            kind: .selection, title: "One photo", target: .images(imageIds: [rig.imageID]),
            count: 1, firstName: "photo.jpg", firstDate: Date(timeIntervalSince1970: 0))
        export.prepare(engine: rig.library.engine, targets: [target], preferred: .selection)
        export.settings = ExportSettings()
        export.settings.destination = rig.output.path
        export.acquireSaveGate = { ids in
            rig.coordinator.reserveInitiate(owner: rig.library, imageIDs: ids)
        }
        var finishCount = 0
        var failureCount = 0
        export.onFinish = { _, _ in finishCount += 1 }
        export.onFailure = { _ in failureCount += 1 }

        export.start()
        XCTAssertTrue(export.isRunning, "start must synchronously reserve before scheduling work")
        XCTAssertTrue(rig.coordinator.hasActiveReservations)
        export.cancel() // Same MainActor turn: the Task above has not run.

        try await waitUntil { !export.isRunning && !rig.coordinator.hasActiveReservations }
        XCTAssertEqual(export.error, "Export cancelled")
        XCTAssertNil(export.progress)
        XCTAssertNil(export.lastReport)
        XCTAssertEqual(finishCount, 0)
        XCTAssertEqual(failureCount, 0)
        XCTAssertTrue(try FileManager.default.contentsOfDirectory(atPath: rig.output.path).isEmpty)
        XCTAssertTrue(rig.coordinator.isUnreservedForHostMutation(owner: rig.library,
                                                                   imageID: rig.imageID))
    }

    func testPrintPDFCancelBeforeTaskStartsCompletesOnceAndReleasesReservation() async throws {
        let rig = try makeRig()
        let defaults = UserDefaults.standard
        let oldPrint = defaults.object(forKey: PrintSettings.defaultsKey)
        defer { restore(oldPrint, key: PrintSettings.defaultsKey, in: defaults) }

        let printing = PrintController()
        // A synthetic item gives Print a nonempty layout without starting an
        // EngineImageReference thumbnail read during prepare. The real library
        // remains the owner of the saved-read reservation and output Engine.
        let item = PhotoItem(id: 0, url: nil, name: "print-test", kind: .synthetic,
                             captureDate: Date(timeIntervalSince1970: 0),
                             pixelWidth: 8, pixelHeight: 8)
        let loader = ThumbnailLoader()
        printing.prepare(items: [item], title: "One page", loader: loader)
        printing.settings = PrintSettings()
        printing.settings.colorHandling = .printer
        printing.acquireSaveGate = { _ in
            rig.coordinator.reserveInitiate(owner: rig.library, imageIDs: [rig.imageID])
        }
        let pdf = rig.output.appendingPathComponent("cancelled.pdf")
        var completions: [Bool] = []

        printing.run(.pdf(pdf), engine: rig.library.engine, window: nil) { ok in
            completions.append(ok)
        }
        XCTAssertTrue(printing.isRunning)
        XCTAssertTrue(rig.coordinator.hasActiveReservations)
        printing.cancel() // Same MainActor turn: no renderForPrint task has started.

        try await waitUntil { !printing.isRunning && !rig.coordinator.hasActiveReservations }
        XCTAssertEqual(printing.error, "Printing cancelled")
        XCTAssertNil(printing.progress)
        XCTAssertEqual(completions, [false], "Cancellation settles the public callback once")
        XCTAssertFalse(FileManager.default.fileExists(atPath: pdf.path))
        XCTAssertTrue(try FileManager.default.contentsOfDirectory(atPath: rig.output.path).isEmpty)
        XCTAssertTrue(rig.coordinator.isUnreservedForHostMutation(owner: rig.library,
                                                                   imageID: rig.imageID))
    }

    private struct Rig {
        let library: EngineLibrary
        let imageID: String
        let output: URL
        let coordinator: DevelopRecoveryCoordinator
    }

    private func makeRig() throws -> Rig {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("output-admission-\(UUID().uuidString)", isDirectory: true)
        let photos = root.appendingPathComponent("photos", isDirectory: true)
        let support = root.appendingPathComponent("support", isDirectory: true)
        let output = root.appendingPathComponent("output", isDirectory: true)
        try FileManager.default.createDirectory(at: photos, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: root) }
        try writeTinyJPEG(photos.appendingPathComponent("photo.jpg"))
        let library = try EngineLibrary.scan(folder: photos, appSupport: support)
        let imageID = try XCTUnwrap(library.items.first?.engineImage?.imageID)
        return Rig(library: library, imageID: imageID, output: output,
                   coordinator: DevelopRecoveryCoordinator())
    }

    private func writeTinyJPEG(_ url: URL) throws {
        let bytes = Data([90, 110, 130, 255, 90, 110, 130, 255,
                          90, 110, 130, 255, 90, 110, 130, 255])
        let provider = try XCTUnwrap(CGDataProvider(data: bytes as CFData))
        let image = try XCTUnwrap(CGImage(width: 2, height: 2, bitsPerComponent: 8,
            bitsPerPixel: 32, bytesPerRow: 8, space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
            provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent))
        let destination = try XCTUnwrap(CGImageDestinationCreateWithURL(
            url as CFURL, UTType.jpeg.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(destination, image, nil)
        XCTAssertTrue(CGImageDestinationFinalize(destination))
    }

    private func waitUntil(_ condition: @MainActor () -> Bool) async throws {
        for _ in 0..<400 {
            if condition() { return }
            try await Task.sleep(nanoseconds: 5_000_000)
        }
        XCTFail("Output admission did not settle within two seconds")
    }

    private func restore(_ object: Any?, key: String, in defaults: UserDefaults) {
        if let object { defaults.set(object, forKey: key) }
        else { defaults.removeObject(forKey: key) }
    }
}
