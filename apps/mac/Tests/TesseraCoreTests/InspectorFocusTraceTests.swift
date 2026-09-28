import AppKit
import XCTest
@testable import Tessera

@MainActor
final class InspectorFocusTraceTests: XCTestCase {
    private final class Sink: InspectorFocusTraceSink {
        var data = Data()
        var closes = 0
        var fail = false
        func append(_ bytes: Data) throws {
            if fail { throw CocoaError(.fileWriteUnknown) }
            data.append(bytes)
        }
        func close() { closes += 1 }
    }

    private final class Node: InspectorFocusTraceNode {
        var identity: ObjectIdentifier { ObjectIdentifier(self) }
        var role: String? = "AXButton"
        var identifier: String? = "document.properties.colorLookup.load"
        var focused: Bool? = true
        var windowIdentity: ObjectIdentifier?
        weak var parentNode: Node?
        var parent: (any InspectorFocusTraceNode)? { parentNode }
        var descendants: [Node] = []
        func children(limit: Int) -> InspectorFocusTraceChildren {
            .init(nodes: Array(descendants.prefix(limit)), truncated: descendants.count > limit)
        }
    }

    private func input(_ code: UInt16 = 48) -> InspectorFocusTrace.Input {
        .init(keyDown: true, keyCode: code, modifiers: 0, eventNumber: 1, timestamp: 1,
              document: true, ownedKeyWindow: true, blockedWindow: false, fullKeyboardAccess: true)
    }

    private func snapshot(_ focus: Node?, root: Node? = nil, window: NSObject) -> InspectorFocusTrace.Snapshot {
        InspectorFocusTrace.snapshot(focused: focus, root: root, window: ObjectIdentifier(window),
                                     nativeType: "TestResponder", nativeIdentity: "native-1")
    }

    func testDisabledAndExcludedNeverCaptureOrCreateOutputAndRouteOnce() {
        var opened = 0
        XCTAssertNil(InspectorFocusTrace.configured(arguments: ["Tessera"], open: { _ in opened += 1; return Sink() }))
        XCTAssertEqual(opened, 0)
        var calls = 0, captures = 0
        let sink = Sink()
        let trace = InspectorFocusTrace(sink: sink)
        var excluded = input()
        excluded.document = false
        var inputs = [excluded]
        excluded = input(); excluded.ownedKeyWindow = false; inputs.append(excluded)
        excluded = input(); excluded.blockedWindow = true; inputs.append(excluded)
        excluded = input(); excluded.keyDown = false; inputs.append(excluded)
        excluded = input(); excluded.modifiers = NSEvent.ModifierFlags.command.rawValue; inputs.append(excluded)
        inputs.append(input(0)) // typed letter is not recorded
        for value in inputs {
            XCTAssertTrue(InspectorFocusTrace.route(trace, input: value, capture: {
                captures += 1; return .unknown
            }, handler: { calls += 1; return true }))
        }
        XCTAssertFalse(InspectorFocusTrace.route(nil, input: input(), capture: {
            captures += 1; return .unknown
        }, handler: { calls += 1; return false }))
        XCTAssertEqual(calls, inputs.count + 1)
        XCTAssertEqual(captures, 0)
        XCTAssertTrue(sink.data.isEmpty)
    }

    func testCapturePrecedesExactlyOneHandlerAndReturnIsUnchanged() {
        let sink = Sink()
        let recording = InspectorFocusTrace(sink: sink)
        for result in [false, true] {
            var order: [String] = []
            let actual = InspectorFocusTrace.route(recording, input: input(), capture: {
                order.append("capture"); return .unknown
            }, handler: { order.append("handler"); return result })
            XCTAssertEqual(order, ["capture", "handler"])
            XCTAssertEqual(actual, result)
        }
        let output = String(decoding: sink.data, as: UTF8.self)
        XCTAssertTrue(output.contains("\"handled\":false"))
        XCTAssertTrue(output.contains("\"handled\":true"))
    }

    func testNonViewSemanticFocusRequiresExactWindowMembership() {
        let window = NSObject(), other = NSObject(), focus = Node()
        focus.windowIdentity = ObjectIdentifier(window)
        XCTAssertEqual(snapshot(focus, window: window).status, "focused")
        focus.windowIdentity = ObjectIdentifier(other)
        XCTAssertEqual(snapshot(focus, window: window).status, "unknown")
        focus.windowIdentity = nil
        XCTAssertEqual(snapshot(focus, window: window).status, "unknown")
        let parent = Node()
        parent.windowIdentity = ObjectIdentifier(window)
        focus.parentNode = parent
        XCTAssertEqual(snapshot(focus, window: window).status, "focused")
        focus.focused = false
        XCTAssertEqual(snapshot(focus, window: window).status, "unknown")
    }

    func testFallbackAmbiguityCyclesDepthAndNodeCapsAreNotFocusProof() {
        let window = NSObject(), root = Node(), first = Node(), second = Node()
        root.focused = false
        for node in [first, second] { node.windowIdentity = ObjectIdentifier(window) }
        root.descendants = [first]
        XCTAssertEqual(snapshot(nil, root: root, window: window).status, "focused")
        root.descendants = [first, second]
        XCTAssertEqual(snapshot(nil, root: root, window: window).status, "ambiguous")
        root.descendants = [root]
        XCTAssertEqual(snapshot(nil, root: root, window: window).status, "unknown")
        root.descendants = [] // break test-owned strong cycle
        let chain = (0..<12).map { _ in Node() }
        for index in 0..<11 { chain[index].focused = false; chain[index].descendants = [chain[index + 1]] }
        chain.last?.windowIdentity = ObjectIdentifier(window)
        XCTAssertEqual(snapshot(nil, root: chain[0], window: window).status, "unknown")
        root.descendants = (0..<70).map { _ in Node() }
        XCTAssertEqual(snapshot(nil, root: root, window: window).status, "unknown")
        first.windowIdentity = nil; first.parentNode = second; second.windowIdentity = nil; second.parentNode = first
        XCTAssertEqual(snapshot(first, window: window).status, "unknown")
    }

    func testOutputExcludesArbitrarySemanticStringsAndRetainsNoNodes() {
        let sink = Sink(), window = NSObject()
        let trace = InspectorFocusTrace(sink: sink)
        weak var released: Node?
        do {
            let node = Node()
            released = node
            node.windowIdentity = ObjectIdentifier(window)
            node.identifier = "/private/user-photo-name.CR3"
            node.role = "secret typed text"
            _ = InspectorFocusTrace.route(trace, input: input(), capture: {
                self.snapshot(node, window: window)
            }, handler: { false })
        }
        XCTAssertNil(released)
        let output = String(decoding: sink.data, as: UTF8.self)
        XCTAssertFalse(output.contains("user-photo"))
        XCTAssertFalse(output.contains("secret typed"))
        XCTAssertTrue(output.contains("focused"))
    }

    func testEventAndByteCapsCloseAndPreventFurtherCapture() {
        let sink = Sink()
        let trace = InspectorFocusTrace(sink: sink)
        var captures = 0, handled = 0
        for _ in 0..<40 {
            _ = InspectorFocusTrace.route(trace, input: input(), capture: {
                captures += 1; return .unknown
            }, handler: { handled += 1; return false })
        }
        XCTAssertEqual(captures, 32)
        XCTAssertEqual(handled, 40)
        XCTAssertLessThanOrEqual(sink.data.count, 64 * 1024)
        XCTAssertEqual(sink.closes, 1)
        let smallSink = Sink()
        let small = InspectorFocusTrace(sink: smallSink, byteLimit: 80)
        for _ in 0..<3 {
            _ = InspectorFocusTrace.route(small, input: input(), capture: { .unknown }, handler: { true })
        }
        XCTAssertLessThanOrEqual(smallSink.data.count, 80)
        XCTAssertEqual(smallSink.closes, 1)
    }

    func testWriteFailureAndDeinitCloseWithoutChangingHandler() {
        let sink = Sink()
        sink.fail = true
        var trace: InspectorFocusTrace? = InspectorFocusTrace(sink: sink)
        var calls = 0
        XCTAssertTrue(InspectorFocusTrace.route(trace, input: input(), capture: { .unknown },
                                               handler: { calls += 1; return true }))
        trace = nil
        XCTAssertEqual(calls, 1)
        XCTAssertEqual(sink.closes, 1)
        let unused = Sink()
        var unrun: InspectorFocusTrace? = InspectorFocusTrace(sink: unused)
        XCTAssertNotNil(unrun)
        unrun = nil
        XCTAssertEqual(unused.closes, 1)
    }

    func testExclusiveOutputDoesNotOverwriteExistingFile() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: directory) }
        let path = directory.appendingPathComponent("trace.jsonl").path
        var trace = InspectorFocusTrace.configured(arguments: ["Tessera", "--inspector-focus-trace", path])
        XCTAssertNotNil(trace)
        XCTAssertNil(InspectorFocusTrace.configured(arguments: ["Tessera", "--inspector-focus-trace", path]))
        _ = InspectorFocusTrace.route(trace, input: input(), capture: { .unknown }, handler: { false })
        trace = nil
        let before = try Data(contentsOf: URL(fileURLWithPath: path))
        XCTAssertNil(InspectorFocusTrace.configured(arguments: ["Tessera", "--inspector-focus-trace", path]))
        XCTAssertEqual(try Data(contentsOf: URL(fileURLWithPath: path)), before)
    }

    func testReentrantRoutingDoesNotCaptureTwiceOrChangeNestedResult() {
        let sink = Sink(), trace = InspectorFocusTrace(sink: Sink())
        let recording = InspectorFocusTrace(sink: sink)
        var captures = 0, handlers = 0
        XCTAssertFalse(InspectorFocusTrace.route(recording, input: input(), capture: {
            captures += 1
            return .unknown
        }, handler: {
            handlers += 1
            XCTAssertTrue(InspectorFocusTrace.route(recording, input: self.input(), capture: {
                captures += 1; return .unknown
            }, handler: { handlers += 1; return true }))
            return false
        }))
        XCTAssertEqual(captures, 1)
        XCTAssertEqual(handlers, 2)
        XCTAssertTrue(String(decoding: sink.data, as: UTF8.self).contains("\"reentrantEventsSkipped\":1"))
        // A separate collector is unaffected by another collector's capture state.
        XCTAssertFalse(InspectorFocusTrace.route(trace, input: input(), capture: { .unknown }, handler: { false }))
    }
}
