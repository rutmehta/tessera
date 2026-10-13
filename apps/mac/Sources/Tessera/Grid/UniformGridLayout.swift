import AppKit

/// Arithmetic grid / strip layout: every query is O(visible items), so 20k+ items cost nothing
/// until they scroll into view (NSCollectionViewFlowLayout precomputes attributes for all items).
///
/// It subclasses NSCollectionViewFlowLayout only so NSCollectionView sees `scrollDirection`
/// (it will not grow its frame horizontally for a plain NSCollectionViewLayout); every layout
/// query is overridden and the flow layout's O(n) `prepare` is never run.
@MainActor
final class UniformGridLayout: NSCollectionViewFlowLayout {
    enum Axis { case vertical, horizontal }

    let axis: Axis
    /// Vertical: target cell width (cells stretch to fill the row). Horizontal: ignored (height-driven).
    var targetWidth: CGFloat = 176
    /// Cell height = width * heightRatio + extraHeight (vertical); width = height / heightRatio (horizontal).
    var heightRatio: CGFloat = 0.78
    var extraHeight: CGFloat = 0
    var spacing: CGFloat = 6
    var inset = NSEdgeInsets(top: 10, left: 10, bottom: 10, right: 10)

    private(set) var columns = 1
    private(set) var cellSize = NSSize(width: 1, height: 1)
    private var count = 0
    private var lastExtent: CGFloat = -1

    var onColumnsChange: ((Int) -> Void)?
    var onViewportChange: (() -> Void)?

    /// UX-14 grouped grid (vertical only): one block per burst, each starting on a new row under a
    /// header, then one block of every single frame. `groupOf` gives a position's group (nil for a
    /// single); blocks are rebuilt in `prepare`, O(n) once per layout, and every query stays
    /// O(visible) through binary search over the block origins.
    static let headerKind = "burst-row-header"
    var groupOf: ((Int) -> Int?)?
    var grouped = false { didSet { if grouped != oldValue { invalidateLayout() } } }
    var headerHeight: CGFloat = 24
    /// A block: its first position, item count, the first row's y and whether it is the singles block.
    struct Block { let start: Int; let count: Int; let y: CGFloat; let singles: Bool }
    private(set) var blocks: [Block] = []
    /// Position → block index (grouped only).
    private var blockOf: [Int32] = []
    private var contentHeight: CGFloat = 0

    var isGrouped: Bool { grouped && axis == .vertical }

    private func blockHeight(_ count: Int) -> CGFloat {
        let rows = (count + columns - 1) / columns
        return headerHeight + CGFloat(rows) * cellSize.height + CGFloat(max(rows - 1, 0)) * spacing + spacing
    }

    private func rebuildBlocks() {
        blocks.removeAll(keepingCapacity: true)
        blockOf = Array(repeating: -1, count: count)
        guard let groupOf, count > 0 else { contentHeight = inset.top + inset.bottom; return }
        var y = inset.top
        var singles: [Int] = []
        var p = 0
        while p < count {
            guard let g = groupOf(p) else { singles.append(p); p += 1; continue }
            var end = p + 1
            while end < count, groupOf(end) == g { end += 1 }
            let n = end - p
            if n == 1 { singles.append(p); p = end; continue }
            for q in p..<end { blockOf[q] = Int32(blocks.count) }
            blocks.append(Block(start: p, count: n, y: y, singles: false))
            y += blockHeight(n)
            p = end
        }
        if !singles.isEmpty {
            // Singles keep their capture order but lay out as one block: position → slot via the
            // block's start list (`singleSlots`).
            singleSlots = singles
            slotOfPosition = Dictionary(uniqueKeysWithValues: singles.enumerated().map { ($1, $0) })
            for q in singles { blockOf[q] = Int32(blocks.count) }
            blocks.append(Block(start: singles[0], count: singles.count, y: y, singles: true))
            y += blockHeight(singles.count)
        } else {
            singleSlots = []
            slotOfPosition = [:]
        }
        contentHeight = y - spacing + inset.bottom
    }
    /// The singles block's positions in order, and position → slot within it.
    private(set) var singleSlots: [Int] = []
    private var slotOfPosition: [Int: Int] = [:]

    /// The block a position belongs to (grouped only).
    func block(of position: Int) -> Int? {
        guard isGrouped, blockOf.indices.contains(position), blockOf[position] >= 0 else { return nil }
        return Int(blockOf[position])
    }

    /// Row header frame of block `b`.
    func headerFrame(block b: Int) -> NSRect {
        let block = blocks[b]
        return NSRect(x: inset.left, y: block.y, width: max(cellSize.width * CGFloat(columns) + spacing * CGFloat(columns - 1), 1), height: headerHeight)
    }

    private func groupedFrame(for index: Int) -> NSRect {
        guard let b = block(of: index) else { return .zero }
        let block = blocks[b]
        let slot = block.singles ? (slotOfPosition[index] ?? 0) : index - block.start
        let row = slot / columns, col = slot % columns
        return NSRect(x: inset.left + CGFloat(col) * (cellSize.width + spacing),
                      y: block.y + headerHeight + CGFloat(row) * (cellSize.height + spacing),
                      width: cellSize.width, height: cellSize.height)
    }

    init(axis: Axis) {
        self.axis = axis
        super.init()
        scrollDirection = axis == .vertical ? .vertical : .horizontal
    }

    required init?(coder: NSCoder) { fatalError() }

    private var viewport: NSRect {
        collectionView?.enclosingScrollView?.contentView.bounds ?? collectionView?.bounds ?? .zero
    }

    override func prepare() {
        // Deliberately not calling super.prepare() (flow layout would compute all n attributes).
        count = collectionView?.numberOfSections ?? 0 > 0 ? (collectionView?.numberOfItems(inSection: 0) ?? 0) : 0
        let vp = viewport
        switch axis {
        case .vertical:
            let avail = max(vp.width - inset.left - inset.right, 40)
            let cols = max(1, Int((avail + spacing) / (targetWidth + spacing)))
            let w = floor((avail - spacing * CGFloat(cols - 1)) / CGFloat(cols))
            cellSize = NSSize(width: w, height: floor(w * heightRatio + extraHeight))
            if cols != columns { columns = cols; onColumnsChange?(cols) }
            lastExtent = vp.width
            if isGrouped { rebuildBlocks() } else { blocks = []; blockOf = [] }
        case .horizontal:
            let h = max(vp.height - inset.top - inset.bottom, 20)
            cellSize = NSSize(width: floor(h / heightRatio), height: h)
            columns = max(count, 1)
            lastExtent = vp.height
        }
        onViewportChange?()
    }

    override var collectionViewContentSize: NSSize {
        let vp = viewport
        switch axis {
        case .vertical:
            if isGrouped { return NSSize(width: vp.width, height: max(contentHeight, vp.height)) }
            let rows = (count + columns - 1) / columns
            let h = inset.top + inset.bottom + CGFloat(rows) * cellSize.height + CGFloat(max(rows - 1, 0)) * spacing
            return NSSize(width: vp.width, height: max(h, vp.height))
        case .horizontal:
            let w = inset.left + inset.right + CGFloat(count) * cellSize.width + CGFloat(max(count - 1, 0)) * spacing
            return NSSize(width: max(w, vp.width), height: vp.height)
        }
    }

    func frame(for index: Int) -> NSRect {
        switch axis {
        case .vertical:
            if isGrouped { return groupedFrame(for: index) }
            let row = index / columns, col = index % columns
            return NSRect(x: inset.left + CGFloat(col) * (cellSize.width + spacing),
                          y: inset.top + CGFloat(row) * (cellSize.height + spacing),
                          width: cellSize.width, height: cellSize.height)
        case .horizontal:
            return NSRect(x: inset.left + CGFloat(index) * (cellSize.width + spacing), y: inset.top,
                          width: cellSize.width, height: cellSize.height)
        }
    }

    private func attributes(_ index: Int) -> NSCollectionViewLayoutAttributes {
        let a = NSCollectionViewLayoutAttributes(forItemWith: IndexPath(item: index, section: 0))
        a.frame = frame(for: index)
        return a
    }

    override func layoutAttributesForElements(in rect: NSRect) -> [NSCollectionViewLayoutAttributes] {
        guard count > 0 else { return [] }
        if isGrouped { return groupedAttributes(in: rect) }
        let range: ClosedRange<Int>
        switch axis {
        case .vertical:
            let pitch = cellSize.height + spacing
            let firstRow = max(0, Int(floor((rect.minY - inset.top) / pitch)))
            let lastRow = max(firstRow, Int(floor((rect.maxY - inset.top) / pitch)))
            let lo = firstRow * columns
            let hi = min(count - 1, (lastRow + 1) * columns - 1)
            guard lo <= hi else { return [] }
            range = lo...hi
        case .horizontal:
            let pitch = cellSize.width + spacing
            let lo = max(0, Int(floor((rect.minX - inset.left) / pitch)))
            let hi = min(count - 1, Int(floor((rect.maxX - inset.left) / pitch)))
            guard lo <= hi else { return [] }
            range = lo...hi
        }
        return range.map(attributes)
    }

    override func layoutAttributesForItem(at indexPath: IndexPath) -> NSCollectionViewLayoutAttributes? {
        guard indexPath.item < count else { return nil }
        return attributes(indexPath.item)
    }

    /// Grouped: the blocks overlapping `rect` (binary search on their origins), their headers and
    /// the rows of each that overlap.
    private func groupedAttributes(in rect: NSRect) -> [NSCollectionViewLayoutAttributes] {
        guard !blocks.isEmpty else { return [] }
        var lo = 0, hi = blocks.count - 1
        while lo < hi {
            let mid = (lo + hi + 1) / 2
            if blocks[mid].y <= rect.minY { lo = mid } else { hi = mid - 1 }
        }
        var out: [NSCollectionViewLayoutAttributes] = []
        var b = lo
        while b < blocks.count, blocks[b].y < rect.maxY {
            let block = blocks[b]
            let header = headerFrame(block: b)
            if header.intersects(rect) {
                let a = NSCollectionViewLayoutAttributes(forSupplementaryViewOfKind: Self.headerKind, with: IndexPath(item: b, section: 0))
                a.frame = header
                out.append(a)
            }
            let pitch = cellSize.height + spacing
            let top = block.y + headerHeight
            let firstRow = max(0, Int(floor((rect.minY - top) / pitch)))
            let lastRow = max(firstRow, Int(floor((rect.maxY - top) / pitch)))
            let first = firstRow * columns
            let last = min(block.count - 1, (lastRow + 1) * columns - 1)
            if first <= last {
                for slot in first...last {
                    let position = block.singles ? singleSlots[slot] : block.start + slot
                    out.append(attributes(position))
                }
            }
            b += 1
        }
        return out
    }

    override func layoutAttributesForSupplementaryView(ofKind elementKind: NSCollectionView.SupplementaryElementKind,
                                                       at indexPath: IndexPath) -> NSCollectionViewLayoutAttributes? {
        guard isGrouped, elementKind == Self.headerKind, blocks.indices.contains(indexPath.item) else { return nil }
        let a = NSCollectionViewLayoutAttributes(forSupplementaryViewOfKind: elementKind, with: indexPath)
        a.frame = headerFrame(block: indexPath.item)
        return a
    }

    override func shouldInvalidateLayout(forBoundsChange newBounds: NSRect) -> Bool {
        switch axis {
        case .vertical: newBounds.width != lastExtent
        case .horizontal: newBounds.height != lastExtent
        }
    }
}
