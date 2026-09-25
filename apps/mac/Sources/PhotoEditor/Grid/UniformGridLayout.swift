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
        case .horizontal:
            let h = max(vp.height - inset.top - inset.bottom, 20)
            cellSize = NSSize(width: floor(h / heightRatio), height: h)
            columns = max(count, 1)
            lastExtent = vp.height
        }
    }

    override var collectionViewContentSize: NSSize {
        let vp = viewport
        switch axis {
        case .vertical:
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

    override func shouldInvalidateLayout(forBoundsChange newBounds: NSRect) -> Bool {
        switch axis {
        case .vertical: newBounds.width != lastExtent
        case .horizontal: newBounds.height != lastExtent
        }
    }
}
