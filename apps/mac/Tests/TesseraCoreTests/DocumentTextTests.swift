import CoreGraphics
import Foundation
import XCTest
import TesseraFFI
@testable import Tessera
@testable import TesseraCore

/// The Type tool's host model (WP B5-10): the engine JSON schema, affine layouts, the three index
/// domains (UTF-16 / UTF-8 / runs), run surgery over mixed styles, caret stops over ligatures and
/// bidi, IME composition, and drafts through a real `DocumentSession` (one node per typing group,
/// cancel, locks, conversion undo).
final class DocumentTextTests: XCTestCase {
    private func run(_ t: String, size: Float = 24, weight: UInt16 = 400, color: [UInt8] = [0, 0, 0, 255]) -> TextRunModel {
        TextRunModel(text: t, family: "Helvetica", weight: weight, size: size, color: color)
    }

    /// A synthetic single-line layout: one glyph per `(cluster, advance, rtl)` in visual order.
    private func layout(_ glyphs: [(Int, Double, Bool)], length: Int, baseline: Double = 20) -> TextLayoutInfo {
        var x = 0.0
        var gs: [TextGlyphInfo] = []
        for (c, adv, rtl) in glyphs {
            gs.append(TextGlyphInfo(run: 0, cluster: c, x: x, y: baseline, advance: adv, rtl: rtl))
            x += adv
        }
        let line = TextLineInfo(source: 0..<length, glyphs: 0..<gs.count, x: 0, baseline: baseline, width: x,
                                availableWidth: nil, ascent: 18, descent: 5)
        return TextLayoutInfo(glyphs: gs, lines: [line], overflow: false, textLength: length)
    }

    // MARK: Schema and affines

    func testModelJSONMatchesTheEngineSchemaAndKeepsPathData() throws {
        let rust = #"{"runs":[{"text":"Hi","family":"Helvetica","weight":700,"italic":true,"size":30.5,"tracking":1.0,"kerning":false,"leading":0.0,"baseline_shift":2.0,"color":[1,2,3,255],"features":{"liga":0},"axes":{}}],"paragraph":{"alignment":"center","hyphenation":false,"left_indent":1.0,"right_indent":0.0,"first_line_indent":0.0,"space_before":0.0,"space_after":3.0},"text_box":{"kind":"paragraph","width":200.0,"height":80.0},"vertical":false,"warp":{"kind":"wave","amount":0.25},"path":{"commands":[{"move":[0.0,0.0]},{"line":[10.0,0.0]}],"offset":0.0}}"#
        let m = try XCTUnwrap(TextSourceModel(json: rust))
        XCTAssertEqual(m.runs[0].baselineShift, 2)
        XCTAssertEqual(m.runs[0].features["liga"], 0)
        XCTAssertEqual(m.paragraph.alignment, .center)
        XCTAssertEqual(m.textBox, .paragraph(width: 200, height: 80))
        XCTAssertEqual(m.warp.kind, .wave)
        XCTAssertTrue(m.needsSourceEditor)
        let back = try XCTUnwrap(TextSourceModel(json: m.json))
        XCTAssertEqual(back, m, "round trip, opaque path included")
        let keys = try JSONSerialization.jsonObject(with: Data(m.json.utf8)) as? [String: Any]
        XCTAssertEqual(Set(keys?.keys ?? [:].keys), ["runs", "paragraph", "text_box", "vertical", "warp", "path"])
        let runKeys = ((keys?["runs"] as? [[String: Any]])?.first ?? [:]).keys
        XCTAssertTrue(runKeys.contains("baseline_shift") && !runKeys.contains("baselineShift"))
        // The engine accepts it (strict serde, unknown fields rejected).
        let plain = TextSourceModel.point("x", family: "Helvetica", size: 20)
        XCTAssertNoThrow(try layoutText(modelJson: plain.json))
    }

    func testAffineConversionToCoreGraphicsKeepsTranslationAndSkew() {
        let m = AffineTransform2D(a: 1, b: 0.25, c: 12, d: 0.5, e: 2, f: 44)
        let cg = m.cgAffineTransform
        let p = CGPoint(x: 4, y: 8)
        XCTAssertEqual(m.apply(p), p.applying(cg))
        XCTAssertEqual(m.apply(p), CGPoint(x: 4 + 2 + 12, y: 2 + 16 + 44))
        XCTAssertEqual(AffineTransform2D(cg), m)
        XCTAssertEqual(AffineTransform2D.translation(5, 7).cgAffineTransform, CGAffineTransform(translationX: 5, y: 7))
        XCTAssertFalse(AffineTransform2D(a: 1, b: 2, c: 0, d: 2, e: 4, f: 0).isFiniteAndInvertible)
        XCTAssertFalse(AffineTransform2D(a: .nan, b: 0, c: 0, d: 0, e: 1, f: 0).isFiniteAndInvertible)
    }

    // MARK: Index domains

    func testUTF16UTF8AndGraphemeMappingAcrossNonBMPAndCombiningMarks() {
        // a · e + U+0301 · 𝐀 (U+1D400, a surrogate pair) · 👍🏽 (two scalars, one grapheme)
        let s = "ae\u{301}\u{1D400}👍🏽"
        let m = TextIndexMap(s)
        XCTAssertEqual(m.utf8Count, s.utf8.count)
        XCTAssertEqual(m.utf16Count, s.utf16.count)
        XCTAssertEqual(m.graphemeBoundaries, [0, 1, 4, 8, 16])
        XCTAssertEqual(m.utf16(fromUTF8: 4), 3)        // after e + mark (1 + 1 + 1 code units)
        XCTAssertEqual(m.utf16(fromUTF8: 8), 5)        // after the surrogate pair
        XCTAssertEqual(m.utf8(fromUTF16: 4), 4, "inside a surrogate pair rounds down")
        XCTAssertEqual(m.utf8(fromUTF16: 5), 8)
        XCTAssertEqual(m.utf16(fromUTF8: 6), 3, "inside a UTF-8 scalar rounds down")
        XCTAssertEqual(m.utf16Range(4..<8), NSRange(location: 3, length: 2))
        XCTAssertEqual(m.utf8Range(NSRange(location: 3, length: 2)), 4..<8)
        XCTAssertEqual(m.substring(4..<8), "\u{1D400}")
        XCTAssertTrue(m.isGraphemeBoundary(8))
        XCTAssertFalse(m.isGraphemeBoundary(2), "the combining mark is not a boundary")
    }

    func testRunMappingPrefersTheRunBeforeTheCaret() {
        let runs = [run("ab"), run("cd", weight: 700), run("")]
        XCTAssertEqual(TextIndexMap.run(at: 0, in: runs).run, 0)
        XCTAssertEqual(TextIndexMap.run(at: 2, in: runs).run, 0)
        XCTAssertEqual(TextIndexMap.run(at: 2, in: runs).offset, 2)
        XCTAssertEqual(TextIndexMap.run(at: 2, in: runs, preferPrevious: false).run, 1)
        XCTAssertEqual(TextIndexMap.run(at: 3, in: runs).run, 1)
        XCTAssertEqual(TextIndexMap.run(at: 3, in: runs).offset, 1)
        XCTAssertEqual(TextRuns.starts(runs), [0, 2, 4, 4])
    }

    // MARK: Run surgery

    func testInsertJoinsTheRunBeforeTheCaretAndSplitsForATypingStyle() {
        let runs = [run("Hello "), run("world", weight: 700)]
        let a = TextRuns.replace(runs, range: 6..<6, with: "big ")
        XCTAssertEqual(a.map(\.text), ["Hello big ", "world"], "typing continues the run before the caret")
        var red = run("", color: [255, 0, 0, 255])
        red.text = ""
        let b = TextRuns.replace(runs, range: 3..<3, with: "X", style: red)
        XCTAssertEqual(b.map(\.text), ["Hel", "X", "lo ", "world"])
        XCTAssertEqual(b[1].color, [255, 0, 0, 255])
        XCTAssertEqual(b[3], runs[1], "untouched run unchanged")
        // Replacing a selection takes the first selected character's style.
        let c = TextRuns.replace(runs, range: 6..<11, with: "there")
        XCTAssertEqual(c.map(\.text), ["Hello ", "there"])
        XCTAssertEqual(c[1].weight, 700)
    }

    func testDeletionAcrossMixedStyleRunsKeepsUntouchedRuns() {
        let runs = [run("plain "), run("bold", weight: 700), run(" end", size: 18)]
        let a = TextRuns.replace(runs, range: 4..<8, with: "")
        XCTAssertEqual(a.map(\.text), ["plai", "ld", " end"])
        XCTAssertEqual(a[1].weight, 700)
        XCTAssertEqual(a[2], runs[2])
        let b = TextRuns.replace(runs, range: 6..<10, with: "")
        XCTAssertEqual(b.map(\.text), ["plain ", " end"], "a fully deleted run disappears")
        let c = TextRuns.replace(runs, range: 0..<14, with: "")
        XCTAssertEqual(c.count, 1, "an empty model keeps one run for its style")
        XCTAssertEqual(c[0].text, "")
    }

    func testApplyStyleSplitsOnlyAtTheSelectionAndSpliceMatchesTheEngine() {
        let runs = [run("one two"), run(" three", size: 30)]
        let styled = TextRuns.applyStyle(runs, range: 4..<10) { $0.italic = true }
        XCTAssertEqual(styled.map(\.text), ["one ", "two", " th", "ree"])
        XCTAssertEqual(styled.map(\.italic), [false, true, true, false])
        XCTAssertEqual(styled[3].size, 30)
        XCTAssertEqual(styled.map(\.text).joined(), "one two three")
        // Run-index splice (the engine's EditTextRuns derivation).
        let sp = try? XCTUnwrap(TextRuns.splice(from: runs, to: styled))
        XCTAssertEqual(sp?.range, 0..<2)
        XCTAssertEqual(sp?.runs.count, 4)
        var only = runs
        only[1].text = " four"
        XCTAssertEqual(TextRuns.splice(from: runs, to: only)?.range, 1..<2)
        XCTAssertNil(TextRuns.splice(from: runs, to: runs))
        var inserted = runs
        inserted.insert(run("mid"), at: 1)
        XCTAssertEqual(TextRuns.splice(from: runs, to: inserted)?.range, 1..<1, "an empty range inserts")
        let summary = TextStyleSummary(Array(styled[1...2]))
        XCTAssertEqual(summary.italic, .one(true))
        XCTAssertEqual(summary.size, .mixed)
        XCTAssertEqual(summary.family, .one("Helvetica"))
    }

    // MARK: Caret over the engine layout

    func testLigatureInteriorIsNotACaretStopAndHitsSnapToClusterEdges() {
        // "office": o | ffi (one glyph, cluster 1) | c | e — as Noto Sans shapes it.
        let model = TextSourceModel.point("office", family: "Noto Sans", size: 24)
        let idx = TextLayoutIndex(model: model, layout: layout([(0, 10, false), (1, 18, false), (4, 9, false), (5, 9, false)], length: 6))
        XCTAssertEqual(idx.caretStops, [0, 1, 4, 5, 6])
        XCTAssertEqual(idx.snap(2), 1)
        XCTAssertEqual(idx.snap(3, forward: true), 4)
        XCTAssertEqual(idx.caret(at: 1).top.x, 10)
        XCTAssertEqual(idx.caret(at: 4).top.x, 28)
        XCTAssertEqual(idx.caret(at: 6).top.x, 46, "end: trailing edge of the last cluster")
        XCTAssertEqual(idx.hitTest(CGPoint(x: 14, y: 15)), 1, "left half of the ligature")
        XCTAssertEqual(idx.hitTest(CGPoint(x: 25, y: 15)), 4, "right half of the ligature")
        XCTAssertEqual(idx.hitTest(CGPoint(x: -5, y: 15)), 0)
        XCTAssertEqual(idx.hitTest(CGPoint(x: 400, y: 15)), 6)
        var session = TextEditSession(model: model, selection: 4..<4)
        session.stops = idx.caretStops
        session.deleteBackward()
        XCTAssertEqual(session.text, "oce", "⌫ removes the whole ligature cluster")
        XCTAssertEqual(idx.selectionRects(1..<4), [CGRect(x: 10, y: 2, width: 18, height: 23)])
    }

    func testMixedBidiCaretAndHitTestFollowTheVisualLayout() {
        // "a אבג z": visual a | ' ' | ג(6) ב(4) א(2) | ' ' | z
        let text = "a \u{5D0}\u{5D1}\u{5D2} z"
        let model = TextSourceModel.point(text, family: "Helvetica", size: 20)
        let lay = layout([(0, 10, false), (1, 5, false), (6, 10, true), (4, 10, true), (2, 10, true), (8, 5, false), (9, 10, false)],
                         length: text.utf8.count)
        let idx = TextLayoutIndex(model: model, layout: lay)
        XCTAssertEqual(idx.caretStops, [0, 1, 2, 4, 6, 8, 9, 10])
        // Before א (offset 2) sits at א's RIGHT edge; before ג (6) at ג's right edge.
        XCTAssertEqual(idx.caret(at: 2).top.x, 45)
        XCTAssertEqual(idx.caret(at: 6).top.x, 25)
        XCTAssertEqual(idx.caret(at: 4).top.x, 35)
        // ב spans x 25…35. Its right half is its logical start (before ב, 4); its left half its end (6).
        XCTAssertEqual(idx.hitTest(CGPoint(x: 33, y: 15)), 4)
        XCTAssertEqual(idx.hitTest(CGPoint(x: 27, y: 15)), 6, "left half of ב: after ב")
        XCTAssertEqual(idx.hitTest(CGPoint(x: 43, y: 15)), 2, "right half of א: before א")
        // Replacing the visually selected ב keeps the source intact around it.
        var s = TextEditSession(model: model, selection: 4..<6)
        s.stops = idx.caretStops
        s.insert("\u{5D3}")
        XCTAssertEqual(s.text, "a \u{5D0}\u{5D3}\u{5D2} z")
        XCTAssertEqual(idx.selectionRects(2..<6), [CGRect(x: 25, y: 2, width: 20, height: 23)])
    }

    func testTrailingNewlineVerticalMovesAndLineBounds() {
        let text = "ab\ncd\n"
        let model = TextSourceModel.point(text, family: "Helvetica", size: 20)
        let g = [TextGlyphInfo(run: 0, cluster: 0, x: 0, y: 20, advance: 10),
                 TextGlyphInfo(run: 0, cluster: 1, x: 10, y: 20, advance: 10),
                 TextGlyphInfo(run: 0, cluster: 3, x: 0, y: 44, advance: 12),
                 TextGlyphInfo(run: 0, cluster: 4, x: 12, y: 44, advance: 12)]
        let lines = [TextLineInfo(source: 0..<3, glyphs: 0..<2, x: 0, baseline: 20, width: 20, availableWidth: nil, ascent: 18, descent: 5),
                     TextLineInfo(source: 3..<6, glyphs: 2..<4, x: 0, baseline: 44, width: 24, availableWidth: nil, ascent: 18, descent: 5)]
        let idx = TextLayoutIndex(model: model, layout: TextLayoutInfo(glyphs: g, lines: lines, overflow: false, textLength: 6))
        XCTAssertEqual(idx.caret(at: 2).line, 0)
        XCTAssertEqual(idx.caret(at: 2).top.x, 20, "before the separator: end of line 0")
        XCTAssertEqual(idx.caret(at: 3).line, 1)
        XCTAssertEqual(idx.caret(at: 6).line, 2, "after a final newline: a new empty line")
        XCTAssertGreaterThan(idx.caret(at: 6).top.y, 44)
        XCTAssertEqual(idx.verticalMove(from: 1, down: true), 4)
        XCTAssertEqual(idx.verticalMove(from: 4, down: false), 1)
        XCTAssertEqual(idx.lineBounds(of: 4), 3..<5)
        XCTAssertEqual(idx.hitTest(CGPoint(x: 100, y: 44)), 5)
        XCTAssertEqual(idx.wordRange(at: 4), 3..<5)
    }

    // MARK: Session and IME

    func testMarkedTextCancelRestoresAndCommitKeepsOneComposition() {
        var s = TextEditSession(model: TextSourceModel.point("ab", family: "Helvetica", size: 20), selection: 1..<1)
        s.setMarked("k", selected: 1..<1)
        s.setMarked("か", selected: 3..<3)
        XCTAssertEqual(s.text, "aかb")
        XCTAssertEqual(s.marked, 1..<4)
        XCTAssertTrue(s.isComposing)
        s.cancelComposition()
        XCTAssertEqual(s.text, "ab", "cancel restores the model before composition")
        XCTAssertEqual(s.selection, 1..<1)
        XCTAssertFalse(s.isChanged)
        s.setMarked("かん", selected: 6..<6)
        s.insert("漢")   // the IME commits
        XCTAssertEqual(s.text, "a漢b")
        XCTAssertNil(s.marked)
        XCTAssertEqual(s.caret, 4)
        XCTAssertTrue(s.isChanged)
        // setMarked("") ends a composition without text.
        s.setMarked("x", selected: 1..<1)
        s.setMarked("", selected: 0..<0)
        XCTAssertEqual(s.text, "a漢b")
        XCTAssertNil(s.marked)
        s.setMarked("y", selected: 1..<1)
        s.commitComposition()
        XCTAssertEqual(s.text, "a漢yb")
        XCTAssertEqual(s.caret, 5)
    }

    func testTypingStyleAndDeleteBackwardOverNonBMP() {
        var s = TextEditSession(model: TextSourceModel.point("hi", family: "Helvetica", size: 20))
        s.applyStyle { $0.size = 40 }
        XCTAssertEqual(s.model.runs.count, 1, "a caret style waits for typed text")
        s.insert("👍🏽")
        XCTAssertEqual(s.model.runs.map(\.size), [20, 40])
        XCTAssertEqual(s.styleSummary.size, .one(40))
        s.deleteBackward()
        XCTAssertEqual(s.text, "hi", "⌫ removes the whole grapheme (two scalars)")
        s.select(0..<2)
        XCTAssertEqual(s.selectedText, "hi")
        s.applyStyle { $0.weight = 700 }
        XCTAssertEqual(s.model.runs.first?.weight, 700)
        s.deleteForward()
        XCTAssertEqual(s.text, "")
        XCTAssertEqual(s.model.runs.count, 1)
    }

    // MARK: Real engine

    private func temp() throws -> URL {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("text-doc-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        return dir
    }

    private func backend() throws -> EngineDocumentBackend {
        let dir = try temp()
        let engine = try Engine.open(appSupportDir: dir.appendingPathComponent("support").path)
        let session = try engine.newDocument(width: 400, height: 200, depth: .u8, profile: nil)
        return EngineDocumentBackend(session: session)
    }

    func testEngineLayoutClustersAgreeWithTheIndexOnRealText() throws {
        let text = "Tae\u{301}\u{1D400}fi \u{5D0}\u{5D1}"
        let model = TextSourceModel.point(text, family: "Helvetica", size: 32)
        let lay = try TextBridge.layout(model)
        XCTAssertEqual(lay.textLength, text.utf8.count)
        let idx = TextLayoutIndex(model: model, layout: lay)
        let map = TextIndexMap(text)
        for g in lay.glyphs { XCTAssertTrue(map.isGraphemeBoundary(g.cluster), "cluster \(g.cluster) on a grapheme boundary") }
        XCTAssertFalse(idx.caretStops.contains(3), "no caret between e and its mark")
        XCTAssertTrue(idx.caretStops.contains(5) && idx.caretStops.contains(9) && !idx.caretStops.contains(6), "the non-BMP letter is one stop")
        XCTAssertTrue(lay.glyphs.contains { $0.rtl }, "Hebrew is RTL")
        for o in idx.caretStops {
            let c = idx.caret(at: o)
            XCTAssertTrue(c.top.x.isFinite && c.bottom.y > c.top.y)
            XCTAssertEqual(idx.snap(idx.hitTest(CGPoint(x: c.top.x, y: (c.top.y + c.bottom.y) / 2))), idx.hitTest(CGPoint(x: c.top.x, y: (c.top.y + c.bottom.y) / 2)))
        }
        let fonts = TextBridge.fonts()
        XCTAssertTrue(fonts.contains { $0.family == "Helvetica" })
        XCTAssertFalse(fonts.contains { $0.family.hasPrefix(".") })
    }

    func testDraftsThroughTheSessionAreOneNodeCancelAndLocks() throws {
        let b = try backend()
        let base = TextSourceModel(runs: [run("Hello"), run(" you", weight: 700)])
        let t = AffineTransform2D.translation(20, 60)
        // A new layer: drafts record nothing; the final call records "Add Text".
        let h0 = try b.historyItems().count
        _ = try b.addTextLayer(name: "", parent: nil, index: nil, model: base, transform: t, interactive: true)
        XCTAssertEqual(try b.historyItems().count, h0)
        let added = try b.addTextLayer(name: "", parent: nil, index: nil, model: base, transform: t, interactive: false)
        let id = try XCTUnwrap(added.created.first)
        XCTAssertEqual(try b.historyItems().count, h0 + 1)
        let rec = try b.textLayer(id: id)
        XCTAssertEqual(rec.model, base)
        XCTAssertEqual(rec.transform, t)
        XCTAssertTrue(rec.caretEditable)
        // A typing group: many drafts, one node, exact undo.
        var s = TextEditSession(model: base)
        for ch in ", friend" {
            s.insert(String(ch))
            _ = try b.setTextLayer(id: id, model: s.model, transform: t, interactive: true, expectedRevision: rec.revision)
        }
        XCTAssertEqual(try b.historyItems().count, h0 + 1)
        XCTAssertTrue(try b.textLayer(id: id).draftPending)
        _ = try b.commit(label: "Typing")
        XCTAssertEqual(try b.historyItems().count, h0 + 2)
        XCTAssertEqual(try b.textLayer(id: id).model, s.model)
        _ = try b.undo()
        XCTAssertEqual(try b.textLayer(id: id).model, base)
        _ = try b.redo()
        // Cancel: no node, model back.
        let committed = try b.textLayer(id: id)
        var d = TextEditSession(model: committed.model)
        d.insert("!!!")
        _ = try b.setTextLayer(id: id, model: d.model, transform: t, interactive: true, expectedRevision: committed.revision)
        _ = try b.cancelSourcePreview()
        XCTAssertEqual(try b.textLayer(id: id).model, committed.model)
        XCTAssertEqual(try b.historyItems().count, h0 + 2)
        // Stale revision and run ranges fail without change.
        XCTAssertThrowsError(try b.setTextLayer(id: id, model: d.model, transform: t, interactive: false,
                                                expectedRevision: committed.revision + 7))
        XCTAssertThrowsError(try b.editTextRuns(id: id, runs: 1..<9, with: [], expectedRevision: nil))
        // Position lock: content edits pass, moves fail.
        _ = try b.setLocks(id: id, locks: LayerLockFlags(transparency: false, pixels: false, position: true, all: false))
        XCTAssertThrowsError(try b.setTextLayer(id: id, model: committed.model, transform: .translation(50, 60),
                                                interactive: false, expectedRevision: nil))
        XCTAssertNoThrow(try b.setTextLayer(id: id, model: d.model, transform: t, interactive: false, expectedRevision: nil))
        // Pixel lock blocks content and conversion.
        _ = try b.setLocks(id: id, locks: LayerLockFlags(transparency: false, pixels: true, position: false, all: false))
        XCTAssertThrowsError(try b.convertToPixels(id: id))
        _ = try b.setLocks(id: id, locks: LayerLockFlags(transparency: false, pixels: false, position: false, all: false))
        let n = try b.historyItems().count
        _ = try b.convertToPixels(id: id)
        XCTAssertEqual(try b.layer(id: id).kind, .pixel)
        XCTAssertEqual(try b.historyItems().count, n + 1)
        _ = try b.undo()
        XCTAssertEqual(try b.layer(id: id).kind, .text)
        XCTAssertEqual(try b.textLayer(id: id).model, d.model, "undo restores the editable source")
    }
}
