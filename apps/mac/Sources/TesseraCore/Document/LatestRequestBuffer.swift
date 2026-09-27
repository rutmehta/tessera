/// One running request and one replaceable pending request. The caller serializes
/// access (the document frontend uses MainActor) and starts only returned requests.
/// Invalidating drops pending work and rejects the current result; it cannot cancel
/// an already-running synchronous engine call or release its worker slot early.
public struct LatestRequestBuffer<Value: Sendable>: Sendable {
    public struct Request: Sendable {
        public let generation: UInt64
        public let value: Value
    }
    private var generation: UInt64 = 0
    private var running: UInt64?
    private var pending: Request?

    public init() {}

    public mutating func submit(_ value: Value) -> Request? {
        generation &+= 1
        let request = Request(generation: generation, value: value)
        guard running == nil else {
            pending = request
            return nil
        }
        running = request.generation
        return request
    }

    public mutating func invalidate() {
        generation &+= 1
        pending = nil
    }

    public mutating func finish(_ token: UInt64) -> (accept: Bool, next: Request?) {
        guard running == token else { return (false, nil) }
        let accept = token == generation
        let next = pending
        pending = nil
        running = next?.generation
        return (accept, next)
    }
}
