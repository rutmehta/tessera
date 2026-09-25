import Foundation
import Testing
@testable import TesseraCore

@Suite struct GroupingTests {
    @Test func splitsOnGaps() {
        let base = Date(timeIntervalSince1970: 0)
        let dates = [0, 0.5, 1.0, 10, 10.4, 30].map { base + $0 }
        let groups = CaptureGrouper.groups(forSortedDates: dates, maxGap: 2)
        #expect(groups == [0..<3, 3..<5, 5..<6])
    }

    @Test func syntheticLibraryIsGroupedAndSorted() {
        let lib = StubLibrary.synthetic(count: 20_000)
        #expect(lib.items.count == 20_000)
        #expect(lib.groups.count > 1_000)
        #expect(lib.items.last!.groupID == lib.groups.count - 1)
        for (g, r) in lib.groups.enumerated() { #expect(r.allSatisfy { lib.items[$0].groupID == g }) }
    }
}

@Suite struct CullStoreTests {
    @Test func decisionsGradesMarksBasket() {
        var s = CullStore(count: 4)
        s.apply(.reject, to: [0])
        s.apply(.grade(2), to: [1])
        s.apply(.mark(7), to: [1, 2])
        s.apply(.toggleBasket, to: [3])
        #expect(s[0].decision == .reject)
        #expect(s[1] == CullState(decision: .keep, grade: 2, mark: 7))
        #expect(s[2].mark == 7 && s[2].decision == .undecided)
        #expect(s.counts.reject == 1 && s.counts.keep == 1 && s.counts.undecided == 2 && s.counts.basket == 1)
        s.apply(.mark(7), to: [1, 2])              // toggles off
        #expect(s[1].mark == 0 && s[2].mark == 0)
        s.apply(.reject, to: [1])                  // reject clears grade
        #expect(s[1] == CullState(decision: .reject))
    }

    @Test func undoRedo() {
        var s = CullStore(count: 2)
        s.apply(.keep, to: [0, 1])
        s.apply(.reject, to: [1])
        #expect(s.undo() == [1])
        #expect(s[1].decision == .keep)
        #expect(s.redo() == [1])
        #expect(s[1].decision == .reject)
        #expect(s.counts.keep == 1 && s.counts.reject == 1)
    }

    @Test func exifDateParsing() {
        let d = MetadataReader.parseExifDate("2024:05:01 12:30:15", subsec: "25")
        #expect(d != nil)
        let d2 = MetadataReader.parseExifDate("2024:05:01 12:30:15", subsec: nil)!
        #expect(abs(d!.timeIntervalSince(d2) - 0.25) < 1e-6)
    }
}
