import CoreGraphics
import CoreText
import Foundation
import ImageIO

/// Two preview tiers served by the Rust preview cache, including its scheduled RAW fallback.
public enum PreviewTier: Hashable, Sendable {
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
    private let lock = NSLock()
    private var cancellation: (@Sendable () -> Void)?
    private var cancelled = false
    private var completed = false
    private var waiters: [CheckedContinuation<Void, Never>] = []
    fileprivate init() {}
    fileprivate func onCancel(_ action: @escaping @Sendable () -> Void) {
        let run = lock.withLock {
            if cancelled { return true }
            cancellation = action
            return false
        }
        if run { action() }
    }
    public func cancel() {
        let action = lock.withLock {
            cancelled = true
            let action = cancellation
            cancellation = nil
            return action
        }
        // Never call the loader while holding the subscriber lock.
        action?()
        finish()
    }
    fileprivate func finish() {
        let pending = lock.withLock {
            completed = true
            cancellation = nil
            let pending = waiters
            waiters.removeAll()
            return pending
        }
        for waiter in pending { waiter.resume() }
    }
    public var isCancelled: Bool { lock.withLock { cancelled } }

    /// Waits until this subscriber has delivered or has been fully detached.
    public func waitForCompletion() async {
        await withCheckedContinuation { continuation in
            let done = lock.withLock {
                if completed { return true }
                waiters.append(continuation)
                return false
            }
            if done { continuation.resume() }
        }
    }
}

/// Memory-bounded cache + bounded-concurrency decode queue.
public final class ThumbnailLoader: @unchecked Sendable {
    /// Explicit cost-bounded LRU, protected by the loader lock. NSCache can evict
    /// a just-inserted image under pressure before its ready callback is delivered.
    /// Keep the newest image even when it alone exceeds the budget.
    private final class Cache {
        struct Entry {
            let image: CGImage
            let cost: Int
            var access: UInt64
        }
        var totalCostLimit = 0
        private var entries: [AnyHashable: Entry] = [:]
        private var cost = 0
        private var clock: UInt64 = 0

        func image(for item: PhotoItem) -> CGImage? {
            let key = ThumbnailLoader.key(item)
            guard var entry = entries[key] else { return nil }
            clock &+= 1
            entry.access = clock
            entries[key] = entry
            return entry.image
        }

        func insert(_ image: CGImage, for item: PhotoItem) {
            remove(item)
            clock &+= 1
            let bytes = image.bytesPerRow * image.height
            entries[ThumbnailLoader.key(item)] = Entry(image: image, cost: bytes, access: clock)
            cost += bytes
            while cost > totalCostLimit && entries.count > 1 {
                guard let oldest = entries.min(by: { $0.value.access < $1.value.access })?.key else { break }
                if let entry = entries.removeValue(forKey: oldest) { cost -= entry.cost }
            }
        }

        func remove(_ item: PhotoItem) {
            if let entry = entries.removeValue(forKey: ThumbnailLoader.key(item)) { cost -= entry.cost }
        }

        func removeAllObjects() {
            entries.removeAll()
            cost = 0
        }
    }

    private let thumbCache = Cache()
    private let previewCache = Cache()
    private let queue: OperationQueue
    private let lock = NSLock()
    private struct FlightKey: Hashable {
        let image: AnyHashable
        let tier: PreviewTier
    }
    private struct Subscriber {
        let request: PreviewRequest
        let completion: @MainActor @Sendable (CGImage) -> Void
    }
    private final class Flight: @unchecked Sendable {
        let id = UUID()
        let key: FlightKey
        let item: PhotoItem
        let priority: Operation.QueuePriority
        var subscribers: [UUID: Subscriber] = [:]
        var task: Task<Void, Never>?
        // Retain a delivered result until cleanup so a late cache-miss subscriber
        // cannot join a flight whose callback snapshot has already been drained.
        var image: CGImage?
        init(item: PhotoItem, tier: PreviewTier, priority: Operation.QueuePriority) {
            self.item = item
            self.key = FlightKey(image: ThumbnailLoader.key(item), tier: tier)
            self.priority = priority
        }
    }
    private var flights: [FlightKey: Flight] = [:]
    private var pending: [Flight] = []
    private var running: [UUID: Flight] = [:]
    private let concurrency: Int
    private let afterDelivery: (@Sendable () -> Void)?
    private var viewports: [UUID: Int] = [:]

    /// Independent grid/filmstrip owners contribute their current on-screen capacity.
    public func setViewportCapacity(_ capacity: Int, owner: UUID) {
        lock.withLock { viewports[owner] = max(0, capacity) }
        trimPending()
    }

    public func removeViewport(owner: UUID) {
        _ = lock.withLock { viewports.removeValue(forKey: owner) }
        trimPending()
    }

    struct QueueSnapshot {
        let active: Int
        let pending: Int
        let subscribers: Int
        let pendingLimit: Int
    }

    var queueSnapshot: QueueSnapshot {
        lock.withLock {
            QueueSnapshot(active: running.count, pending: pending.count,
                          subscribers: flights.values.reduce(0) { $0 + $1.subscribers.count },
                          pendingLimit: pendingLimit)
        }
    }

    private var pendingLimit: Int { 2 * (viewports.isEmpty ? 32 : viewports.values.reduce(0, +)) }

    private func trimPending() {
        let removed: [Subscriber] = lock.withLock {
            var removed: [Subscriber] = []
            // Cap speculative loupe neighbours as well as the aggregate viewport backlog.
            for tier in [PreviewTier.thumbnail, .preview] {
                let limit = tier == .thumbnail ? pendingLimit : 2
                while pending.filter({ $0.key.tier == tier }).count > limit {
                    guard let index = pending.firstIndex(where: { $0.key.tier == tier }) else { break }
                    let flight = pending.remove(at: index)
                    flights.removeValue(forKey: flight.key)
                    removed.append(contentsOf: flight.subscribers.values)
                    flight.subscribers.removeAll()
                }
            }
            // A loupe can be visible with the retained grid hidden and no filmstrip.
            // Its single-photo viewport still needs its two pending slots.
            while pending.count > max(2, pendingLimit) {
                let index = pending.firstIndex(where: { $0.key.tier == .thumbnail }) ?? 0
                let flight = pending.remove(at: index)
                flights.removeValue(forKey: flight.key)
                removed.append(contentsOf: flight.subscribers.values)
                flight.subscribers.removeAll()
            }
            return removed
        }
        for subscriber in removed { subscriber.request.cancel() }
    }

    public convenience init() {
        self.init(thumbnailCostLimit: 512 << 20, previewCostLimit: 768 << 20)
    }

    init(thumbnailCostLimit: Int, previewCostLimit: Int, concurrency: Int = 4,
         afterDelivery: (@Sendable () -> Void)? = nil) {
        self.concurrency = max(1, concurrency)
        self.afterDelivery = afterDelivery
        thumbCache.totalCostLimit = thumbnailCostLimit
        previewCache.totalCostLimit = previewCostLimit
        queue = OperationQueue()
        queue.name = "thumbnails"
        queue.qualityOfService = .userInitiated
        queue.maxConcurrentOperationCount = max(1, concurrency)
    }

    static func key(_ item: PhotoItem) -> AnyHashable {
        item.engineImage.map(AnyHashable.init) ?? AnyHashable(item)
    }

    private func cache(_ tier: PreviewTier) -> Cache {
        tier == .thumbnail ? thumbCache : previewCache
    }

    public func removeAll() {
        discard { _ in true }
        lock.withLock {
            thumbCache.removeAllObjects()
            previewCache.removeAllObjects()
        }
    }

    deinit { removeAll() }

    public func invalidate(_ item: PhotoItem) {
        discard { $0.key.image == Self.key(item) }
        lock.withLock {
            thumbCache.remove(item)
            previewCache.remove(item)
        }
    }

    private func discard(where matches: (Flight) -> Bool) {
        let subscribers: [Subscriber] = lock.withLock {
            let removed = flights.values.filter(matches)
            for flight in removed {
                flights.removeValue(forKey: flight.key)
                flight.task?.cancel()
            }
            pending.removeAll(where: matches)
            return removed.flatMap { $0.subscribers.values }
        }
        for subscriber in subscribers { subscriber.request.cancel() }
    }

    public func cached(_ item: PhotoItem, tier: PreviewTier) -> CGImage? {
        lock.withLock {
            cache(tier).image(for: item) ?? flights[FlightKey(image: Self.key(item), tier: tier)]?.image
        }
    }

    @MainActor public func request(_ item: PhotoItem, tier: PreviewTier, priority: Operation.QueuePriority = .normal,
                        completion: @escaping @MainActor @Sendable (CGImage) -> Void) -> PreviewRequest? {
        if let hit = cached(item, tier: tier) {
            completion(hit)
            return nil
        }
        let request = PreviewRequest()
        let id = UUID()
        let flight = lock.withLock {
            let key = FlightKey(image: Self.key(item), tier: tier)
            let flight: Flight
            if let existing = flights[key] { flight = existing }
            else {
                flight = Flight(item: item, tier: tier, priority: priority)
                flights[key] = flight
                pending.append(flight)
            }
            flight.subscribers[id] = Subscriber(request: request, completion: completion)
            return flight
        }
        request.onCancel { [weak self, weak flight] in
            guard let flight else { return }
            self?.detach(id, from: flight)
        }
        pump()
        trimPending()
        return request
    }

    private func detach(_ id: UUID, from flight: Flight) {
        lock.withLock {
            flight.subscribers.removeValue(forKey: id)
            if flight.subscribers.isEmpty {
                if flights[flight.key] === flight { flights.removeValue(forKey: flight.key) }
                pending.removeAll { $0 === flight }
                flight.task?.cancel()
            }
        }
        pump()
    }

    private func pump() {
        lock.withLock {
            while running.count < concurrency && !pending.isEmpty {
                // A cancelled FFI/ImageIO call cannot be interrupted. Keep its image/tier
                // slot until it actually returns; a new generation waits rather than overlaps.
                let eligible = pending.indices.filter { index in
                    !running.values.contains { $0.key == pending[index].key }
                }
                guard let index = eligible.max(by: { lhs, rhs in
                    let left = pending[lhs], right = pending[rhs]
                    if left.key.tier != right.key.tier { return left.key.tier == .thumbnail }
                    if left.priority != right.priority { return left.priority.rawValue < right.priority.rawValue }
                    return lhs > rhs
                }) else { break }
                let flight = pending.remove(at: index)
                running[flight.id] = flight
                let queue = queue
                flight.task = Task.detached { [weak self] in
                    defer { self?.finished(flight) }
                    let subscription = flight.item.engineImage.map {
                        $0.previewEvents.subscribe(imageID: $0.imageID, maxPx: UInt32(flight.key.tier.maxPixelSize))
                    }
                    defer { subscription?.cancel() }
                    var iterator = subscription?.stream.makeAsyncIterator()
                    while !Task.isCancelled {
                        let result = await Self.renderQueued(flight.item, tier: flight.key.tier,
                                                             priority: flight.priority, queue: queue)
                        guard !Task.isCancelled else { return }
                        if let image = result.image {
                            await self?.deliver(image, flight: flight)
                            self?.afterDelivery?()
                            return
                        }
                        guard result.pending, await iterator?.next() != nil else { return }
                    }
                }
            }
        }
    }

    @MainActor private func deliver(_ image: CGImage, flight: Flight) {
        let subscribers: [Subscriber] = lock.withLock {
            guard flights[flight.key] === flight, !flight.subscribers.isEmpty else { return [] }
            flight.image = image
            cache(flight.key.tier).insert(image, for: flight.item)
            return Array(flight.subscribers.values)
        }
        for subscriber in subscribers where !subscriber.request.isCancelled {
            // Callbacks can invalidate or remove all, so recheck every subscriber.
            let valid = lock.withLock { flights[flight.key] === flight }
            if valid { subscriber.completion(image) }
        }
    }

    private func finished(_ flight: Flight) {
        let subscribers = lock.withLock {
            running.removeValue(forKey: flight.id)
            flight.task = nil
            if flights[flight.key] === flight { flights.removeValue(forKey: flight.key) }
            let subscribers = Array(flight.subscribers.values)
            flight.subscribers.removeAll()
            return subscribers
        }
        for subscriber in subscribers { subscriber.request.finish() }
        pump()
    }

    private static func renderQueued(_ item: PhotoItem, tier: PreviewTier, priority: Operation.QueuePriority,
                                     queue: OperationQueue) async -> RenderResult {
        await withCheckedContinuation { continuation in
            let op = BlockOperation { continuation.resume(returning: renderResult(item, tier: tier)) }
            op.queuePriority = priority
            queue.addOperation(op)
        }
    }

    // MARK: Rendering

    public static func render(_ item: PhotoItem, tier: PreviewTier) -> CGImage? {
        renderResult(item, tier: tier).image
    }

    private struct RenderResult: Sendable {
        var image: CGImage? = nil
        var pending = false
    }

    private static func renderResult(_ item: PhotoItem, tier: PreviewTier) -> RenderResult {
        if let ref = item.engineImage {
            guard let response = try? ref.engine.embeddedPreview(imageId: ref.imageID, maxPx: UInt32(tier.maxPixelSize)) else {
                return RenderResult()
            }
            guard let bytes = response.bytes, let src = CGImageSourceCreateWithData(bytes as CFData, nil) else {
                return RenderResult(pending: response.pending)
            }
            return RenderResult(image: CGImageSourceCreateImageAtIndex(src, 0,
                [kCGImageSourceShouldCacheImmediately: true] as CFDictionary), pending: response.pending)
        }
        return RenderResult(image: renderLocal(item, tier: tier))
    }

    private static func renderLocal(_ item: PhotoItem, tier: PreviewTier) -> CGImage? {
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
