import XCTest
import TesseraFFI
@testable import TesseraCore

/// UX-15 (item 24): an engine error shown anywhere is its message, never the enum's reflection
/// (`TesseraFFI.BridgeError.Failure(message: "…")`, which Library ▸ Smart Previews printed).
final class BridgeErrorDescriptionTests: XCTestCase {
    func testLocalizedDescriptionIsTheMessage() {
        let failure: Error = BridgeError.Failure(message: "No Smart Preview for this photo")
        XCTAssertEqual(failure.localizedDescription, "No Smart Preview for this photo")
        let busy: Error = BridgeError.Busy(message: "the catalog is busy")
        XCTAssertEqual(busy.localizedDescription, "the catalog is busy")
        XCTAssertEqual((failure as? LocalizedError)?.errorDescription, "No Smart Preview for this photo")
        XCTAssertFalse(failure.localizedDescription.contains("BridgeError"))
        XCTAssertFalse(failure.localizedDescription.contains("message:"))
    }

    func testBusyIsStillRecognised() {
        XCTAssertTrue(EngineBusy.isBusy(BridgeError.Busy(message: "busy")))
        XCTAssertTrue(EngineCall.isBusy(BridgeError.Busy(message: "the queue is full; try again")))
        XCTAssertFalse(EngineCall.isBusy(BridgeError.Failure(message: "no such image")))
    }

    func testNoSurfaceInterpolatesAnEngineErrorValue() throws {
        // Menus, toasts and sheets show `localizedDescription`; `"\(error)"` of a BridgeError would
        // print the case name again.
        let sources = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent().appendingPathComponent("Sources")
        let files = try XCTUnwrap(FileManager.default.enumerator(at: sources, includingPropertiesForKeys: nil))
            .compactMap { $0 as? URL }.filter { $0.pathExtension == "swift" && !$0.path.contains("/TesseraFFI/") }
        let shown = try NSRegularExpression(pattern: #"(statusMessage|showToast|selectionError|Text)\s*[(=][^\n]*\\\((error|err)\)"#)
        var offenders: [String] = []
        for file in files {
            let text = try String(contentsOf: file, encoding: .utf8)
            if shown.firstMatch(in: text, range: NSRange(text.startIndex..., in: text)) != nil {
                offenders.append(file.lastPathComponent)
            }
        }
        XCTAssertEqual(offenders, [], "show error.localizedDescription, not the error value")
    }
}
