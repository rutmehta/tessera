import Foundation
import TesseraFFI

public enum RasterizedPSDCopyResult: Sendable, Equatable { case saved, cancelled }

/// Synchronous run belongs on the output worker; cancel must be called directly,
/// never queued behind run. Accepted cancellation can still be draining an encoder.
public protocol RasterizedPSDCopyOperation: AnyObject, Sendable {
    @discardableResult func cancel() -> Bool
    func run(path: String) throws -> RasterizedPSDCopyResult
}

final class EngineRasterizedPSDCopyOperation: RasterizedPSDCopyOperation {
    private let operation: TesseraFFI.RasterizedPsdCopyOperation
    init(_ operation: TesseraFFI.RasterizedPsdCopyOperation) { self.operation = operation }
    func cancel() -> Bool { operation.cancel() }
    func run(path: String) throws -> RasterizedPSDCopyResult {
        switch try operation.run(path: path) {
        case .saved: return .saved
        case .cancelled: return .cancelled
        }
    }
}

/// Main-actor ownership; cancellation never vacates a running request's slot.
/// The Rust registry independently enforces admission through actual unwind.
@MainActor
public final class RasterizedPSDCopySlot {
    public private(set) var id: UUID?
    public private(set) var cancelling = false
    private var operation: (any RasterizedPSDCopyOperation)?
    public init() {}
    public func install(_ operation: any RasterizedPSDCopyOperation) -> UUID? {
        guard id == nil else { return nil }
        let id = UUID()
        self.id = id
        self.operation = operation
        cancelling = false
        return id
    }
    @discardableResult public func cancel() -> Bool {
        guard let operation else { return false }
        let accepted = operation.cancel()
        if accepted { cancelling = true }
        return accepted
    }
    @discardableResult public func finish(_ id: UUID) -> Bool {
        guard self.id == id else { return false }
        self.id = nil
        operation = nil
        cancelling = false
        return true
    }
    public func close() {
        _ = cancel()
        id = nil
        operation = nil
        cancelling = false
    }
}
