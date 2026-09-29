import AppKit
import XCTest
@testable import Tessera

/// Exercises AppKit's semantic objects and Objective-C NSArray/getter boundary,
/// not a mock InspectorFocusTraceNode. Runtime remains an A-only gate.
@MainActor
final class InspectorFocusAXBridgeTests: XCTestCase {
    private final class ObjectiveCChildren: NSObject {
        var result: AnyObject?
        var childReads = 0
        var navigationReads = 0

        @objc func accessibilityChildren() -> AnyObject? {
            childReads += 1
            return result
        }

        // Deliberately heterogeneous Objective-C result. The repair must NEVER
        // request this typed navigation accessor, even if it is implemented.
        @objc func accessibilityChildrenInNavigationOrder() -> AnyObject? {
            navigationReads += 1
            return NSArray(array: [NSNull(), NSString(string: "not a semantic element")])
        }
    }

    private func element() -> NSAccessibilityElement {
        let element = NSAccessibilityElement()
        element.setAccessibilityRole(.button)
        element.setAccessibilityIdentifier("document.properties.colorLookup.load")
        element.setAccessibilityFocused(true)
        return element
    }

    func testNativeAppKitChildrenAcceptNonViewSemanticElements() throws {
        let root = NSAccessibilityElement(), child = element()
        root.setAccessibilityChildren([child])
        let children = AppKitFocusNode(root).children(limit: 4)
        XCTAssertEqual(children.nodes.count, 1)
        XCTAssertFalse(children.truncated)
        XCTAssertEqual(try XCTUnwrap(children.nodes.first).identity, ObjectIdentifier(child))
        XCTAssertEqual(children.nodes.first?.role, "AXButton")
        XCTAssertEqual(children.nodes.first?.identifier, "document.properties.colorLookup.load")
    }

    func testObjectiveCArrayIsCheckedElementByElementWithoutNavigationBridge() {
        let provider = ObjectiveCChildren(), first = element(), last = element()
        provider.result = NSArray(array: [first, NSNull(), NSString(string: "private text"), NSObject(), last])
        let children = AppKitFocusNode.nativeChildren(of: provider, limit: 64)
        XCTAssertEqual(children.nodes.map(\.identity), [ObjectIdentifier(first), ObjectIdentifier(last)])
        XCTAssertTrue(children.truncated, "unsafe entries cannot be silently treated as a complete tree")
        XCTAssertEqual(provider.childReads, 1)
        XCTAssertEqual(provider.navigationReads, 0)
    }

    func testNilMalformedAndMissingObjectiveCGetterFailClosed() {
        let provider = ObjectiveCChildren()
        XCTAssertTrue(AppKitFocusNode.nativeChildren(of: provider, limit: 4).truncated)
        provider.result = NSString(string: "not an array")
        let malformed = AppKitFocusNode.nativeChildren(of: provider, limit: 4)
        XCTAssertTrue(malformed.nodes.isEmpty)
        XCTAssertTrue(malformed.truncated)
        XCTAssertTrue(AppKitFocusNode.nativeChildren(of: NSObject(), limit: 4).truncated)
        XCTAssertEqual(provider.navigationReads, 0)
        provider.result = NSArray()
        let empty = AppKitFocusNode.nativeChildren(of: provider, limit: 4)
        XCTAssertTrue(empty.nodes.isEmpty)
        XCTAssertFalse(empty.truncated, "an explicitly empty NSArray differs from unavailable semantics")
    }

    func testLimitsBoundObjectiveCCollectionAndZeroBudgetDoesNotInvokeGetter() {
        let provider = ObjectiveCChildren()
        let values = (0..<70).map { _ in element() }
        provider.result = NSArray(array: values)
        for limit in [0, -1] {
            let children = AppKitFocusNode.nativeChildren(of: provider, limit: limit)
            XCTAssertTrue(children.nodes.isEmpty)
            XCTAssertTrue(children.truncated)
        }
        XCTAssertEqual(provider.childReads, 0)
        let two = AppKitFocusNode.nativeChildren(of: provider, limit: 2)
        XCTAssertEqual(two.nodes.map(\.identity), values.prefix(2).map { ObjectIdentifier($0) })
        XCTAssertTrue(two.truncated)
        let capped = AppKitFocusNode.nativeChildren(of: provider, limit: Int.max)
        XCTAssertEqual(capped.nodes.count, 64)
        XCTAssertTrue(capped.truncated)
        XCTAssertEqual(provider.navigationReads, 0)
    }

    func testNativeHeterogeneousChildrenKeepSnapshotUnknownAndReleaseObjects() {
        let window = NSObject()
        weak var releasedRoot: NSAccessibilityElement?
        weak var releasedChild: NSAccessibilityElement?
        // All native graph/array/getter construction and capture occur inside
        // an explicit drain boundary. A lexical do scope does not drain ObjC
        // autoreleases. Keep the returned value and window alive after draining.
        let snapshot = autoreleasepool {
            let root = NSAccessibilityElement(), child = element()
            releasedRoot = root; releasedChild = child
            root.setAccessibilityFocused(false)
            child.setAccessibilityWindow(window)
            child.setAccessibilityChildren([])
            root.setAccessibilityChildren([child, NSNull()])
            return InspectorFocusTrace.snapshot(focused: nil, root: AppKitFocusNode(root),
                window: ObjectIdentifier(window), nativeType: "Test", nativeIdentity: "test")
        }
        withExtendedLifetime((window, snapshot)) {
            XCTAssertEqual(snapshot.status, "unknown")
            XCTAssertEqual(snapshot.incomplete, true)
            XCTAssertNil(releasedRoot)
            XCTAssertNil(releasedChild, "value-only snapshot must not retain native semantic elements after pool drain")
        }
    }

    func testSameNativeGraphWithoutObserverReleasesAfterPoolDrain() {
        let window = NSObject()
        weak var releasedRoot: NSAccessibilityElement?
        weak var releasedChild: NSAccessibilityElement?
        autoreleasepool {
            // Match the observed test's graph exactly, omitting only wrapping
            // and snapshot capture. No manual clearing of native ownership.
            let root = NSAccessibilityElement(), child = element()
            releasedRoot = root; releasedChild = child
            root.setAccessibilityFocused(false)
            child.setAccessibilityWindow(window)
            child.setAccessibilityChildren([])
            root.setAccessibilityChildren([child, NSNull()])
        }
        withExtendedLifetime(window) {
            XCTAssertNil(releasedRoot, "no-observer native graph must release after pool drain")
            XCTAssertNil(releasedChild, "no-observer native child must release after pool drain")
        }
    }
}
