import CoreGraphics
import Foundation

/// File ▸ Print… page layout (docs/01 §2.26): single image per page, a contact-sheet grid, or
/// custom fixed-size cells packed onto the page. All geometry is in points (1/72 in) with a
/// top-left origin (flipped, like the print view), so it maps directly to the page drawing and to
/// `pixels(for:dpi:)` for the engine renders.
public struct PrintLayout: Codable, Equatable, Sendable {
    public enum Style: String, Codable, CaseIterable, Sendable, Identifiable {
        case single, contactSheet = "contact_sheet", custom
        public var id: String { rawValue }
        public var title: String {
            switch self {
            case .single: "Single image"
            case .contactSheet: "Contact sheet"
            case .custom: "Custom cells"
            }
        }
    }

    public var style: Style = .single
    /// Contact sheet grid.
    public var rows = 5
    public var columns = 4
    /// Gap between cells, points.
    public var spacing: Double = 9
    /// Custom cell size, points (default 4 × 6 in portrait).
    public var cellWidth: Double = 288
    public var cellHeight: Double = 432
    /// Page margins, points.
    public var marginTop: Double = 36
    public var marginLeft: Double = 36
    public var marginBottom: Double = 36
    public var marginRight: Double = 36
    /// Turn a picture 90° when that makes it larger in its cell.
    public var rotateToFit = true
    /// Contact sheets: file name under each picture.
    public var captions = true
    /// Caption band height, points (contact sheets with captions).
    public static let captionHeight: Double = 12

    public init() {}

    /// The printable content rectangle of a page.
    public func contentRect(page: CGSize) -> CGRect {
        CGRect(x: marginLeft, y: marginTop,
               width: max(page.width - marginLeft - marginRight, 0),
               height: max(page.height - marginTop - marginBottom, 0))
    }

    /// Cell rectangles on one page (reading order). Each cell holds one picture (and its caption
    /// band below, for captioned contact sheets). Empty when the margins leave no room.
    public func cells(page: CGSize) -> [CGRect] {
        let content = contentRect(page: page)
        guard content.width >= 1, content.height >= 1 else { return [] }
        switch style {
        case .single:
            return [content]
        case .contactSheet:
            let (r, c) = (max(rows, 1), max(columns, 1))
            let gap = max(spacing, 0)
            let w = (content.width - gap * Double(c - 1)) / Double(c)
            let h = (content.height - gap * Double(r - 1)) / Double(r)
            guard w >= 1, h >= 1 else { return [] }
            return (0..<r).flatMap { row in
                (0..<c).map { col in
                    CGRect(x: content.minX + Double(col) * (w + gap), y: content.minY + Double(row) * (h + gap),
                           width: w, height: h)
                }
            }
        case .custom:
            let gap = max(spacing, 0)
            let (w, h) = (cellWidth, cellHeight)
            guard w >= 1, h >= 1, w <= content.width + 0.001, h <= content.height + 0.001 else { return [] }
            let c = Int(((content.width + gap) / (w + gap)).rounded(.down) + 0.0001)
            let r = Int(((content.height + gap) / (h + gap)).rounded(.down) + 0.0001)
            // Centre the packed block on the page.
            let blockW = Double(c) * w + Double(c - 1) * gap, blockH = Double(r) * h + Double(r - 1) * gap
            let x0 = content.minX + (content.width - blockW) / 2, y0 = content.minY + (content.height - blockH) / 2
            return (0..<r).flatMap { row in
                (0..<c).map { col in
                    CGRect(x: x0 + Double(col) * (w + gap), y: y0 + Double(row) * (h + gap), width: w, height: h)
                }
            }
        }
    }

    public func cellsPerPage(page: CGSize) -> Int { cells(page: page).count }

    public func pageCount(images: Int, page: CGSize) -> Int {
        let per = cellsPerPage(page: page)
        guard images > 0, per > 0 else { return 0 }
        return (images + per - 1) / per
    }

    /// Where the picture goes inside a cell: the image area (the cell minus a caption band).
    public func imageArea(in cell: CGRect) -> CGRect {
        guard style == .contactSheet, captions, cell.height > Self.captionHeight * 2 else { return cell }
        return CGRect(x: cell.minX, y: cell.minY, width: cell.width, height: cell.height - Self.captionHeight)
    }

    /// Caption rectangle below the picture area, if any.
    public func captionArea(in cell: CGRect) -> CGRect? {
        let area = imageArea(in: cell)
        guard area != cell else { return nil }
        return CGRect(x: cell.minX, y: area.maxY, width: cell.width, height: Self.captionHeight)
    }

    /// One picture placed in a cell: aspect-fit and centred. `rotated` means the picture is drawn
    /// turned 90° clockwise; `size` is then the unrotated picture's size as drawn (its width runs
    /// along the page's vertical axis).
    public struct Placement: Equatable, Sendable {
        /// Drawn bounds on the page (already rotated).
        public var frame: CGRect
        public var rotated: Bool
        /// The picture's own drawn width × height before rotation, points.
        public var pictureSize: CGSize
    }

    public func place(aspect: Double, in area: CGRect) -> Placement {
        let aspect = aspect.isFinite && aspect > 0 ? aspect : 1
        func fit(_ a: Double) -> CGSize {
            let w = min(area.width, area.height * a)
            return CGSize(width: w, height: w / a)
        }
        let upright = fit(aspect)
        let turned = fit(1 / aspect)   // the rotated picture's on-page bounds
        let rotate = rotateToFit && turned.width * turned.height > upright.width * upright.height * 1.0001
        let bounds = rotate ? turned : upright
        let frame = CGRect(x: area.midX - bounds.width / 2, y: area.midY - bounds.height / 2,
                           width: bounds.width, height: bounds.height)
        return Placement(frame: frame, rotated: rotate,
                         pictureSize: rotate ? CGSize(width: bounds.height, height: bounds.width) : bounds)
    }

    /// Engine render box for a placement at `dpi`: the picture's own width × height in pixels.
    public static func pixels(for placement: Placement, dpi: Double) -> (width: UInt32, height: UInt32) {
        let scale = dpi / 72
        return (UInt32(max((placement.pictureSize.width * scale).rounded(), 1)),
                UInt32(max((placement.pictureSize.height * scale).rounded(), 1)))
    }

    /// Pages of image indices in reading order.
    public func pages(images: Int, page: CGSize) -> [[Int]] {
        let per = cellsPerPage(page: page)
        guard per > 0, images > 0 else { return [] }
        return stride(from: 0, to: images, by: per).map { Array($0..<min($0 + per, images)) }
    }
}
