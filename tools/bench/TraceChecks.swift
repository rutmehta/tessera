import Foundation

@main struct TraceChecks {
    static func main() {
        let off = PerformanceTrace(enabled: false, capacity: 2)
        off.record("input", session: "s", input: 1)
        precondition(off.snapshot().events.isEmpty)
        let trace = PerformanceTrace(enabled: true, capacity: 2)
        trace.record("input", session: "s", input: 1)
        trace.record("callback_enqueue", session: "s", generation: 42, width: 800, height: 600, level: 2)
        trace.record("input")
        let snapshot = trace.snapshot()
        precondition(snapshot.dropped == 1 && snapshot.events.count == 2)
        precondition(snapshot.events[1].generation == 42 && snapshot.events[1].input == nil)
        precondition(snapshot.events[1].residency == "unavailable")
        let concurrent = PerformanceTrace(enabled: true, capacity: 1000)
        let span = concurrent.begin("flush", session: "s", input: 1)
        concurrent.end(span)
        DispatchQueue.concurrentPerform(iterations: 100) { i in
            concurrent.record("callback_enqueue", session: "s", generation: UInt64(i))
        }
        let rows = concurrent.snapshot().events
        precondition(rows.count == 102 && rows[0].span == rows[1].span)
        precondition((rows[1].durationMs ?? -1) >= 0)
        precondition(Set(rows.dropFirst(2).compactMap(\.generation)).count == 100)
        print("PerformanceTrace checks passed")
    }
}
