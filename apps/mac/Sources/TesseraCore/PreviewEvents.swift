import Foundation
import TesseraFFI

/// One listener per engine, shared by every item and both preview tiers. It never owns the engine.
/// It also forwards `LibraryChanged` (the catalog change feed) to the library's owner.
final class PreviewEvents: EngineEventListener, @unchecked Sendable {
    struct Subscription: Sendable {
        let stream: AsyncStream<Void>
        let cancel: @Sendable () -> Void
    }
    private struct Observer {
        let imageID: String
        let maxPx: UInt32
        let continuation: AsyncStream<Void>.Continuation
    }
    private let lock = NSLock()
    private var observers: [UUID: Observer] = [:]
    private var libraryHandler: (@Sendable (UInt64) -> Void)?

    /// Called on an engine thread with the catalog's new change sequence.
    func onLibraryChanged(_ handler: (@Sendable (UInt64) -> Void)?) {
        lock.withLock { libraryHandler = handler }
    }

    func subscribe(imageID: String, maxPx: UInt32) -> Subscription {
        let id = UUID()
        let (stream, continuation) = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingNewest(1))
        continuation.onTermination = { [weak self] _ in self?.remove(id) }
        lock.withLock { observers[id] = Observer(imageID: imageID, maxPx: maxPx, continuation: continuation) }
        return Subscription(stream: stream, cancel: { [weak self] in
            self?.remove(id)
            continuation.finish()
        })
    }

    private func remove(_ id: UUID) { _ = lock.withLock { observers.removeValue(forKey: id) } }

    func onEvent(event: EngineEvent) {
        if case let .libraryChanged(sequence) = event {
            lock.withLock { libraryHandler }?(sequence)
            return
        }
        guard case let .previewReady(imageId, maxPx) = event else { return }
        let matches = lock.withLock {
            observers.values.filter { $0.imageID == imageId && $0.maxPx == maxPx }.map(\.continuation)
        }
        // Yield outside the lock: termination can concurrently unregister a cancelled cell.
        for continuation in matches { continuation.yield(()) }
    }
}