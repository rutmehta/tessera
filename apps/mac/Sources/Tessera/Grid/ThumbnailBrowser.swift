import AppKit
import TesseraCore
import QuartzCore
import SwiftUI

/// SwiftUI host for the AppKit grid (`.grid`) and filmstrip (`.filmstrip`).
struct ThumbnailBrowser: NSViewRepresentable {
    let model: AppModel
    let style: CellStyle

    func makeCoordinator() -> BrowserController { BrowserController(model: model, style: style) }
    func makeNSView(context: Context) -> NSScrollView {
        let controller = context.coordinator
        // A library may already be installed (e.g. --stub at launch) before this view existed.
        DispatchQueue.main.async {
            MainActor.assumeIsolated {
                controller.libraryDidReload()
                controller.selectionDidChange(scrollToFocus: true)
            }
        }
        return controller.scrollView
    }
    func updateNSView(_ nsView: NSScrollView, context: Context) {}
}

final class ThumbnailCollectionView: NSCollectionView {
    var onDoubleClick: ((Int) -> Void)?

    override func mouseDown(with event: NSEvent) {
        super.mouseDown(with: event)
        if event.clickCount == 2 {
            let p = convert(event.locationInWindow, from: nil)
            if let ip = indexPathForItem(at: p) { onDoubleClick?(ip.item) }
        }
    }

    // Culling keys are routed by KeyRouter; do not let NSCollectionView type-select or beep.
    override func keyDown(with event: NSEvent) { interpretKeyEvents([]) }
    override func doCommand(by selector: Selector) {}
}

/// Data source, delegate and model observer for one NSCollectionView.
@MainActor
final class BrowserController: NSObject, NSCollectionViewDataSource, NSCollectionViewDelegate, LibraryObserver, ScrollBenchmarkRunner {
    let model: AppModel
    let style: CellStyle
    let scrollView = NSScrollView()
    let collectionView = ThumbnailCollectionView()
    let layout: UniformGridLayout
    private var focusedCellPosition: Int?

    init(model: AppModel, style: CellStyle) {
        self.model = model
        self.style = style
        layout = UniformGridLayout(axis: style == .grid ? .vertical : .horizontal)
        super.init()

        switch style {
        case .grid:
            layout.targetWidth = model.thumbnailSize
            layout.heightRatio = 0.72
            layout.extraHeight = style.captionHeight
            layout.onColumnsChange = { [weak model] cols in model?.gridColumns = cols }
        case .filmstrip:
            layout.heightRatio = 0.72
            layout.spacing = 4
            layout.inset = NSEdgeInsets(top: 6, left: 8, bottom: 6, right: 8)
        }

        collectionView.collectionViewLayout = layout
        collectionView.dataSource = self
        collectionView.delegate = self
        collectionView.isSelectable = true
        collectionView.allowsMultipleSelection = true
        collectionView.allowsEmptySelection = true
        collectionView.backgroundColors = [.clear]
        collectionView.register(ThumbnailCell.self, forItemWithIdentifier: ThumbnailCell.identifier)
        collectionView.onDoubleClick = { [weak model] p in
            model?.select(position: p)
            model?.viewMode = .loupe
        }

                collectionView.autoresizingMask = style == .grid ? [.width] : []
        scrollView.documentView = collectionView
        scrollView.hasVerticalScroller = style == .grid
        scrollView.hasHorizontalScroller = style == .filmstrip
        scrollView.autohidesScrollers = true
        scrollView.drawsBackground = true
        scrollView.backgroundColor = Theme.gridBackground
        scrollView.scrollerStyle = .overlay
        scrollView.setAccessibilityIdentifier(style == .grid ? "grid" : "filmstrip")

        model.addObserver(self)
    }

    // MARK: Data source

    func collectionView(_ collectionView: NSCollectionView, numberOfItemsInSection section: Int) -> Int {
        model.visibleCount
    }

    func collectionView(_ collectionView: NSCollectionView, itemForRepresentedObjectAt indexPath: IndexPath) -> NSCollectionViewItem {
        let cell = collectionView.makeItem(withIdentifier: ThumbnailCell.identifier, for: indexPath) as! ThumbnailCell
        let p = indexPath.item
        let item = model.item(at: p)
        cell.configure(item: item, state: model.state(at: p), status: model.status(at: p),
                       basketTarget: model.basketTarget, suggestedBest: model.isSuggestedBest(item),
                       groupIndex: model.indexInGroup(of: item), groupSize: model.groupSize(of: item),
                       focused: p == model.focus, style: style, loader: model.loader)
        return cell
    }

    // MARK: Delegate (mouse selection)

    func collectionView(_ collectionView: NSCollectionView, didSelectItemsAt indexPaths: Set<IndexPath>) {
        syncSelectionFromView(clicked: indexPaths.count == 1 ? indexPaths.first?.item : nil)
    }

    func collectionView(_ collectionView: NSCollectionView, didDeselectItemsAt indexPaths: Set<IndexPath>) {
        syncSelectionFromView(clicked: nil)
    }

    private func syncSelectionFromView(clicked: Int?) {
        var set = IndexSet()
        for ip in collectionView.selectionIndexPaths { set.insert(ip.item) }
        model.setSelectionFromUI(set, clicked: clicked)
    }

    // MARK: LibraryObserver

    func libraryDidReload() {
        collectionView.reloadData()
        collectionView.layoutSubtreeIfNeeded()
        syncDocumentSize()
        focusedCellPosition = nil
        scrollView.contentView.scroll(to: .zero)
        scrollView.reflectScrolledClipView(scrollView.contentView)
    }

    func itemsDidChange(_ positions: IndexSet) {
        for p in positions {
            guard let cell = collectionView.item(at: IndexPath(item: p, section: 0)) as? ThumbnailCell else { continue }
            cell.update(state: model.state(at: p), status: model.status(at: p), basketTarget: model.basketTarget)
        }
    }

    func selectionDidChange(scrollToFocus: Bool) {
        let target = Set(model.selection.map { IndexPath(item: $0, section: 0) })
        if collectionView.selectionIndexPaths != target {
            collectionView.selectionIndexPaths = target
        }
        if focusedCellPosition != model.focus {
            if let old = focusedCellPosition, let cell = collectionView.item(at: IndexPath(item: old, section: 0)) as? ThumbnailCell {
                cell.setFocused(false)
            }
            if let f = model.focus, let cell = collectionView.item(at: IndexPath(item: f, section: 0)) as? ThumbnailCell {
                cell.setFocused(true)
            }
            focusedCellPosition = model.focus
        }
        if scrollToFocus, let f = model.focus, f < model.visibleCount {
            scrollToVisible(f)
        }
    }

    func thumbnailSizeDidChange() {
        guard style == .grid else { return }
        layout.targetWidth = model.thumbnailSize
        layout.invalidateLayout()
        if let f = model.focus {
            collectionView.layoutSubtreeIfNeeded()
            scrollToVisible(f, center: true)
        }
    }

    /// NSCollectionView only grows its frame along the vertical axis on its own; make sure the
    /// document view matches the layout in both axes (the horizontal filmstrip needs this).
    private func syncDocumentSize() {
        let content = layout.collectionViewContentSize
        if collectionView.frame.size != content { collectionView.setFrameSize(content) }
    }

    private func scrollToVisible(_ position: Int, center: Bool = false) {
        guard scrollView.window != nil else { return }
        collectionView.layoutSubtreeIfNeeded()
        syncDocumentSize()
        let frame = layout.frame(for: position)
        let clip = scrollView.contentView
        let visible = clip.bounds
        var origin = visible.origin
        // The clip view may extend under the toolbar; its top content inset is not visible.
        let insetTop = clip.contentInsets.top
        let insetBottom = clip.contentInsets.bottom
        switch style {
        case .grid:
            let margin: CGFloat = layout.spacing + 4
            let top = visible.minY + insetTop
            let bottom = visible.maxY - insetBottom
            if center {
                origin.y = frame.midY - (top + bottom) / 2 + visible.minY
            } else if frame.minY - margin < top {
                origin.y = frame.minY - margin - insetTop
            } else if frame.maxY + margin > bottom {
                origin.y = frame.maxY + margin - visible.height + insetBottom
            } else { return }
            origin.y = min(max(origin.y, -insetTop), max(collectionView.frame.height - visible.height + insetBottom, -insetTop))
        case .filmstrip:
            // Keep the focused frame centred in the strip, like Capture One's browser.
            origin.x = frame.midX - visible.width / 2
            origin.x = min(max(origin.x, 0), max(collectionView.frame.width - visible.width, 0))
            if abs(origin.x - visible.minX) < 1 { return }
        }
        clip.scroll(to: origin)
        scrollView.reflectScrolledClipView(clip)
    }

    // MARK: Scroll benchmark (Debug menu)

    private var benchLink: CADisplayLink?
    private var benchStart: CFTimeInterval = 0
    private var benchLast: CFTimeInterval = 0
    private var benchIntervals: [Double] = []
    private var benchWork: [Double] = []
    private var benchSpeed: CGFloat = 0
    private var benchRefresh: Double = 1.0 / 60

    /// Scrolls the grid top-to-bottom at a fixed speed for 8 s on the display link and reports
    /// frame intervals and main-thread layout cost per frame.
    func runScrollBenchmark() {
        guard style == .grid, benchLink == nil, model.visibleCount > 0 else { return }
        let maxY = max(collectionView.frame.height - scrollView.contentView.bounds.height, 0)
        benchSpeed = min(max(maxY / 8, 2000), 15000)   // points per second
        benchIntervals.removeAll(keepingCapacity: true)
        benchWork.removeAll(keepingCapacity: true)
        benchStart = 0
        scrollView.contentView.scroll(to: .zero)
        let link = collectionView.displayLink(target: self, selector: #selector(benchTick(_:)))
        link.add(to: .main, forMode: .common)
        benchLink = link
        model.statusMessage = "Scroll benchmark running…"
    }

    @objc private func benchTick(_ link: CADisplayLink) {
        let now = link.timestamp
        if benchStart == 0 {
            benchStart = now
            benchLast = now
            benchRefresh = max(link.targetTimestamp - link.timestamp, 1.0 / 240)
            return
        }
        benchIntervals.append(now - benchLast)
        benchLast = now
        let t0 = CACurrentMediaTime()
        let maxY = max(collectionView.frame.height - scrollView.contentView.bounds.height, 0)
        let y = min(CGFloat(now - benchStart) * benchSpeed, maxY)
        scrollView.contentView.scroll(to: NSPoint(x: 0, y: y))
        scrollView.reflectScrolledClipView(scrollView.contentView)
        collectionView.layoutSubtreeIfNeeded()
        collectionView.displayIfNeeded()
        benchWork.append(CACurrentMediaTime() - t0)
        if now - benchStart >= 8 || y >= maxY { finishBenchmark() }
    }

    private func finishBenchmark() {
        benchLink?.invalidate()
        benchLink = nil
        guard !benchIntervals.isEmpty else { return }
        let sorted = benchIntervals.sorted()
        let avg = benchIntervals.reduce(0, +) / Double(benchIntervals.count)
        let p99 = sorted[min(sorted.count - 1, Int(Double(sorted.count) * 0.99))]
        let slow = benchIntervals.filter { $0 > 1.0 / 60 * 1.25 }.count   // frames that missed 60 fps
        let work = benchWork.sorted()
        let workP99 = work[min(work.count - 1, Int(Double(work.count) * 0.99))]
        let fps = 1 / avg
        let verdict = slow <= max(1, benchIntervals.count / 100) ? "PASS" : "FAIL"
        let msg = String(format: "Scroll benchmark %@: %@ items, %d frames, %.0f fps avg, p99 frame %.1f ms, %d frames slower than 60 fps, layout p99 %.2f ms",
                         verdict, model.visibleCount.formatted(), benchIntervals.count, fps, p99 * 1000, slow, workP99 * 1000)
        model.statusMessage = msg
        NSLog("%@", msg)
    }
}
