import AppKit
import Observation
import SwiftUI
import XCTest
@testable import Tessera
@testable import TesseraCore

/// UX-16 (FOLLOWUPS 26–36): the owner's second pass on 0.7.0. The pure parts of each fix; the
/// shell-level ones are in `ToastLayoutTests` (28), `DeliverPanelToggleTests` (34, 35) and
/// `DevelopHistoryCheckoutTests` (32).
@MainActor
final class UX16FollowupTests: XCTestCase {
    // MARK: 26 — the burst strip cell follows the decision

    /// `AppModel.state(id:)` is what the strip cell, the big frame and the triage rows read. The
    /// cull store is unobserved, so the read must register the decision revision: a view reading
    /// only `state(id:)` is invalidated by a decision, an undo and a redo.
    func testReadingAStateIsInvalidatedByDecisions() {
        let model = AppModel()
        model.loadStubItems(count: 6)
        model.select(position: 1)
        let id = try! XCTUnwrap(model.focusedItem).id

        func observe(_ body: () -> Void) -> Bool {
            let box = FiredBox()
            withObservationTracking { _ = model.state(id: id) } onChange: { box.mark(-1) }
            body()
            return !box.ids.isEmpty
        }
        XCTAssertTrue(observe { model.perform(.keep) }, "a decision invalidates a state read")
        XCTAssertEqual(model.state(id: id).decision, .keep)
        XCTAssertTrue(observe { model.undo() }, "undo invalidates a state read")
        XCTAssertEqual(model.state(id: id).decision, .undecided)
        XCTAssertTrue(observe { model.redo() }, "redo invalidates a state read")
        XCTAssertEqual(model.state(id: id).decision, .keep)
        XCTAssertFalse(observe { model.select(position: 2) }, "a selection change alone is not a decision")
    }

    /// "Keep the pick, reject the rest" changes several frames in one step; every cell re-reads.
    func testKeepBestRejectRestInvalidatesEveryFrameOfTheBurst() {
        let model = AppModel()
        model.loadStubItems(count: 12)
        model.select(position: 0)
        guard let burst = model.focusedBurst, burst.frames.count > 1 else { return }
        let box = FiredBox()
        for id in burst.frames {
            withObservationTracking { _ = model.state(id: id) } onChange: { box.mark(id) }
        }
        model.keepBestRejectRest()
        XCTAssertEqual(box.ids, Set(burst.frames))
        XCTAssertEqual(model.state(id: burst.pick).decision, .keep)
        for id in burst.frames where id != burst.pick { XCTAssertEqual(model.state(id: id).decision, .reject) }
    }

    // MARK: 27 — the signal pills say what they are

    func testSignalPillTexts() {
        XCTAssertEqual(SignalPillText.sharpness(0.123), "0.12")
        XCTAssertEqual(SignalPillText.sharpness(nil), "—")
        XCTAssertEqual(SignalPillText.eyes(0.9), "open")
        XCTAssertEqual(SignalPillText.eyes(0.0), "closed")
        XCTAssertEqual(SignalPillText.face(0.456), "46 %")
        XCTAssertEqual(SignalPillText.face(nil), "—")
        XCTAssertEqual(SignalPillText.score(0.5), "0.50")
        XCTAssertEqual(SignalPillText.score(nil), "—")
        for help in [SignalPillText.sharpnessHelp, SignalPillText.eyesHelp, SignalPillText.faceHelp, SignalPillText.scoreHelp] {
            XCTAssertFalse(help.isEmpty)
        }
        XCTAssertTrue(SignalPillText.sharpnessHelp.contains("0.08"), "the triage threshold is named")
    }

    /// The pill body shows the number beside the bar (the source is the spec of the view).
    func testSharpnessPillShowsTheValueBesideTheBar() throws {
        let source = try String(contentsOf: Self.appSources.appendingPathComponent("Cull/BurstReviewView.swift"), encoding: .utf8)
        let pills = try XCTUnwrap(source.range(of: "private struct SignalPills"))
        let body = source[pills.lowerBound...]
        XCTAssertTrue(body.contains("Text(SignalPillText.sharpness(signals?.sharpness))"), "the value is shown")
        XCTAssertTrue(body.contains("Theme.Width.signalBar"), "the bar stays")
        XCTAssertEqual(body.components(separatedBy: "pill(help:").count - 1, 4, "every pill has a tooltip")
    }

    // MARK: 30 — ⇧⌘Z is the develop history; the agent re-run is its own command

    func testAgentRerunHasItsOwnShortcut() {
        let redo = KeyboardShortcut("z", modifiers: [.command, .shift])
        XCTAssertNotEqual(AppModel.agentRerunShortcut, redo)
        XCTAssertEqual(AppModel.agentRerunShortcut, KeyboardShortcut("r", modifiers: [.command, .shift, .option]))
        XCTAssertFalse(AppModel.agentRerunTitle.lowercased().contains("redo"), "the command does not call itself Redo")
    }

    func testEditMenuRedoNeverReachesTheAgent() {
        let model = AppModel()
        model.loadStubItems(count: 6)
        model.select(position: 0)
        model.selectShellMode(.edit)
        XCTAssertTrue(model.isPhotoEditing)
        XCTAssertEqual(model.redoMenuTitle, "Redo Photo Edit")
        XCTAssertEqual(model.undoMenuTitle, "Undo Photo Edit")
        model.redo()
        XCTAssertFalse(model.agent.isRunning, "the Edit menu's Redo never starts an agent run")
        XCTAssertEqual(model.statusMessage, "Nothing to redo", "the develop history answers, even when empty")
        model.undo()
        XCTAssertEqual(model.statusMessage, "Nothing to undo")
        XCTAssertNil(model.lastAgentInstruction)
        XCTAssertFalse(model.canRerunAgentInstruction)
        model.rerunAgentInstruction()
        XCTAssertEqual(model.statusMessage, "No agent instruction to run again")
        XCTAssertFalse(model.agent.isRunning)
    }

    func testInstructionRunsAreNotCalledRedo() {
        XCTAssertEqual(AgentController.instructionHeadline("warmer", done: 1), "Agent finished “warmer”: 1 photo")
        XCTAssertEqual(AgentController.instructionHeadline("warmer", done: 0), "Agent finished “warmer”: 0 photos")
        XCTAssertFalse(AgentController.instructionHeadline("x", done: 2).contains("Redo"))
    }

    /// The only ⇧⌘Z in the app is the Edit menu's Redo, which calls `AppModel.redo()`.
    func testOnlyTheEditMenuBindsShiftCommandZ() throws {
        let files = try FileManager.default.subpathsOfDirectory(atPath: Self.appSources.path)
            .filter { $0.hasSuffix(".swift") && !$0.hasPrefix("Document/") }
        var bindings: [String] = []
        for rel in files {
            let text = try String(contentsOf: Self.appSources.appendingPathComponent(rel), encoding: .utf8)
            for (i, line) in text.components(separatedBy: "\n").enumerated()
            where line.contains("\"z\"") && line.contains(".shift") && line.contains(".command") {
                bindings.append("\(rel):\(i + 1): \(line.trimmingCharacters(in: .whitespaces))")
            }
        }
        XCTAssertEqual(bindings.count, 1, bindings.joined(separator: "\n"))
        XCTAssertTrue(bindings.first?.hasPrefix("App/AppCommands.swift") == true, bindings.joined(separator: "\n"))
    }

    // MARK: 31 — crop guides

    func testCropGuideSegments() {
        let box = CGRect(x: 10, y: 20, width: 300, height: 200)
        XCTAssertEqual(CropGuides.segments(.thirds, in: box).count, 4)
        XCTAssertEqual(CropGuides.segments(.grid, in: box).count, 14)
        XCTAssertEqual(CropGuides.segments(.goldenRatio, in: box).count, 4)
        XCTAssertEqual(CropGuides.segments(.diagonals, in: box).count, 4)
        XCTAssertEqual(CropGuides.segments(.none, in: box).count, 0)
        XCTAssertEqual(CropGuides.segments(.thirds, in: .zero).count, 0)
        for overlay in CropOverlay.allCases {
            for s in CropGuides.segments(overlay, in: box) {
                XCTAssertTrue(box.insetBy(dx: -0.01, dy: -0.01).contains(s.from) && box.insetBy(dx: -0.01, dy: -0.01).contains(s.to),
                              "\(overlay) \(s) inside the box")
            }
        }
        let thirds = CropGuides.segments(.thirds, in: box)
        XCTAssertEqual(thirds[0].from.x, 110, accuracy: 0.001)
        XCTAssertEqual(thirds[1].from.y, 20 + 200.0 / 3, accuracy: 0.001)
        XCTAssertEqual(CropGuides.segments(.diagonals, in: box)[0].to, CGPoint(x: 210, y: 220))
    }

    /// Stroked on a mid-grey ground, a thirds line is plainly lighter than its surroundings
    /// (the old half-point line at 30 % was not).
    func testCropGuidesAreVisibleWhenStroked() throws {
        let size = CGSize(width: 120, height: 90)
        let box = CGRect(origin: .zero, size: size)
        let image = NSImage(size: size)
        image.lockFocusFlipped(true)
        NSColor(srgbRed: 0.5, green: 0.5, blue: 0.5, alpha: 1).setFill() // lint:allow (test ground)
        box.fill()
        LoupeToolOverlay.strokeGuides(CropGuides.segments(.thirds, in: box))
        image.unlockFocus()
        let rep = try XCTUnwrap(NSBitmapImageRep(data: try XCTUnwrap(image.tiffRepresentation)))
        let scale = CGFloat(rep.pixelsWide) / size.width
        func luminance(_ x: CGFloat, _ y: CGFloat) -> CGFloat {
            let c = try! XCTUnwrap(rep.colorAt(x: Int(x * scale), y: Int(y * scale)))
            return c.redComponent * 0.3 + c.greenComponent * 0.59 + c.blueComponent * 0.11
        }
        let onLine = luminance(40, 45)       // the first vertical third
        let ground = luminance(20, 45)
        XCTAssertGreaterThan(onLine, ground + 0.25, "guide \(onLine) over ground \(ground)")
        let onHorizontal = luminance(20, 30)
        XCTAssertGreaterThan(onHorizontal, ground + 0.25)
    }

    // MARK: 32 — the Edits readout counts the listed steps

    func testEditsReadout() {
        XCTAssertEqual(EditsReadout.text(listed: 0, applied: 0), "No edits")
        XCTAssertEqual(EditsReadout.text(listed: 1, applied: 1), "1 step")
        XCTAssertEqual(EditsReadout.text(listed: 6, applied: 6), "6 steps")
        XCTAssertEqual(EditsReadout.text(listed: 6, applied: 0), "Original · 6 steps")
        XCTAssertEqual(EditsReadout.text(listed: 6, applied: 3), "at 3 of 6 steps")
    }

    // MARK: 36 — the window stays inside the visible frame

    func testWindowFrameFitting() {
        let visible = CGRect(x: 0, y: 80, width: 2000, height: 1180)   // a Dock 80 pt tall
        // Under the Dock: pulled up.
        XCTAssertEqual(WindowFrameFitting.fit(CGRect(x: 100, y: 0, width: 1440, height: 900), in: visible),
                       CGRect(x: 100, y: 80, width: 1440, height: 900))
        // Taller than the screen: shrunk and placed at the bottom of the visible frame.
        XCTAssertEqual(WindowFrameFitting.fit(CGRect(x: 100, y: 0, width: 1440, height: 1400), in: visible),
                       CGRect(x: 100, y: 80, width: 1440, height: 1180))
        // Past the right edge: moved left.
        XCTAssertEqual(WindowFrameFitting.fit(CGRect(x: 1000, y: 200, width: 1440, height: 900), in: visible),
                       CGRect(x: 560, y: 200, width: 1440, height: 900))
        // Already inside: untouched.
        let inside = CGRect(x: 100, y: 200, width: 1440, height: 900)
        XCTAssertEqual(WindowFrameFitting.fit(inside, in: visible), inside)
        // A degenerate visible frame changes nothing.
        XCTAssertEqual(WindowFrameFitting.fit(inside, in: .zero), inside)
    }

    func testWindowIsPulledAboveTheDockOnApply() throws {
        guard let screen = NSScreen.main else { throw XCTSkip("no screen") }
        let visible = screen.visibleFrame
        let window = NSWindow(contentRect: NSRect(x: visible.minX + 10, y: visible.minY - 60, width: 600, height: 400),
                              styleMask: [.titled, .resizable], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        defer { window.close() }
        WindowFrameFitting.apply(to: window)
        XCTAssertGreaterThanOrEqual(window.frame.minY, visible.minY - 0.5, "the window sits above the Dock")
        XCTAssertTrue(visible.insetBy(dx: -0.5, dy: -0.5).contains(window.frame))
    }

    /// Observation's `onChange` is `@Sendable`; it runs on the mutating (main) thread here.
    private final class FiredBox: @unchecked Sendable {
        private let lock = NSLock()
        private var set = Set<Int>()
        func mark(_ id: Int) { lock.withLock { _ = set.insert(id) } }
        var ids: Set<Int> { lock.withLock { set } }
    }

    private static var appSources: URL {
        URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
            .appendingPathComponent("Sources/Tessera")
    }
}
