import AppKit
import XCTest
@testable import Tessera
@testable import TesseraCore

/// B5-44: adding a duplicate, undocumented or system-reserved binding must fail the gate.
/// Source enumeration is intentional: SwiftPM does not host SwiftUI's main menu.
final class ShortcutIntegrityTests: XCTestCase {
    func testMenuAndRouterDocumentationAndMenuCollisions() throws {
        let root = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent().deletingLastPathComponent()
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/env")
        process.arguments = ["python3", root.appendingPathComponent("tools/orchestrate/shortcut-audit.py").path]
        let output = Pipe()
        process.standardOutput = output
        process.standardError = output
        try process.run()
        let data = output.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        XCTAssertEqual(process.terminationStatus, 0, String(decoding: data, as: UTF8.self))
    }

    func testToolLettersOnlyOverlapWithinTheirCycleGroup() {
        // A new tool accidentally using another palette slot's letter must not silently
        // become unreachable through ToolKeyMap's first(where:) selection.
        for tool in DocumentTool.allCases {
            let peers = DocumentTool.allCases.filter { $0.key.lowercased() == tool.key.lowercased() }
            XCTAssertEqual(Set(peers), Set(tool.group), "Duplicate tool letter \(tool.key): \(peers)")
            for current in tool.group {
                let plain = ToolKeyMap.action(keyCode: 0, characters: tool.key, mods: [], current: current)
                let shifted = ToolKeyMap.action(keyCode: 0, characters: tool.key, mods: .shift, current: current)
                guard case .tool(let selected)? = plain, case .tool(let cycled)? = shifted else {
                    XCTFail("Unreachable tool group \(tool.key)"); continue
                }
                XCTAssertTrue(tool.group.contains(selected))
                XCTAssertTrue(tool.group.contains(cycled))
            }
        }
    }

    @MainActor
    func testReservedSystemShortcutsPassThroughRouterInBothModes() throws {
        let reserved: [(UInt16, String, NSEvent.ModifierFlags)] = [
            (12, "q", .command), (4, "h", .command), (4, "h", [.command, .option]),
            (46, "m", .command), (43, ",", .command), (48, "\t", .command),
            (12, "q", [.control, .command]), (49, " ", .command),
            (20, "3", [.command, .shift]), (21, "4", [.command, .shift]),
            (23, "5", [.command, .shift]), (120, "\u{F705}", .control),
            (99, "\u{F706}", .control), (103, "\u{F70E}", []),
            (111, "\u{F70F}", []), (50, "`", .command),
        ]
        let model = AppModel()
        model.loadStubItems(count: 3)
        model.documents.engine = StubDocumentEngine()
        model.documents.newDocument(NewDocumentSettings(width: 32, height: 32))
        let router = KeyRouter(model: model)
        let window = LayoutProbeHarness.window(contentRect: NSRect(x: 0, y: 0, width: 200, height: 100),
                                               styleMask: .titled, backing: .buffered, defer: false)
        defer { window.close() }
        for mode in [ViewMode.grid, .document] {
            model.viewMode = mode
            for (code, character, flags) in reserved {
                let event = try XCTUnwrap(NSEvent.keyEvent(with: .keyDown, location: .zero,
                    modifierFlags: flags, timestamp: 0, windowNumber: window.windowNumber, context: nil,
                    characters: character, charactersIgnoringModifiers: character, isARepeat: false, keyCode: code))
                XCTAssertFalse(router.handle(event), "Reserved \(flags.rawValue)+\(character) in \(mode)")
                var mods: DocumentKeyMap.Mods = []
                if flags.contains(.command) { mods.insert(.command) }
                if flags.contains(.control) { mods.insert(.control) }
                if flags.contains(.option) { mods.insert(.option) }
                if flags.contains(.shift) { mods.insert(.shift) }
                for tool in DocumentTool.allCases {
                    XCTAssertNil(ToolKeyMap.action(keyCode: code, characters: character, mods: mods, current: tool))
                }
                XCTAssertNil(DocumentKeyMap.action(keyCode: code, characters: character, mods: mods))
            }
        }
    }
}
