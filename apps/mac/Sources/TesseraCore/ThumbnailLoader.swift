import CoreGraphics
import CoreText
import Foundation
import ImageIO

/// Two preview tiers served from the embedded camera JPEG via ImageIO (docs/06 §3 "instant previews",
/// docs/08 "embedded preview fast path"). Stand-in for `crates/previews`.
public enum PreviewTier: Sendable {
    /// Grid / filmstrip cells.
    case thumbnail
    /// Loupe first paint.
    case preview

    public var maxPixelSize: Int {
        switch self {
        case .thumbnail: 384
        case .preview: 2560
        }
    }
}

/// A cancellable in-flight request. Cells cancel on reuse so fast scrolling never queues stale work.
public final class PreviewRequest: @unchecked Sendable {
    fileprivate let operation: BlockOperation
    fileprivate init(_ op: BlockOperation) { operation = op }
    public func cancel() { operation.cancel() }
    public var isCancelled: Bool { operation.isCancelled }
}

/// Memory-bounded cache + bounded-concurrency decode queue.
public final class ThumbnailLoader: @unchecked Sendable {
    private final class Box { let image: CGImage; init(_ i: CGImage) { image = i } }
    private struct Key: Hashable { let item: Int; let tier: PreviewTier; let generation: Int }

    private let thumbCache = NSCache<NSNumber, Box>()
    private let previewCache = NSCache<NSNumber, Box>()
    private let queue: OperationQueue

    public init() {
        thumbCache.totalCostLimit = 512 << 20   // bytes
        previewCache.totalCostLimit = 768 << 20
        queue = OperationQueue()
        queue.name = "thumbnails"
        queue.qualityOfService = .userInitiated
        queue.maxConcurrentOperationCount = max(2, ProcessInfo.processInfo.activeProcessorCount - 1)
    }

    private func cache(_ tier: PreviewTier) -> NSCache<NSNumber, Box> {
        tier == .thumbnail ? thumbCache : previewCache
    }

    public func removeAll() {
        queue.cancelAllOperations()
        thumbCache.removeAllObjects()
        previewCache.removeAllObjects()
    }

    public func cached(_ item: PhotoItem, tier: PreviewTier) -> CGImage? {
        cache(tier).object(forKey: NSNumber(value: item.id))?.image
    }

    /// Loads asynchronously; `completion` runs on the main thread (not called if cancelled).
    public func request(_ item: PhotoItem, tier: PreviewTier, priority: Operation.QueuePriority = .normal,
                        completion: @escaping @MainActor @Sendable (CGImage) -> Void) -> PreviewRequest? {
        if let hit = cached(item, tier: tier) {
            MainActor.assumeIsolated { completion(hit) }
            return nil
        }
        let op = BlockOperation()
        let request = PreviewRequest(op)
        nonisolated(unsafe) let cache = cache(tier)  // NSCache is thread-safe
        op.addExecutionBlock { [weak op] in
            guard let op, !op.isCancelled else { return }
            guard let image = Self.render(item, tier: tier) else { return }
            cache.setObject(Box(image), forKey: NSNumber(value: item.id), cost: image.bytesPerRow * image.height)
            guard !op.isCancelled else { return }
            DispatchQueue.main.async {
                guard !request.isCancelled else { return }
                MainActor.assumeIsolated { completion(image) }
            }
        }
        op.queuePriority = priority
        queue.addOperation(op)
        return request
    }

    // MARK: Rendering

    public static func render(_ item: PhotoItem, tier: PreviewTier) -> CGImage? {
        if item.kind == .synthetic {
            return SyntheticThumbnail.make(for: item, maxPixel: tier == .thumbnail ? 256 : 1600)
        }
        guard let url = item.url else { return nil }
        let srcOpts = [kCGImageSourceShouldCache: false] as CFDictionary
        guard let src = CGImageSourceCreateWithURL(url as CFURL, srcOpts) else { return nil }
        // Thumbnail tier: embedded thumbnail if present. Preview tier: for RAW, ImageIO's thumbnail
        // path uses the largest embedded JPEG; for JPEG it is a DCT-scaled decode. Never a full raw develop.
        let opts: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageIfAbsent: true,
            kCGImageSourceCreateThumbnailFromImageAlways: tier == .preview && item.kind != .raw,
            kCGImageSourceThumbnailMaxPixelSize: tier.maxPixelSize,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceShouldCacheImmediately: true,
        ]
        if let img = CGImageSourceCreateThumbnailAtIndex(src, 0, opts as CFDictionary) {
            // Some RAWs carry only a 160 px thumbnail; for the loupe, fall back to a scaled decode.
            if tier == .preview, item.kind == .raw, max(img.width, img.height) < 1024 {
                var always = opts
                always[kCGImageSourceCreateThumbnailFromImageAlways] = true
                return CGImageSourceCreateThumbnailAtIndex(src, 0, always as CFDictionary) ?? img
            }
            return img
        }
        return nil
    }
}

/// Procedural thumbnails for synthetic items: a per-group hue gradient with the frame number,
/// cheap enough (~0.1 ms) that 20k items scroll without a disk cache.
public enum SyntheticThumbnail {
    public static func make(for item: PhotoItem, maxPixel: Int) -> CGImage? {
        let aspect = item.aspectRatio
        let w = aspect >= 1 ? maxPixel : Int(Double(maxPixel) * aspect)
        let h = aspect >= 1 ? Int(Double(maxPixel) / aspect) : maxPixel
        guard let cs = CGColorSpace(name: CGColorSpace.sRGB),
              let ctx = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0, space: cs,
                                  bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue)
        else { return nil }

        let hue = CGFloat((item.groupID &* 47) % 360) / 360
        let jitter = CGFloat(item.seed % 1000) / 1000
        let top = hsb(hue, 0.45, 0.55 + 0.15 * jitter)
        let bottom = hsb(fmod(hue + 0.08, 1), 0.65, 0.22)
        let gradient = CGGradient(colorsSpace: cs, colors: [top, bottom] as CFArray, locations: [0, 1])!
        ctx.drawLinearGradient(gradient, start: CGPoint(x: 0, y: h), end: CGPoint(x: CGFloat(w) * jitter, y: 0), options: [])

        // "Horizon" and "sun" so frames within a group look like near-duplicates.
        ctx.setFillColor(hsb(fmod(hue + 0.5, 1), 0.3, 0.9, alpha: 0.85))
        let r = CGFloat(min(w, h)) * 0.12
        let cx = CGFloat(w) * (0.25 + 0.5 * CGFloat((item.seed >> 16) % 100) / 100)
        ctx.fillEllipse(in: CGRect(x: cx - r, y: CGFloat(h) * 0.6 - r, width: 2 * r, height: 2 * r))
        ctx.setFillColor(CGColor(gray: 0, alpha: 0.35))
        ctx.fill(CGRect(x: 0, y: 0, width: CGFloat(w), height: CGFloat(h) * 0.3))

        let text = "\(item.id + 1)" as CFString
        let font = CTFontCreateWithName("Helvetica Neue Medium" as CFString, CGFloat(h) * 0.16, nil)
        let attrs = [kCTFontAttributeName: font,
                     kCTForegroundColorAttributeName: CGColor(gray: 1, alpha: 0.85)] as CFDictionary
        let line = CTLineCreateWithAttributedString(CFAttributedStringCreate(nil, text, attrs))
        let bounds = CTLineGetBoundsWithOptions(line, .useOpticalBounds)
        ctx.textPosition = CGPoint(x: (CGFloat(w) - bounds.width) / 2, y: CGFloat(h) * 0.08)
        CTLineDraw(line, ctx)
        return ctx.makeImage()
    }

    private static func hsb(_ h: CGFloat, _ s: CGFloat, _ b: CGFloat, alpha: CGFloat = 1) -> CGColor {
        // HSB -> RGB
        let i = Int(h * 6) % 6
        let f = h * 6 - floor(h * 6)
        let p = b * (1 - s), q = b * (1 - f * s), t = b * (1 - (1 - f) * s)
        let (r, g, bl): (CGFloat, CGFloat, CGFloat) = switch i {
        case 0: (b, t, p)
        case 1: (q, b, p)
        case 2: (p, b, t)
        case 3: (p, q, b)
        case 4: (t, p, b)
        default: (b, p, q)
        }
        return CGColor(srgbRed: r, green: g, blue: bl, alpha: alpha)
    }
}
