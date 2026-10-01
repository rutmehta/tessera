import Foundation
import XCTest
import TesseraFFI
@testable import TesseraCore

/// Persistent channels (WP B5-08): the Channels panel model from records, the Save / Load Selection
/// sheet operation mapping, Quick Mask enter / exit to the selection (stub and engine), and the engine
/// adapter's record conversion and history.
final class DocumentChannelsTests: XCTestCase {
    private func channel(_ id: UInt64, _ name: String, _ kind: SavedChannelKind = .alpha, index: UInt32,
                         visible: Bool = false, color: ToolColor = ToolColor(r: 1, g: 0, b: 0), opacity: Float = 0.5,
                         selectedAreas: Bool = false) -> SavedChannel {
        SavedChannel(id: id, kind: kind, name: name, color: color, opacity: opacity, selectedAreas: selectedAreas,
                     visible: visible, index: index, revision: id * 10)
    }

    // MARK: Panel model

    func testPanelRowsFromRecords() {
        let ink = ToolColor(r: 0, g: 0.5, b: 1)
        let styled = ChannelOverlayStyle(color: ToolColor(r: 0, g: 1, b: 0), opacity: 0.3, indicatesSelected: true)
        let records = [channel(7, "Spot", .spot, index: 1, visible: true, color: ink),
                       channel(3, "Alpha 1", index: 0, color: styled.color, opacity: 0.3, selectedAreas: true),
                       channel(9, "Quick Mask", index: 2, visible: true)]
        var comps = ComponentVisibility()
        comps.green = false
        let rows = ChannelsPanelModel.rows(records: records, components: comps, quickMask: 9)
        XCTAssertEqual(rows.map(\.title), ["RGB", "Red", "Green", "Blue", "Alpha 1", "Spot", "Quick Mask"])
        XCTAssertEqual(rows.map(\.id), ["rgb", "component.0", "component.1", "component.2", "channel.3", "channel.7", "channel.9"])
        XCTAssertEqual(rows.map(\.kind), [.composite, .component(0), .component(1), .component(2), .alpha, .spot, .alpha])
        XCTAssertEqual(rows.map(\.visible), [false, true, false, true, false, true, true], "RGB is on only when every component is")
        XCTAssertEqual(rows.map(\.editable), [false, false, false, false, true, true, true])
        XCTAssertEqual(rows.map(\.isQuickMask), [false, false, false, false, false, false, true])
        XCTAssertEqual(rows[4].color, styled.color, "alpha rows show the saved overlay colour")
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

        // B5-17d: entering moves the selection into the mask and drops it.
        let q = try QuickMask.enter(b)
        let rows = try b.documentChannels()
        XCTAssertEqual(rows.map(\.name), [QuickMask.channelName])
        XCTAssertEqual(rows.first?.id, q.channelID)
        XCTAssertEqual(rows.first?.visible, true, "the overlay shows the mask")
        XCTAssertNil(try doc.info().selectionBounds, "entering drops the selection")
        XCTAssertNotNil(try QuickMask.exit(b, channel: q.channelID))
        XCTAssertEqual(try doc.info().selectionBounds, rect, "the mask became the selection")
        XCTAssertTrue(try b.documentChannels().isEmpty, "the temporary channel is gone")
        XCTAssertNil(try QuickMask.exit(b, channel: q.channelID), "a second exit is a no-op")

        // Without a selection Quick Mask starts all selected and returns the whole canvas.
        _ = try doc.clearSelection()
        let all = try QuickMask.enter(b)
        XCTAssertNil(try doc.info().selectionBounds)
        _ = try QuickMask.exit(b, channel: all.channelID)
        XCTAssertEqual(try doc.info().selectionBounds, CanvasRect(x: 0, y: 0, width: 400, height: 300))
    }

    // MARK: Row clicks (B5-17d)

    func testRowClickHighlightMatchesThePaintTarget() throws {
        let records = [SavedChannel(id: 7, kind: .alpha, name: "Alpha 1", color: ToolColor(r: 1, g: 0, b: 0),
                                    opacity: 0.5, visible: false, index: 0, revision: 1)]
        let rows = ChannelsPanelModel.rows(records: records, components: ComponentVisibility(), quickMask: nil)
        let alpha = rows[4], rgb = rows[0], red = rows[1]
        // A plain click on an alpha row highlights it and paints into it.
        XCTAssertEqual(ChannelsPanelModel.click(alpha, command: false),
                       ChannelRowClick(highlight: 7, retarget: true, load: nil))
        // RGB or a colour row: painting returns to the layer and the highlight is cleared.
        for row in [rgb, red] {
            let c = try XCTUnwrap(ChannelsPanelModel.click(row, command: false))
            XCTAssertEqual(c, ChannelRowClick(highlight: nil, retarget: true, load: nil), row.title)
            XCTAssertEqual(c.highlight, c.paintTarget, "the highlight matches the paint target")
        }
        XCTAssertEqual(ChannelsPanelModel.click(alpha, command: false)?.paintTarget, 7)
        // ⌘-click (Photoshop: ⌘-click a channel thumbnail) loads the channel as the selection and leaves
        // the highlight and the paint target alone, so the highlight still shows where paint goes (B5-23).
        let load = try XCTUnwrap(ChannelsPanelModel.click(alpha, command: true))
        XCTAssertEqual(load, ChannelRowClick(highlight: nil, retarget: false, load: 7))
        XCTAssertFalse(load.retarget, "⌘-click neither re-highlights nor redirects paint")
        XCTAssertNil(load.paintTarget)
        XCTAssertNil(ChannelsPanelModel.click(rgb, command: true))
        XCTAssertNil(ChannelsPanelModel.click(red, command: true))
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

    // MARK: Alpha display settings (B5-17b)

    func testSavedChannelMapsSelectedAreasAndOverlayStyle() {
        let r = ChannelRecord(id: 4, kind: .alpha, name: "Sky", color: PaintColor(r: 0, g: 1, b: 0.25), opacity: 0.3,
                              selectedAreas: true, visible: true, index: 2, revision: 11)
        let c = SavedChannel(r)
        XCTAssertTrue(c.selectedAreas)
        XCTAssertEqual(c.overlayStyle, ChannelOverlayStyle(color: ToolColor(r: 0, g: 1, b: 0.25), opacity: 0.3,
                                                           indicatesSelected: true), "the overlay follows the record")
        let legacy = SavedChannel(ChannelRecord(id: 5, kind: .alpha, name: "Old", color: PaintColor(r: 1, g: 0, b: 0),
                                                opacity: 0.5, selectedAreas: false, visible: false, index: 0, revision: 1))
        XCTAssertEqual(legacy.overlayStyle, .alphaDefault)
        XCTAssertFalse(channel(6, "Ink", .spot, index: 0, selectedAreas: true).overlayStyle.indicatesSelected,
                       "spot ink never inverts")
    }

    func testChannelOptionsFormPlansOneEdit() {
        let alpha = channel(3, "Alpha 1", index: 0)
        var f = ChannelOptionsForm(alpha)
        XCTAssertEqual(f.indicates, .maskedAreas)
        XCTAssertEqual(ChannelIndicates.allCases.map(\.title), ["Masked Areas", "Selected Areas", "Spot Color"])
        XCTAssertNil(f.displayEdit(from: alpha), "unchanged: no call")
        f.indicates = .selectedAreas
        f.color = ToolColor(r: 0, g: 1, b: 0)
        f.opacity = 0.3
        XCTAssertEqual(f.displayEdit(from: alpha), .alpha(color: ToolColor(r: 0, g: 1, b: 0), opacity: 0.3, selectedAreas: true))
        f.indicates = .spotColor
        XCTAssertEqual(f.displayEdit(from: alpha), .spot(color: ToolColor(r: 0, g: 1, b: 0), solidity: 0.3))
        let spot = channel(7, "Spot", .spot, index: 1, color: ToolColor(r: 0, g: 0.5, b: 1), opacity: 0.8)
        var g = ChannelOptionsForm(spot)
        XCTAssertEqual(g.indicates, .spotColor)
        XCTAssertNil(g.displayEdit(from: spot))
        g.indicates = .maskedAreas
        XCTAssertEqual(g.displayEdit(from: spot), .alpha(color: ToolColor(r: 0, g: 0.5, b: 1), opacity: 0.8, selectedAreas: false),
                       "a spot channel becomes an alpha channel")
        var h = ChannelOptionsForm(alpha)
        h.setColor(ToolColor(r: 0.99995, g: 0.00003, b: 0))
        h.setOpacity(0.50001)
        XCTAssertNil(h.displayEdit(from: alpha), "colour-well round-trip noise is not an edit")
        h.setOpacity(0.49)
        XCTAssertEqual(h.displayEdit(from: alpha), .alpha(color: alpha.color, opacity: 0.49, selectedAreas: false))
        let selected = channel(8, "Sel", index: 2, selectedAreas: true)
        XCTAssertEqual(ChannelOptionsForm(selected).indicates, .selectedAreas)
    }

    func testStubAlphaDisplayRoundTrip() throws {
        let doc = try StubDocumentEngine.shared.newDocument(width: 400, height: 300, depth: .u8, profile: nil)
        defer { doc.close() }
        let b = try XCTUnwrap(doc as? DocumentChannelsBackend)
        let a = try b.newAlphaChannel(name: "A", selected: false).channelID
        XCTAssertEqual(try b.documentChannels().first?.overlayStyle, .alphaDefault)
        XCTAssertThrowsError(try b.setAlphaChannelDisplay(id: a, color: ToolColor(r: 0, g: 1, b: 0), opacity: 1.5,
                                                          selectedAreas: true))
        _ = try b.setAlphaChannelDisplay(id: a, color: ToolColor(r: 0, g: 1, b: 0), opacity: 0.3, selectedAreas: true)
        let r = try XCTUnwrap(try b.documentChannels().first)
        XCTAssertEqual(r.kind, .alpha)
        XCTAssertEqual(r.overlayStyle, ChannelOverlayStyle(color: ToolColor(r: 0, g: 1, b: 0), opacity: 0.3, indicatesSelected: true))
        _ = try b.setSpotChannel(id: a, color: ToolColor(r: 0, g: 0, b: 1), solidity: 0.5)
        XCTAssertFalse(try XCTUnwrap(try b.documentChannels().first).selectedAreas)
        _ = try ChannelDisplayEdit.alpha(color: ToolColor(r: 1, g: 0, b: 0), opacity: 0.5, selectedAreas: false).apply(b, id: a)
        XCTAssertEqual(try b.documentChannels().first?.kind, .alpha)
        XCTAssertThrowsError(try b.setAlphaChannelDisplay(id: 999, color: ToolColor(r: 1, g: 0, b: 0), opacity: 0.5,
                                                          selectedAreas: false))
    }

    func testEngineAlphaDisplayIsOneNodeAndSurvivesReopen() throws {
        let dir = try temp()
        let e = try Engine.open(appSupportDir: dir.appendingPathComponent("support").path)
        let engine = EngineDocumentEngine.for(e)
        for ext in ["tessera-doc", "psd"] {
            let doc = try engine.newDocument(width: 128, height: 64, depth: .u8, profile: nil)
            let b = try XCTUnwrap(doc as? DocumentChannelsBackend)
            let a = try b.newAlphaChannel(name: "Sky", selected: false).channelID
            let old = try XCTUnwrap(try b.documentChannels().first)
            var f = ChannelOptionsForm(old)
            f.indicates = .selectedAreas
            f.color = ToolColor(r: 0, g: 1, b: 0)
            f.opacity = 0.3
            let n = try doc.historyItems().count
            let edit = try XCTUnwrap(f.displayEdit(from: old))
            let change = try edit.apply(b, id: a)
            XCTAssertEqual(try doc.historyItems().count, n + 1, "one history node")
            XCTAssertEqual(try doc.historyItems().last?.label, "Channel Options")
            XCTAssertEqual(change.historyHead, try doc.info().historyHead)
            let want = ChannelOverlayStyle(color: ToolColor(r: 0, g: 1, b: 0), opacity: 0.3, indicatesSelected: true)
            XCTAssertEqual(try b.documentChannels().first?.overlayStyle, want)
            _ = try doc.undo()
            XCTAssertEqual(try b.documentChannels().first?.overlayStyle, .alphaDefault)
            _ = try doc.redo()
            XCTAssertEqual(try b.documentChannels().first?.overlayStyle, want)

            let url = dir.appendingPathComponent("display.\(ext)")
            try doc.saveAs(path: url.path)
            doc.close()
            let re = try engine.openDocument(path: url.path)
            defer { re.close() }
            let rb = try XCTUnwrap(re as? DocumentChannelsBackend)
            let r = try XCTUnwrap(try rb.documentChannels().first)
            XCTAssertEqual(r.name, "Sky", ext)
            XCTAssertTrue(r.selectedAreas, ext)
            XCTAssertEqual(r.color.g, 1, accuracy: 1e-3, ext)
            XCTAssertEqual(r.opacity, 0.3, accuracy: 6e-3, ext)
        }
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

        // Quick Mask round trip through the engine (B5-17d): one "Quick Mask" node in, one out; entering
        // drops the selection, exiting restores it.
        let n0 = try doc.historyItems().count
        let q = try QuickMask.enter(b)
        XCTAssertEqual(try b.documentChannels().count, 2)
        XCTAssertNil(try doc.info().selectionBounds, "entering drops the selection")
        XCTAssertEqual(try doc.historyItems().count, n0 + 1)
        XCTAssertEqual(try doc.historyItems().last?.label, "Quick Mask")
        XCTAssertEqual(q.change.historyHead, try doc.info().historyHead)
        _ = try QuickMask.exit(b, channel: q.channelID)
        XCTAssertEqual(try doc.historyItems().count, n0 + 2)
        XCTAssertEqual(try doc.historyItems().last?.label, "Quick Mask")
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
