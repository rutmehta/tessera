import Foundation
import XCTest
import TesseraFFI
@testable import TesseraCore
@testable import Tessera

/// UX-16 (item 32): the Edits list is a history. After Original every step stays listed and any
/// of them returns the photo to that state; the current one is marked (`isHead`). Through the
/// bridge on the Sony ARW fixture, like `DevelopTests`.
@MainActor
final class DevelopHistoryCheckoutTests: XCTestCase {
    private var root: URL {
        URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    }

    private func scratchRaw() throws -> (folder: URL, support: URL) {
        let fixtures = root.appendingPathComponent("../../fixtures/raw").standardizedFileURL
        let raw = try XCTUnwrap(try FileManager.default.contentsOfDirectory(at: fixtures, includingPropertiesForKeys: nil)
            .first { $0.pathExtension.lowercased() == "arw" }, "fetch fixtures/raw first")
        let temp = root.appendingPathComponent("build/history-test-\(UUID().uuidString)")
        let folder = temp.appendingPathComponent("raw")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: temp) }
        try FileManager.default.copyItem(at: raw, to: folder.appendingPathComponent(raw.lastPathComponent))
        return (folder, temp.appendingPathComponent("support"))
    }

    func testEveryListedStepClicksBackAfterOriginal() async throws {
        let (folder, support) = try scratchRaw()
        let library = try EngineLibrary.scan(folder: folder, appSupport: support)
        let item = try XCTUnwrap(library.items.first)
        let controller = try await DevelopController.open(try XCTUnwrap(item.engineImage), itemID: item.id)
        defer { Task { _ = await controller.close() } }
        let values: [Double] = [0.3, 0.6, 0.9]
        for v in values {
            controller.set(.exposure, v, interactive: false)
            XCTAssertTrue(controller.commit(label: String(format: "Exposure +%.2f", v)))
        }
        var items = controller.historyItems()
        XCTAssertEqual(items.count, 3)
        XCTAssertTrue(items.allSatisfy(\.applied))
        XCTAssertTrue(items.last?.isHead == true, "the newest step is the current one")
        XCTAssertEqual(EditsReadout.text(listed: items.count, applied: items.filter(\.applied).count), "3 steps")

        // Original: nothing applied, everything still listed, no row marked (the Original row is).
        XCTAssertTrue(try controller.checkoutHistory(nil))
        XCTAssertEqual(controller.value(.exposure), 0)
        items = controller.historyItems()
        XCTAssertEqual(items.count, 3, "the steps stay listed after Original")
        XCTAssertTrue(items.allSatisfy { !$0.applied && !$0.isHead })
        XCTAssertEqual(EditsReadout.text(listed: items.count, applied: items.filter(\.applied).count), "Original · 3 steps")

        // The middle step clicks back: it and the first are applied, it is the head, the third
        // stays listed as the redo path.
        let middle = items[1]
        XCTAssertTrue(try controller.checkoutHistory(middle.id))
        XCTAssertEqual(controller.value(.exposure), 0.6, accuracy: 0.001)
        items = controller.historyItems()
        XCTAssertEqual(items.map(\.applied), [true, true, false])
        XCTAssertEqual(items.map(\.isHead), [false, true, false])
        XCTAssertEqual(EditsReadout.text(listed: items.count, applied: items.filter(\.applied).count), "at 2 of 3 steps")

        // And the last step again, from there.
        XCTAssertTrue(try controller.checkoutHistory(items[2].id))
        XCTAssertEqual(controller.value(.exposure), 0.9, accuracy: 0.001)
        items = controller.historyItems()
        XCTAssertEqual(items.map(\.isHead), [false, false, true])
        XCTAssertTrue(items.allSatisfy(\.applied))
        XCTAssertFalse(try controller.checkoutHistory(items[2].id), "the current step is a no-op")
    }

    /// The same path the Edits rows take: `DevelopTools.checkout` through `AppModel`.
    func testToolsCheckoutMovesTheSessionAndRefreshesTheList() async throws {
        let (folder, support) = try scratchRaw()
        let model = AppModel()
        model.install(try EngineLibrary.scan(folder: folder, appSupport: support))
        model.select(position: 0)
        model.enterPhotoEdit()
        model.openDevelop(for: try XCTUnwrap(model.focusedItem))
        let deadline = Date().addingTimeInterval(60)
        while model.developStatus != .ready, Date() < deadline { try await Task.sleep(for: .milliseconds(20)) }
        XCTAssertEqual(model.developStatus, .ready)
        let controller = try XCTUnwrap(model.develop)
        defer { model.returnToLibrary() }
        let tools = DevelopTools(model: model)
        for v in [0.3, 0.6] {
            controller.set(.exposure, v, interactive: false)
            XCTAssertTrue(controller.commit(label: String(format: "Exposure +%.2f", v)))
        }
        tools.refreshHistory()
        XCTAssertEqual(tools.historyItems.count, 2)
        tools.checkout(nil)
        XCTAssertEqual(controller.value(.exposure), 0)
        XCTAssertEqual(tools.historyItems.count, 2, "still listed")
        XCTAssertTrue(tools.historyItems.allSatisfy { !$0.applied })
        let first = try XCTUnwrap(tools.historyItems.first)
        tools.checkout(first.id)
        XCTAssertEqual(controller.value(.exposure), 0.3, accuracy: 0.001, "a listed step clicks back after Original")
        XCTAssertEqual(tools.historyItems.map(\.isHead), [true, false])
        XCTAssertEqual(model.statusMessage, "History: Exposure +0.30")
    }
}
