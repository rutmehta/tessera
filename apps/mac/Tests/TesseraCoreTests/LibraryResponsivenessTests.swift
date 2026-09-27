import AppKit
import XCTest
@testable import TesseraCore
@testable import Tessera

final class LibraryResponsivenessTests: XCTestCase {
    private func singletons(_ count: Int) -> StubLibrary {
        let items = (0..<count).map { i in
            PhotoItem(id: i, url: nil, name: "item\(i)", kind: .synthetic,
                      captureDate: Date(timeIntervalSince1970: Double(i) * 20),
                      pixelWidth: 100, pixelHeight: 100, seed: UInt64(i))
        }
        return StubLibrary(title: "Singletons", folder: nil, items: items, subfolders: [], scanDuration: 0)
    }

    func testSingletonsAndRemappedItems() {
        let library = singletons(20_000)
        let cull = CullController(memory: library)
        XCTAssertEqual(library.groups.count, 20_000)
        for item in library.items { XCTAssertFalse(cull.isSuggestedBest(item)) }
        let mixed = StubLibrary.synthetic(count: 20_000)
        let other = CullController(memory: mixed)
        let item = mixed.items[0].with(id: mixed.items.count - 1, groupID: 0)
        XCTAssertFalse(other.isSuggestedBest(item), "old group identity cannot select an unrelated remapped item")
    }

    func testBottomHundredLookupScaling() {
        for count in [1_000, 20_000] {
            let library = singletons(count)
            let cull = CullController(memory: library)
            let items = Array(library.items.suffix(100))
            var checksum = 0
            func time(_ lookup: (PhotoItem) -> Bool) -> Double {
                let start = Date()
                for _ in 0..<100 {
                    for item in items { checksum += lookup(item) ? 1 : 0 }
                }
                return Date().timeIntervalSince(start) * 1_000 / 100
            }
            let before = time { cull.isSuggestedBest($0.id) }
            let after = time { cull.isSuggestedBest($0) }
            XCTAssertEqual(checksum, 0)
            print("M2-54 lookup count=\(count) before_ms=\(before) after_ms=\(after)")
        }
    }

    @MainActor func testWarmBottomHundredCellConfiguration() async throws {
        var times: [Double] = []
        for count in [1_000, 20_000] {
            let library = singletons(count)
            let app = AppModel()
            app.install(library, snapshot: CullController.prepare(library))
            let items = Array(library.items.suffix(100))
            for item in items { _ = app.loader.request(item, tier: .thumbnail) { _ in } }
            let deadline = Date().addingTimeInterval(60)
            while items.contains(where: { app.loader.cached($0, tier: .thumbnail) == nil }), Date() < deadline {
                try await Task.sleep(for: .milliseconds(10))
            }
            XCTAssertTrue(items.allSatisfy { app.loader.cached($0, tier: .thumbnail) != nil })
            let cells = items.map { _ in ThumbnailCell() }
            func configure(linearLookup: Bool = false) {
                for (cell, item) in zip(cells, items) {
                    cell.configure(item: item, state: app.state(id: item.id), status: app.cull.statuses[item.id],
                                   basketTarget: app.basketTarget,
                                   suggestedBest: linearLookup ? app.cull.isSuggestedBest(item.id) : app.isSuggestedBest(item),
                                   groupIndex: app.indexInGroup(of: item), groupSize: app.groupSize(of: item),
                                   focused: false, style: .grid, loader: app.loader, suggestion: nil)
                }
            }
            configure()
            var before: [Double] = []
            for _ in 0..<30 {
                let start = Date()
                configure(linearLookup: true)
                before.append(Date().timeIntervalSince(start) * 1_000)
            }
            var samples: [Double] = []
            for _ in 0..<30 {
                let start = Date()
                configure()
                samples.append(Date().timeIntervalSince(start) * 1_000)
            }
            let median = samples.sorted()[samples.count / 2]
            times.append(median)
            print("M2-54 warm_bottom_100 count=\(count) before_median_ms=\(before.sorted()[before.count / 2]) median_ms=\(median) max_ms=\(samples.max()!)")
        }
        print("M2-54 warm_bottom_100 ratio=\(times[1] / times[0]) target=1.2")
    }

    @MainActor func testTwentyThousandCoreInstallAndSelectAll() {
        let library = StubLibrary.synthetic(count: 20_000)
        let snapshot = CullController.prepare(library)
        let app = AppModel()
        let start = Date()
        app.install(library, snapshot: snapshot)
        let installed = Date()
        app.selectAll()
        let selected = Date()
        XCTAssertEqual(app.visibleCount, 20_000)
        XCTAssertEqual(app.selectionCount, 20_000)
        app.refreshVisible()
        XCTAssertEqual(app.selectionCount, 20_000, "optional refresh preserves selection")
        print("M2-54 core_install_ms=\(installed.timeIntervalSince(start) * 1_000) select_all_ms=\(selected.timeIntervalSince(installed) * 1_000)")
    }

    func testLatestRequestWinsEvenWhenOldWorkFinishesLast() {
        var generation = LibraryRequestGeneration()
        let old = generation.next()
        let latest = generation.next()
        XCTAssertFalse(generation.accepts(old))
        XCTAssertTrue(generation.accepts(latest))
        _ = generation.next() // library replacement or invalidation
        XCTAssertFalse(generation.accepts(latest))
    }

    func testSuggestedBestUsesStableGroupIdentity() {
        let library = StubLibrary.synthetic(count: 20_000)
        let cull = CullController(memory: library)
        for item in library.items {
            let group = library.groups[item.groupID]
            XCTAssertEqual(cull.isSuggestedBest(item), group.count > 1 && group.lowerBound == item.id)
        }
    }
}
