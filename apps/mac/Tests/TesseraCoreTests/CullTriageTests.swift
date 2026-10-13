import AppKit
import XCTest
@testable import Tessera
@testable import TesseraCore

/// WP UX-14: the agent's triage (SPEC §4) from groups, picks and signals; the Cull agent field's
/// phrases; the burst-first navigation and quick actions on the stub library; the grouped grid.
final class CullTriageTests: XCTestCase {
    private func sig(_ sharp: Double, eyes: Double? = nil, quality: Double? = nil, blur: Double? = nil) -> FrameSignals {
        FrameSignals(sharpness: sharp, motionBlur: blur, eyesOpen: eyes, quality: quality)
    }

    func testBucketsFollowThePickAndTheDefects() {
        // Two bursts and two singles; the pick of the second burst has its eyes closed.
        let groups = [0..<3, 3..<5, 5..<6, 6..<7]
        let best = [1, 3, 5, 6]
        let signals: [Int: FrameSignals] = [
            0: sig(0.6), 1: sig(0.9, eyes: 0.9), 2: sig(0.2),
            3: sig(0.8, eyes: 0.1), 4: sig(0.7, eyes: 0.9),
            5: sig(0.8, quality: 0.8), 6: sig(0.5, quality: 0.3),
        ]
        let t = CullTriage.compute(groups: groups, best: best, signals: signals)
        XCTAssertEqual(t.burstCount, 2)
        XCTAssertEqual(t.burstItems, [0, 1, 2, 3, 4])
        XCTAssertEqual(t.keepers, [1, 5], "the sharp pick and the well-scored single")
        XCTAssertEqual(t.rejects[2], .blurry)
        XCTAssertEqual(t.rejects[0], .nearDuplicate)
        XCTAssertEqual(t.rejects[3], .eyesClosed, "a defective pick is a reject, not a keeper")
        XCTAssertEqual(t.rejects[4], .nearDuplicate)
        XCTAssertEqual(t.rest, [6])
        XCTAssertEqual(t.count(.rejects), 4)
        XCTAssertEqual(t.count(.blurry), 1)
        XCTAssertEqual(t.measured, 7)
        XCTAssertEqual(t.total, 7)
        XCTAssertEqual(t.items(.rejects), [0, 2, 3, 4])
        XCTAssertEqual(t.items(.eyesClosed), [3])
    }

    func testUnmeasuredFramesWaitForThePass() {
        let t = CullTriage.compute(groups: [0..<2, 2..<3], best: [0, 2], signals: [0: sig(0.9)])
        XCTAssertEqual(t.keepers, [0])
        XCTAssertTrue(t.rejects.isEmpty, "frame 1 is not a near-duplicate until it is measured")
        XCTAssertEqual(t.measured, 1)
        XCTAssertEqual(t.burstCount, 1)
        XCTAssertEqual(CullTriage.compute(groups: [0..<2], best: [0], signals: [:]).burstCount, 1, "bursts count without signals")
    }

    func testMotionBlurCountsAsBlurry() {
        XCTAssertTrue(sig(0.8, blur: 0.9).isBlurry)
        XCTAssertFalse(sig(0.8, blur: 0.4).isBlurry)
        XCTAssertFalse(FrameSignals().isMeasured)
        XCTAssertEqual(FrameSignals(faceSharpness: 0.5, quality: 0.8).technicalScore!, 0.8 * 0.75, accuracy: 1e-12)
    }

    func testPickReasonNamesSharpnessAndEyes() {
        let signals: [Int: FrameSignals] = [0: sig(0.9, eyes: 0.8), 1: sig(0.5), 2: sig(0.7)]
        XCTAssertEqual(CullTriage.pickReason(pick: 0, group: 0..<3, signals: signals), "sharpest of 3, eyes open")
        XCTAssertEqual(CullTriage.pickReason(pick: 2, group: 0..<3, signals: [0: sig(0.9, quality: 0.5), 2: sig(0.7, quality: 0.9)]),
                       "best score of 3")
        XCTAssertNil(CullTriage.pickReason(pick: 1, group: 0..<3, signals: [:]), "nothing measured: no reason yet")
    }

    func testRemapFollowsFramesAndDropsTheGone() {
        var t = CullTriage.compute(groups: [0..<3], best: [1], signals: [0: sig(0.5), 1: sig(0.9), 2: sig(0.1)])
        t = t.remapped { $0 == 2 ? nil : $0 + 10 }
        XCTAssertEqual(t.keepers, [11])
        XCTAssertEqual(t.rejects, [10: .nearDuplicate])
        XCTAssertEqual(t.burstItems, [10, 11])
    }

    func testAgentPhrasesMapToQuickActions() {
        XCTAssertEqual(CullAgentPhrase.parse("Reject the blurry ones", people: []), .rejectBlurry)
        XCTAssertEqual(CullAgentPhrase.parse("keep the best of each burst", people: []), .keepPicks)
        XCTAssertEqual(CullAgentPhrase.parse("where are the closed eyes", people: []), .findClosedEyes)
        XCTAssertEqual(CullAgentPhrase.parse("show me Dhruv", people: ["Dhruv", "Ana Maria"]), .showPerson("Dhruv"))
        XCTAssertEqual(CullAgentPhrase.parse("ana maria laughing", people: ["Dhruv", "Ana Maria"]), .showPerson("Ana Maria"))
        XCTAssertNil(CullAgentPhrase.parse("the ones where everyone is laughing", people: ["Dhruv"]))
    }

    // MARK: Burst-first navigation on the stub library

    @MainActor
    private func burstModel() -> AppModel {
        let model = AppModel()
        model.loadStubItems(count: 40)
        model.cullLayout = .burst
        XCTAssertTrue(model.isCullWorkspace)
        XCTAssertTrue(model.isBurstViewShown)
        return model
    }

    @MainActor
    func testBurstMovesLandOnThePickAndStayInsideTheBurst() throws {
        let model = burstModel()
        let bursts = model.cull.groups.indices.filter { model.cull.groups[$0].count > 1 }
        XCTAssertGreaterThan(bursts.count, 1, "the synthetic shoot has bursts")
        model.select(id: model.cull.groups[bursts[0]].lowerBound)
        // ⌥→ walks to the next burst (singles on the way are landed on too) and lands on its pick.
        var hops = 0
        while model.focusedItem?.groupID != bursts[1], hops < 20 { XCTAssertTrue(model.moveBurst(1)); hops += 1 }
        let burst = try XCTUnwrap(model.focusedBurst)
        XCTAssertEqual(burst.group, bursts[1])
        XCTAssertEqual(model.focusedItem?.id, burst.pick, "⌥→ lands on the pick")
        XCTAssertEqual(model.focusedBurstOrdinal, 2)
        // ← → stay inside the burst.
        let first = burst.frames.lowerBound, last = burst.frames.upperBound - 1
        model.select(id: first)
        model.navigate(.left, groupwise: false, extend: false)
        XCTAssertEqual(model.focusedItem?.id, first)
        XCTAssertEqual(model.statusMessage, "First frame in the burst · ⌥← previous burst")
        model.navigate(.right, groupwise: false, extend: false)
        XCTAssertEqual(model.focusedItem?.id, first + 1)
        model.select(id: last)
        model.navigate(.right, groupwise: false, extend: false)
        XCTAssertEqual(model.focusedItem?.id, last)
        model.select(id: model.cull.groups[0].lowerBound)
        XCTAssertFalse(model.moveBurst(-1), "the first group has no previous one")
    }

    @MainActor
    func testDecisionsAdvanceWithinTheBurstThenToTheNext() throws {
        let model = burstModel()
        // The first burst of 2+ frames.
        let g = try XCTUnwrap(model.cull.groups.firstIndex { $0.count > 1 })
        let frames = model.cull.groups[g]
        model.select(id: frames.lowerBound)
        model.perform(.keep)
        XCTAssertEqual(model.cull[frames.lowerBound].decision, .keep)
        XCTAssertEqual(model.focusedItem?.groupID, g, "the next undecided frame of the same burst")
        XCTAssertEqual(model.cull[try XCTUnwrap(model.focusedItem).id].decision, .undecided)
        for _ in 0..<(frames.count - 1) { model.perform(.reject) }
        XCTAssertTrue(frames.allSatisfy { model.cull[$0].decision != .undecided })
        XCTAssertNotEqual(model.focusedItem?.groupID, g, "every frame decided: the next burst")
        XCTAssertEqual(model.burstsDecided, 1)
    }

    @MainActor
    func testKeepPickInEveryBurstIsOneUndoStep() {
        let model = burstModel()
        model.keepPickInEveryBurst()
        let bursts = model.cull.groups.filter { $0.count > 1 }
        for (g, range) in model.cull.groups.enumerated() where range.count > 1 {
            for id in range {
                XCTAssertEqual(model.cull[id].decision, id == model.cull.bestOfGroup[g] ? .keep : .reject)
            }
        }
        XCTAssertEqual(model.burstsDecided, bursts.count)
        XCTAssertEqual(model.toast?.undoable, true)
        model.undo()
        XCTAssertEqual(model.counts.undecided, model.library.items.count, "⌘Z takes the whole step back")
        model.keepPickInEveryBurst()
        model.keepPickInEveryBurst()
        XCTAssertEqual(model.statusMessage, "Every burst is decided")
    }

    @MainActor
    func testTriageRowsFilterTheViewAndToggleOff() {
        let model = burstModel()
        let all = model.visibleCount
        model.setCullFilter(.bucket(.bursts))
        XCTAssertEqual(model.visibleCount, model.assist.triage.burstItems.count)
        XCTAssertLessThan(model.visibleCount, all)
        model.setCullFilter(.bucket(.bursts))
        XCTAssertNil(model.cullFilter, "the same row again shows the whole shoot")
        XCTAssertEqual(model.visibleCount, all)
        XCTAssertFalse(model.cullAgent("make it warmer"), "unknown phrases are reported, not acted on")
        model.submitAgentInstruction("reject the blurry ones")
        XCTAssertEqual(model.statusMessage, "No blurry frames to reject", "the stub has no signals")
    }

    @MainActor
    func testGTogglesBurstAndGridAndTheGridGroupsByBurst() {
        let model = burstModel()
        let router = KeyRouter(model: model)
        let window = LayoutProbeHarness.window(contentRect: NSRect(x: 0, y: 0, width: 200, height: 100),
                                               styleMask: .titled, backing: .buffered, defer: false)
        let g = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [], timestamp: 0, windowNumber: window.windowNumber,
                                 context: nil, characters: "g", charactersIgnoringModifiers: "g", isARepeat: false, keyCode: 5)!
        XCTAssertTrue(router.handle(g))
        XCTAssertEqual(model.cullLayout, .grid)
        XCTAssertTrue(model.isGroupedGridShown)
        XCTAssertTrue(router.handle(g))
        XCTAssertEqual(model.cullLayout, .burst)
        model.viewMode = .loupe
        XCTAssertTrue(router.handle(g))
        XCTAssertEqual(model.viewMode, .grid, "from the loupe G is still the grid")
        UserDefaults.standard.removeObject(forKey: AppModel.cullLayoutKey)
    }

    @MainActor
    func testGroupedLayoutPutsBurstsInRowsAndSinglesLast() {
        // Positions 0–2 a burst, 3 a single, 4–5 a burst, 6 a single.
        let groups = [0, 0, 0, 1, 2, 2, 3]
        let layout = UniformGridLayout(axis: .vertical)
        layout.targetWidth = 100
        layout.spacing = 10
        layout.inset = NSEdgeInsets(top: 0, left: 0, bottom: 0, right: 0)
        layout.headerHeight = 20
        layout.groupOf = { p in [3, 6].contains(p) ? nil : groups[p] }
        layout.grouped = true
        let collection = NSCollectionView(frame: NSRect(x: 0, y: 0, width: 320, height: 400))
        let scroll = NSScrollView(frame: collection.frame)
        scroll.documentView = collection
        collection.collectionViewLayout = layout
        collection.dataSource = StubSource(count: 7)
        collection.reloadData()
        layout.prepare()
        XCTAssertEqual(layout.columns, 3)
        XCTAssertEqual(layout.blocks.count, 3)
        XCTAssertEqual(layout.blocks.map(\.singles), [false, false, true])
        XCTAssertEqual(layout.blocks[0].count, 3)
        XCTAssertEqual(layout.blocks[1].count, 2)
        XCTAssertEqual(layout.singleSlots, [3, 6])
        // Each burst starts on its own row under a header; the singles share the last block.
        XCTAssertEqual(layout.frame(for: 0).minY, 20)
        XCTAssertEqual(layout.frame(for: 4).minY, layout.blocks[1].y + 20)
        XCTAssertGreaterThan(layout.frame(for: 4).minY, layout.frame(for: 2).maxY)
        XCTAssertEqual(layout.frame(for: 4).minX, 0, "a new burst starts at the left")
        XCTAssertEqual(layout.frame(for: 3).minY, layout.frame(for: 6).minY, "singles sit together at the bottom")
        XCTAssertLessThan(layout.frame(for: 3).minX, layout.frame(for: 6).minX)
        let visible = layout.layoutAttributesForElements(in: NSRect(x: 0, y: 0, width: 320, height: 60))
        XCTAssertEqual(visible.filter { $0.representedElementKind == UniformGridLayout.headerKind }.count, 1)
        XCTAssertEqual(Set(visible.filter { $0.representedElementCategory == .item }.map(\.indexPath!.item)), [0, 1, 2])
        XCTAssertEqual(layout.collectionViewContentSize.height, max(400, layout.blocks[2].y + 20 + layout.cellSize.height))
    }

    private final class StubSource: NSObject, NSCollectionViewDataSource {
        let count: Int
        init(count: Int) { self.count = count }
        func collectionView(_ collectionView: NSCollectionView, numberOfItemsInSection section: Int) -> Int { count }
        func collectionView(_ collectionView: NSCollectionView, itemForRepresentedObjectAt indexPath: IndexPath) -> NSCollectionViewItem {
            NSCollectionViewItem()
        }
    }
}
