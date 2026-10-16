import AppKit
import XCTest
@testable import Tessera
@testable import TesseraCore

/// UX-15 (item 23): a grid cell whose thumbnail request returned without an image (the engine
/// declined, the embedded preview was unreadable, the event stream closed) asks again the next
/// time it is shown instead of staying blank; and the grid row's Smart Preview glyph and Review
/// button lay out in their corners.
@MainActor
final class ThumbnailRetryTests: XCTestCase {
    private func scratch() throws -> URL {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("thumbnail-retry-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        return dir
    }

    func testRequestThatFinishesWithoutAnImageIsFinishedNotCancelled() async throws {
        let url = try scratch().appendingPathComponent("missing.jpg")
        let item = PhotoItem(id: 0, url: url, name: "missing.jpg", kind: .jpeg, captureDate: .distantPast, pixelWidth: 96, pixelHeight: 64)
        let loader = ThumbnailLoader(thumbnailCostLimit: 1 << 20, previewCostLimit: 1 << 20, concurrency: 1)
        var delivered = 0
        let request = try XCTUnwrap(loader.request(item, tier: .thumbnail) { _ in delivered += 1 })
        await request.waitForCompletion()
        XCTAssertEqual(delivered, 0)
        XCTAssertTrue(request.isFinished)
        XCTAssertFalse(request.isCancelled)
        loader.removeAll()
    }

    func testCellShownAgainAsksForAThumbnailItNeverReceived() async throws {
        let dir = try scratch()
        let url = dir.appendingPathComponent("late.jpg")
        let item = PhotoItem(id: 7, url: url, name: "late.jpg", kind: .jpeg, captureDate: .distantPast, pixelWidth: 96, pixelHeight: 64)
        let loader = ThumbnailLoader(thumbnailCostLimit: 1 << 20, previewCostLimit: 1 << 20, concurrency: 1)
        let cell = ThumbnailCell()
        _ = cell.view
        let status = ItemStatus(phase: .unedited, albums: [], isCachedDeclaration: false)
        // The file is not there yet: the flight ends without an image.
        cell.configure(item: item, state: CullState(), status: status, basketTarget: "", suggestedBest: false,
                       groupIndex: 0, groupSize: 1, focused: false, style: .grid, loader: loader)
        try await drain(loader)
        XCTAssertFalse(cell.delivered)
        XCTAssertNil((cell.view as? ThumbnailCellView)?.imageLayer.contents, "blank until an image arrives")

        // Shown again with nothing new: nothing arrives, and the cell keeps asking without a
        // request in flight piling up (one at a time).
        cell.resumeLoading(loader: loader)
        try await drain(loader)
        XCTAssertFalse(cell.delivered)

        // The file appears (the engine finished, the volume came back): the next show draws it.
        try ShellHarness.writeJPEG(url, shade: 40)
        cell.resumeLoading(loader: loader)
        try await drain(loader)
        XCTAssertTrue(cell.delivered, "a cell shown again asks for the thumbnail it never got")
        XCTAssertNotNil((cell.view as? ThumbnailCellView)?.imageLayer.contents)

        // Delivered: being shown again asks for nothing more.
        let before = loader.queueSnapshot
        cell.resumeLoading(loader: loader)
        XCTAssertEqual(loader.queueSnapshot.active + loader.queueSnapshot.pending, before.active + before.pending)
        cell.stopLoading()
        loader.removeAll()
    }

    func testSmartPreviewGlyphSitsInTheImageCornerClearOfTheChips() {
        let image = NSRect(x: 6, y: 6, width: 200, height: 144)
        let glyph = BadgeOverlayView.smartPreviewGlyphFrame(imageRect: image, small: false)
        XCTAssertEqual(glyph.size, NSSize(width: Theme.Height.chip, height: Theme.Height.chip), "chip-sized, one glyph")
        XCTAssertEqual(glyph.maxX, image.maxX - Theme.Space.xs)
        XCTAssertEqual(glyph.maxY, image.maxY - Theme.Space.xs, "bottom-right corner (flipped)")
        XCTAssertTrue(image.contains(glyph))
        // The decision chip is top-left, the mark chip top-right: neither shares the glyph's corner.
        let decision = NSRect(x: image.minX + Theme.Space.xs, y: image.minY + Theme.Space.xs, width: 48, height: Theme.Height.chip)
        let mark = NSRect(x: image.maxX - Theme.Space.xs - Theme.Height.chip, y: image.minY + Theme.Space.xs, width: Theme.Height.chip, height: Theme.Height.chip)
        XCTAssertFalse(glyph.intersects(decision))
        XCTAssertFalse(glyph.intersects(mark))
        let small = BadgeOverlayView.smartPreviewGlyphFrame(imageRect: image, small: true)
        XCTAssertEqual(small.maxX, image.maxX - Theme.Space.xxs)
    }

    func testBurstRowHeaderLaysOutTheReviewButtonAfterTheDetail() {
        let header = BurstRowHeaderView(frame: NSRect(x: 0, y: 0, width: 600, height: Theme.Height.large))
        header.configure(title: "Burst 3", detail: "2 frames · pick DSC02497.ARW") {}
        header.layoutSubtreeIfNeeded()
        let f = BurstRowHeaderView.frames(width: 600, height: Theme.Height.large, title: CGSize(width: 50, height: 15), detail: 180, review: 58)
        XCTAssertEqual(f.review.height, Theme.Height.regular, "a control, not a text link")
        XCTAssertGreaterThanOrEqual(f.review.minX, f.detail.maxX + Theme.Space.s - 0.5)
        XCTAssertLessThanOrEqual(f.review.maxX, 600)
        XCTAssertEqual(f.review.midY, Theme.Height.large / 2, accuracy: 0.5, "centred in the header row")
        // A narrow header truncates the detail, never the button.
        let narrow = BurstRowHeaderView.frames(width: 200, height: Theme.Height.large, title: CGSize(width: 50, height: 15), detail: 180, review: 58)
        XCTAssertLessThanOrEqual(narrow.review.maxX, 200 + 0.5)
        XCTAssertEqual(narrow.review.width, 58)
        let button = header.subviews.compactMap { $0 as? SecondaryHeaderButton }.first
        XCTAssertEqual(button?.title, "Review")
        XCTAssertEqual(button?.accessibilityIdentifier(), "library.cull.grid.review")
    }

    /// Lets the loader's worker return and its delivery land on the main actor.
    private func drain(_ loader: ThumbnailLoader) async throws {
        let deadline = Date().addingTimeInterval(5)
        while Date() < deadline, loader.queueSnapshot.active + loader.queueSnapshot.pending > 0 {
            try await Task.sleep(nanoseconds: 20_000_000)
        }
        try await Task.sleep(nanoseconds: 50_000_000)
    }
}
