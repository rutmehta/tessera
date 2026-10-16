import Foundation
import XCTest
@testable import TesseraCore

/// Memory evidence for ST-2: develop sessions opened and closed straight on the engine, without
/// the shell, the loupe or the preview caches. The footprint after every close is printed for the
/// engine-side report (what the shared renderer and its caches keep); the assertions cover what
/// the app side owns: a closed controller lets go of its surfaces and deallocates.
@MainActor
final class EngineSessionMemoryTests: XCTestCase {
    func testClosedSessionsReleaseTheirControllersAndSurfaces() async throws {
        try XCTSkipIf(ProcessInfo.processInfo.environment["CI"] != nil, "needs the engine's RAW decode on a real Mac")
        let scratch = FileManager.default.temporaryDirectory.appendingPathComponent("session-memory-\(UUID().uuidString)")
        let folder = scratch.appendingPathComponent("shoot")
        addTeardownBlock { try? FileManager.default.removeItem(at: scratch) }
        let source = ProcessInfo.processInfo.environment["TESSERA_SAMPLE_SHOOT"].map { URL(fileURLWithPath: $0, isDirectory: true) }
            ?? URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
                .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
                .appendingPathComponent("fixtures/raw")
        try FileManager.default.createDirectory(at: scratch, withIntermediateDirectories: true)
        try FileManager.default.copyItem(at: source, to: folder)
        for name in (try? FileManager.default.contentsOfDirectory(atPath: folder.path)) ?? [] where name.hasSuffix(".xmp") || name == ".edits" {
            try? FileManager.default.removeItem(at: folder.appendingPathComponent(name))
        }

        var lines = ["ST-2 engine session memory: start \(MemoryFootprint.current()?.description ?? "?")"]
        let library = try EngineLibrary.scan(folder: folder, appSupport: scratch.appendingPathComponent("support"))
        lines.append("  after scan: \(MemoryFootprint.current()?.description ?? "?")")
        let items = Array(library.items.prefix(6))
        XCTAssertGreaterThanOrEqual(items.count, 3)
        var released: [() -> Bool] = []
        /// Opens, renders and closes one session; returns whether its controller has deallocated.
        func cycle(_ item: PhotoItem, _ n: Int) async throws -> () -> Bool {
            let controller = try await DevelopController.open(try XCTUnwrap(item.engineImage), itemID: item.id)
            try controller.attachSurfaces(viewWidth: 1440, viewHeight: 900)
            let deadline = Date().addingTimeInterval(60)
            while controller.lastFrame?.isFinal != true, Date() < deadline { try await Task.sleep(for: .milliseconds(20)) }
            XCTAssertTrue(controller.lastFrame?.isFinal == true, "\(item.name) rendered")
            lines.append("  photo \(n + 1) open: \(MemoryFootprint.current()?.description ?? "?")")
            let shown = controller.lastFrame?.surfaceID ?? 0
            let result = await controller.close()
            XCTAssertNoThrow(try result.get())
            XCTAssertNil(controller.surface(shown), "the ring is released on close")
            XCTAssertNil(controller.plan)
            weak var weakController = controller
            return { weakController == nil }
        }
        for (n, item) in items.enumerated() {
            released.append(try await cycle(item, n))
            try await Task.sleep(for: .milliseconds(300))
            lines.append("  photo \(n + 1) closed: \(MemoryFootprint.current()?.description ?? "?")")
        }
        try await Task.sleep(for: .seconds(1))
        lines.append("  all closed: \(MemoryFootprint.current()?.description ?? "?")")
        let report = lines.joined(separator: "\n")
        print(report)
        FileHandle.standardError.write(Data((report + "\n").utf8))
        for (n, gone) in released.enumerated() { XCTAssertTrue(gone(), "controller \(n + 1) deallocated after close") }
        _ = library
    }
}
