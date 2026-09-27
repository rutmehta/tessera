import Foundation
import XCTest
import TesseraFFI
@testable import TesseraCore

/// Persistent channels (WP B5-08): the Channels panel model from records, the Save / Load Selection
/// sheet operation mapping, Quick Mask enter / exit to the selection (stub and engine), and the engine
/// adapter's record conversion and history.
final class DocumentChannelsTests: XCTestCase {
    private func channel(_ id: UInt64, _ name: String, _ kind: SavedChannelKind = .alpha, index: UInt32,
                         visible: Bool = false, color: ToolColor = ToolColor(r: 1, g: 0, b: 0)) -> SavedChannel {
        SavedChannel(id: id, kind: kind, name: name, color: color, opacity: 0.5, visible: visible, index: index, revision: id * 10)
    }

    // MARK: Panel model

    func testPanelRowsFromRecords() {
        let ink = ToolColor(r: 0, g: 0.5, b: 1)
        let records = [channel(7, "Spot", .spot, index: 1, visible: true, color: ink), channel(3, "Alpha 1", index: 0),
                       channel(9, "Quick Mask", index: 2, visible: true)]
        var comps = ComponentVisibility()
        comps.green = false
        let styled = ChannelOverlayStyle(color: ToolColor(r: 0, g: 1, b: 0), opacity: 0.3, indicatesSelected: true)
        let rows = ChannelsPanelModel.rows(records: records, components: comps, quickMask: 9, styles: [3: styled])
        XCTAssertEqual(rows.map(\.title), ["RGB", "Red", "Green", "Blue", "Alpha 1", "Spot", "Quick Mask"])
        XCTAssertEqual(rows.map(\.id), ["rgb", "component.0", "component.1", "component.2", "channel.3", "channel.7", "channel.9"])
        XCTAssertEqual(rows.map(\.kind), [.composite, .component(0), .component(1), .component(2), .alpha, .spot, .alpha])
        XCTAssertEqual(rows.map(\.visible), [false, true, false, true, false, true, true], "RGB is on only when every component is")
        XCTAssertEqual(rows.map(\.editable), [false, false, false, false, true, true, true])
        XCTAssertEqual(rows.map(\.isQuickMask), [false, false, false, false, false, false, true])
        XCTAssertEqual(rows[4].color, styled.color, "alpha rows show the session overlay colour")
        XCTAssertEqual(rows[5].color, ink, "spot rows show the ink")
        XCTAssertEqual(rows[6].color, ChannelOverlayStyle.alphaDefault.color)
        XCTAssertEqual(rows[5].revision, 70)
        XCTAssertEqual(rows[4].channelID, 3)
        XCTAssertNil(rows[0].channelID)
    }

    func testComponentVisibilityAndNames() {
        var v = ComponentVisibility()
        XCTAssertTrue(v.all)
        v[2] = false
        XCTAssertFalse(v.all)
        XCTAssertFalse(v.none)
        v.setComposite(false)
        XCTAssertTrue(v.none)
        v.setComposite(true)
        XCTAssertTrue(v.all)
        let records = [channel(1, "Alpha 1", index: 0), channel(2, "Alpha 3", index: 1)]
        XCTAssertEqual(ChannelsPanelModel.nextName("Alpha", existing: records), "Alpha 2")
        XCTAssertEqual(ChannelsPanelModel.nextName("Spot Color", existing: records), "Spot Color 1")
    }

    // MARK: Sheet operation mapping

    func testSaveSelectionFormMapping() {
        var f = SaveSelectionForm(name: "  Mask  ")
        XCTAssertEqual(f.operations, [.replace])
        XCTAssertEqual(SaveSelectionForm.title(.replace, newChannel: true), "New Channel")
        let r = f.request
        XCTAssertEqual(r?.name, "Mask")
        XCTAssertNil(r?.target)
        XCTAssertEqual(r?.op, .replace)
        f.operation = .intersect
        XCTAssertEqual(f.request?.op, .replace, "a new channel only takes New Channel")
        f.name = " "
        XCTAssertNil(f.request, "a new channel needs a name")

        f.destination = 4
        XCTAssertEqual(f.operations, [.replace, .add, .subtract, .intersect])
        XCTAssertEqual(f.operations.map { SaveSelectionForm.title($0, newChannel: false) },
                       ["Replace Channel", "Add to Channel", "Subtract from Channel", "Intersect with Channel"])
        for op in SelectionCombine.allCases {
            f.operation = op
            XCTAssertEqual(f.request?.target, 4)
            XCTAssertEqual(f.request?.op, op)
        }
    }

    func testLoadSelectionFormMapping() {
        var f = LoadSelectionForm(channel: nil, hasSelection: false)
        XCTAssertNil(f.request)
        f.channel = 5
        f.operation = .subtract
        XCTAssertEqual(f.operations, [.replace])
        XCTAssertEqual(f.request?.op, .replace, "without a selection only New Selection applies")
        f.hasSelection = true
        XCTAssertEqual(f.operations.map(\.title), ["New Selection", "Add to Selection", "Subtract from Selection",
                                                   "Intersect with Selection"])
        for op in SelectionCombine.allCases {
            f.operation = op
            f.invert = op == .add
            XCTAssertEqual(f.request?.id, 5)
            XCTAssertEqual(f.request?.op, op)
            XCTAssertEqual(f.request?.invert, op == .add)
        }
    }

    // MARK: Quick Mask and the stub

    func testQuickMaskEnterExitOnTheStub() throws {
        let doc = try StubDocumentEngine.shared.newDocument(width: 400, height: 300, depth: .u8, profile: nil)
        defer { doc.close() }
        let b = try XCTUnwrap(doc as? DocumentChannelsBackend)
        let rect = CanvasRect(x: 10, y: 20, width: 100, height: 50)
        _ = try doc.setSelectionRect(x: rect.x, y: rect.y, width: rect.width, height: rect.height, feather: 0)

        let q = try QuickMask.enter(b, hasSelection: true)
        let rows = try b.documentChannels()
        XCTAssertEqual(rows.map(\.name), [QuickMask.channelName])
        XCTAssertEqual(rows.first?.id, q.channelID)
        XCTAssertEqual(rows.first?.visible, true, "the overlay shows the mask")
        _ = try doc.clearSelection()
        XCTAssertNotNil(try QuickMask.exit(b, channel: q.channelID))
        XCTAssertEqual(try doc.info().selectionBounds, rect, "the mask became the selection")
        XCTAssertTrue(try b.documentChannels().isEmpty, "the temporary channel is gone")
        XCTAssertNil(try QuickMask.exit(b, channel: q.channelID), "a second exit is a no-op")

        // Without a selection Quick Mask starts all selected and returns the whole canvas.
        _ = try doc.clearSelection()
        let all = try QuickMask.enter(b, hasSelection: false)
        _ = try QuickMask.exit(b, channel: all.channelID)
        XCTAssertEqual(try doc.info().selectionBounds, CanvasRect(x: 0, y: 0, width: 400, height: 300))
    }

    func testStubChannelsSaveLoadAndEdit() throws {
        let doc = try StubDocumentEngine.shared.newDocument(width: 400, height: 300, depth: .u8, profile: nil)
        defer { doc.close() }
        let b = try XCTUnwrap(doc as? DocumentChannelsBackend)
        XCTAssertThrowsError(try b.saveSelectionChannel(name: "A", target: nil, op: .replace), "no selection")
        _ = try doc.setSelectionRect(x: 0, y: 0, width: 100, height: 100, feather: 0)
        let a = try b.saveSelectionChannel(name: "A", target: nil, op: .replace).channelID
        let dup = try b.duplicateDocumentChannel(id: a).channelID
        XCTAssertEqual(try b.documentChannels().map(\.name), ["A", "A copy"])
        _ = try doc.setSelectionRect(x: 50, y: 0, width: 100, height: 100, feather: 0)
        _ = try b.saveSelectionChannel(name: "", target: a, op: .add)
        _ = try doc.clearSelection()
        _ = try b.loadSelectionChannel(id: a, op: .replace, invert: false)
        XCTAssertEqual(try doc.info().selectionBounds, CanvasRect(x: 0, y: 0, width: 150, height: 100))
        _ = try b.loadSelectionChannel(id: dup, op: .intersect, invert: false)
        XCTAssertEqual(try doc.info().selectionBounds, CanvasRect(x: 0, y: 0, width: 100, height: 100))
        _ = try b.renameDocumentChannel(id: dup, name: "Copy")
        XCTAssertThrowsError(try b.renameDocumentChannel(id: dup, name: "  "))
        XCTAssertThrowsError(try b.setSpotChannel(id: dup, color: ToolColor(r: 2, g: 0, b: 0), solidity: 0.5))
        _ = try b.setSpotChannel(id: dup, color: ToolColor(r: 0, g: 1, b: 0), solidity: 0.25)
        let spot = try XCTUnwrap(try b.documentChannels().first { $0.id == dup })
        XCTAssertEqual(spot.kind, .spot)
        XCTAssertEqual(spot.opacity, 0.25)
        _ = try b.deleteDocumentChannel(id: a)
        XCTAssertThrowsError(try b.loadSelectionChannel(id: a, op: .replace, invert: false), "stale id")
        XCTAssertEqual(try b.documentChannels().map(\.name), ["Copy"])
        XCTAssertEqual(try b.documentChannels().map(\.index), [0])
    }

    // MARK: Engine adapter

    private func temp() throws -> URL {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("doc-channels-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        return dir
    }

    func testEngineChannelsThroughTheAdapter() throws {
        let dir = try temp()
        let e = try Engine.open(appSupportDir: dir.appendingPathComponent("support").path)
        let doc = try EngineDocumentEngine.for(e).newDocument(width: 256, height: 128, depth: .u8, profile: nil)
        defer { doc.close() }
        let b = try XCTUnwrap(doc as? DocumentChannelsBackend)
        let rect = CanvasRect(x: 16, y: 8, width: 64, height: 32)
        _ = try doc.setSelectionRect(x: rect.x, y: rect.y, width: rect.width, height: rect.height, feather: 0)
        let before = try doc.historyItems().count
        let a = try b.saveSelectionChannel(name: "Alpha 1", target: nil, op: .replace)
        XCTAssertEqual(try doc.historyItems().count, before + 1)
        XCTAssertEqual(try doc.historyItems().last?.label, "Save Selection")
        XCTAssertEqual(a.change.historyHead, try doc.info().historyHead)
        let rows = try b.documentChannels()
        XCTAssertEqual(rows.map(\.name), ["Alpha 1"])
        XCTAssertEqual(rows.first?.kind, .alpha)
        XCTAssertEqual(rows.first?.id, a.channelID)
        XCTAssertNotEqual(try b.channelThumbnail(id: a.channelID, maxPx: 32), 0)

        // Quick Mask round trip through the engine: three history nodes, the selection restored.
        let q = try QuickMask.enter(b, hasSelection: true)
        XCTAssertEqual(try b.documentChannels().count, 2)
        _ = try doc.clearSelection()
        _ = try QuickMask.exit(b, channel: q.channelID)
        XCTAssertEqual(try doc.info().selectionBounds, rect)
        XCTAssertEqual(try b.documentChannels().map(\.id), [a.channelID])

        // Spot channel, invalid metadata rejected, undo removes it.
        let s = try b.newSpotChannel(name: "Varnish", color: ToolColor(r: 1, g: 0.8, b: 0), solidity: 0.4, fromSelection: true)
        let spot = try XCTUnwrap(try b.documentChannels().first { $0.id == s.channelID })
        XCTAssertEqual(spot.kind, .spot)
        XCTAssertEqual(spot.opacity, 0.4, accuracy: 1e-6)
        XCTAssertThrowsError(try b.setSpotChannel(id: s.channelID, color: ToolColor(r: 0, g: 0, b: 0), solidity: 1.5))
        _ = try doc.undo()
        XCTAssertNil(try b.documentChannels().first { $0.id == s.channelID })

        // Load with invert into a new selection.
        _ = try b.loadSelectionChannel(id: a.channelID, op: .replace, invert: true)
        XCTAssertEqual(try doc.info().selectionBounds, CanvasRect(x: 0, y: 0, width: 256, height: 128))
        try b.setChannelVisible(id: a.channelID, visible: true)
        XCTAssertEqual(try b.documentChannels().first?.visible, true)
        XCTAssertThrowsError(try b.deleteDocumentChannel(id: 999))
    }
}
