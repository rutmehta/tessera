import TesseraCore
import TesseraFFI
import XCTest

/// Keywords panel ▸ Suggested (WP M3-15): the chip model's accept / ⇧-accept-all / reject and
/// threshold, and the filter bar's search-term serialisation.
final class SuggestionChipTests: XCTestCase {
    private func sample() -> SuggestionChips {
        SuggestionChips([
            SuggestedKeyword(keyword: "texture", confidence: 0.22),
            SuggestedKeyword(keyword: "beach", confidence: 0.81, path: ["Places", "Beach"], existing: true),
            SuggestedKeyword(keyword: "photograph", confidence: 0.93),
            SuggestedKeyword(keyword: "sea", confidence: 0.62, ambiguous: true),
            SuggestedKeyword(keyword: "Beach", confidence: 0.4),  // duplicate by name
        ])
    }

    func testOrderingDeduplicationAndMapping() {
        let chips = sample()
        XCTAssertEqual(chips.items.map(\.keyword), ["photograph", "beach", "sea", "texture"])
        XCTAssertEqual(chips.threshold, SuggestionChips.defaultThreshold)
        XCTAssertEqual(chips.items[1].mappedPath, "Places › Beach")
        XCTAssertNil(chips.items[0].mappedPath, "new keywords are created under Suggested")
        XCTAssertEqual(chips.items[0].path, ["Suggested", "photograph"])
        XCTAssertEqual(chips.items[0].percent, "93 %")
        XCTAssertEqual(SuggestedKeyword(keyword: "x", confidence: 1.7).confidence, 1)
    }

    func testClickAcceptsOneAndRejectRemovesOne() {
        var chips = sample()
        XCTAssertEqual(chips.accept("BEACH"), ["beach"], "case-insensitive; returns the engine's spelling")
        XCTAssertFalse(chips.items.contains { $0.keyword == "beach" })
        XCTAssertEqual(chips.accept("beach"), [], "already accepted")
        XCTAssertEqual(chips.accept("sea"), [], "ambiguous chips cannot be accepted")
        XCTAssertTrue(chips.items.contains { $0.keyword == "sea" })
        XCTAssertEqual(chips.reject("sea"), ["sea"], "…but can be rejected")
        XCTAssertEqual(chips.reject("unknown"), [])
        XCTAssertEqual(chips.items.map(\.keyword), ["photograph", "texture"])
    }

    func testShiftClickAcceptsEverythingAtOrAboveTheThreshold() {
        var chips = sample()
        chips.threshold = 0.62
        XCTAssertEqual(chips.aboveThreshold.map(\.keyword), ["photograph", "beach"], "sea is ambiguous")
        XCTAssertTrue(chips.isAboveThreshold(chips.items[2]))
        XCTAssertEqual(chips.acceptAllAboveThreshold(), ["photograph", "beach"])
        XCTAssertEqual(chips.items.map(\.keyword), ["sea", "texture"])
        XCTAssertEqual(chips.acceptAllAboveThreshold(), [])
        chips.threshold = 0.1
        XCTAssertEqual(chips.acceptAllAboveThreshold(), ["texture"])
        chips.threshold = 7
        XCTAssertEqual(chips.threshold, 1, "clamped")
    }

    func testReplaceKeepsTheThreshold() {
        var chips = sample()
        chips.threshold = 0.3
        chips.replace(with: [SuggestedKeyword(keyword: "tree", confidence: 0.5)])
        XCTAssertEqual(chips.threshold, 0.3)
        XCTAssertEqual(chips.items.map(\.keyword), ["tree"])
    }

    func testBridgeRecordConversion() {
        let info = KeywordSuggestionInfo(keyword: "blue", confidence: 0.81, images: 2, path: ["Colors", "Blue"],
                                         existing: true, ambiguous: false)
        let s = SuggestedKeyword(info)
        XCTAssertEqual(s.images, 2)
        XCTAssertEqual(s.confidence, 0.81, accuracy: 1e-6)
        XCTAssertEqual(s.mappedPath, "Colors › Blue")
    }
}

final class SearchTermTests: XCTestCase {
    func testTextTermsAreQuotedWithTheGrammarsJSONEscaping() {
        XCTAssertEqual(SearchTerm.text("EXIT ONLY"), #"text:"EXIT ONLY""#)
        XCTAssertEqual(SearchTerm.text("  two\n lines\t"), #"text:"two lines""#)
        XCTAssertEqual(SearchTerm.text(#"say "hi" \ now"#), #"text:"say \"hi\" \\ now""#)
        XCTAssertEqual(SearchTerm.text("a/b"), #"text:"a/b""#, "slashes are not escaped")
        XCTAssertEqual(SearchTerm.text("   "), "")
    }

    func testAppendingANDsOntoExistingText() {
        let t = SearchTerm.text("exit")
        XCTAssertEqual(SearchTerm.appending(t, to: ""), t)
        XCTAssertEqual(SearchTerm.appending(t, to: "rating>=2"), "rating>=2 " + t)
        XCTAssertEqual(SearchTerm.appending(t, to: "beach OR sea"), "(beach OR sea) " + t)
        XCTAssertEqual(SearchTerm.appending(t, to: "beach or sea"), "(beach or sea) " + t)
        XCTAssertEqual(SearchTerm.appending(t, to: "(beach OR sea) rating>=2"), "(beach OR sea) rating>=2 " + t)
        XCTAssertEqual(SearchTerm.appending(t, to: #"text:"cats OR dogs""#), #"text:"cats OR dogs" "# + t)
        XCTAssertEqual(SearchTerm.appending(t, to: t), t, "no duplicate term")
        XCTAssertEqual(SearchTerm.appending("", to: " beach "), "beach")
    }
}
