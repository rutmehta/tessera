import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers
import XCTest
import TesseraFFI
@testable import TesseraCore

/// WP M3-11: the agent group's fade patch, the review queue model, Settings ▸ AI with a mocked
/// Keychain, and assisted culling through a real engine session.
final class AgentFadeTests: XCTestCase {
    private func json(_ s: String) -> [String: Any] {
        (try! JSONSerialization.jsonObject(with: Data(s.utf8))) as! [String: Any]
    }

    func testBlendInterpolatesNumbersAndSwitchesOtherValuesAtHalf() {
        let without = json(#"{"tone":{"exposure":0.0,"contrast":0.0},"white_balance":{"mode":"as_shot","temperature":5000.0},"grain":{"seed":0},"on":false}"#)
        let with = json(#"{"tone":{"exposure":1.0,"contrast":20.0},"white_balance":{"mode":"custom","temperature":5400.0},"grain":{"seed":10},"on":true}"#)
        let quarter = AgentFade.blend(without: without, with: with, amount: 0.25) as! [String: Any]
        let tone = quarter["tone"] as! [String: Any]
        XCTAssertEqual((tone["exposure"] as! NSNumber).doubleValue, 0.25, accuracy: 1e-12)
        XCTAssertEqual((tone["contrast"] as! NSNumber).doubleValue, 5, accuracy: 1e-12)
        let wb = quarter["white_balance"] as! [String: Any]
        XCTAssertEqual(wb["mode"] as? String, "as_shot", "non-numeric keeps the group-off value below 50 %")
        XCTAssertEqual((wb["temperature"] as! NSNumber).doubleValue, 5100, accuracy: 1e-9)
        let seed = (quarter["grain"] as! [String: Any])["seed"] as! NSNumber
        XCTAssertFalse(CFNumberIsFloatType(seed), "integers stay integers")
        XCTAssertEqual(seed.intValue, 3, "2.5 rounds half away from zero, as in the engine")
        XCTAssertEqual(quarter["on"] as? Bool, false)
        let half = AgentFade.blend(without: without, with: with, amount: 0.5) as! [String: Any]
        XCTAssertEqual((half["white_balance"] as! [String: Any])["mode"] as? String, "custom")
        XCTAssertEqual(half["on"] as? Bool, true)
        // The end points are exactly the engine's JSON.
        XCTAssertTrue(NSDictionary(dictionary: AgentFade.blend(without: without, with: with, amount: 0) as! [String: Any]).isEqual(to: without))
        XCTAssertTrue(NSDictionary(dictionary: AgentFade.blend(without: without, with: with, amount: 1) as! [String: Any]).isEqual(to: with))
    }

    func testPatchOnlyTouchesWhatTheAmountChangesAndKeepsLaterEdits() throws {
        // A later manual edit (shadows +10) is in both states; the group set exposure and contrast.
        let without = #"{"tone":{"exposure":0.0,"contrast":0.0,"shadows":10.0},"color":{"vibrance":0.0}}"#
        let with = #"{"tone":{"exposure":1.0,"contrast":20.0,"shadows":10.0},"color":{"vibrance":12.0}}"#
        let current = json(with)
        let patch = try XCTUnwrap(AgentFade.patch(current: current, withoutJSON: without, withJSON: with, amount: 0.6))
        let tone = try XCTUnwrap(patch["tone"] as? [String: Any])
        XCTAssertEqual(Set(tone.keys), ["exposure", "contrast"], "shadows (a later edit) is untouched")
        XCTAssertEqual((tone["exposure"] as! NSNumber).doubleValue, 0.6, accuracy: 1e-12)
        XCTAssertEqual((tone["contrast"] as! NSNumber).doubleValue, 12, accuracy: 1e-12)
        XCTAssertEqual(((patch["color"] as! [String: Any])["vibrance"] as! NSNumber).doubleValue, 7.2, accuracy: 1e-12)
        XCTAssertTrue(try XCTUnwrap(AgentFade.patch(current: current, withoutJSON: without, withJSON: with, amount: 1)).isEmpty,
                      "100 % is the current state: nothing to send")
        XCTAssertNil(AgentFade.patch(current: current, withoutJSON: "[]", withJSON: with, amount: 0.5))
        XCTAssertEqual(AgentFade.percent(0.604), "60 %")
    }

    func testMergePatchRemovesMembersWithNull() {
        let patch = AgentFade.mergePatch(from: ["a": 1, "b": ["c": 2, "d": 3]], to: ["b": ["c": 2]])
        XCTAssertTrue(patch["a"] is NSNull)
        XCTAssertTrue((patch["b"] as? [String: Any])?["d"] is NSNull)
        XCTAssertNil((patch["b"] as? [String: Any])?["c"])
        // Applying the patch with the controller's own RFC 7386 merge gives the target.
        let merged = DevelopController.merge(["a": 1, "b": ["c": 2, "d": 3]], patch, keepNulls: false)
        XCTAssertTrue(NSDictionary(dictionary: merged).isEqual(to: ["b": ["c": 2]]))
    }
}

final class AgentReviewQueueTests: XCTestCase {
    private func entry(_ id: String, _ confidence: Double, error: String? = nil) -> AgentReviewEntry {
        AgentReviewEntry(imageID: id, itemID: Int(id.dropFirst()), name: "\(id).jpg", groupID: 1, confidence: confidence,
                         steps: [.init(entryID: 1, title: "Exposure", rationale: "because \(id)")], error: error)
    }

    func testOrdersFailuresThenLeastConfidentFirst() {
        let q = AgentReviewQueue(entries: [entry("i1", 0.9), entry("i2", 0.2), entry("i3", 0.5, error: "timeout"), entry("i4", 0.2)])
        XCTAssertEqual(q.entries.map(\.imageID), ["i3", "i2", "i4", "i1"])
        XCTAssertEqual(q.pendingCount, 3)
        XCTAssertEqual(q.failedCount, 1)
        XCTAssertEqual(q.summary, "3 to review · 1 failed")
        XCTAssertEqual(q.entries[1].confidenceText, "Low · 20 %")
        XCTAssertEqual(q.entries[3].confidenceText, "High · 90 %")
        XCTAssertEqual(q.entries[0].confidenceText, "Failed")
        XCTAssertEqual(q.entries[0].summary, "timeout")
        XCTAssertEqual(q.entries[1].summary, "because i2")
    }

    func testStatusesChangeInPlaceAndNextSkipsReviewed() {
        var q = AgentReviewQueue(entries: [entry("i1", 0.9), entry("i2", 0.2), entry("i4", 0.3)])
        XCTAssertEqual(q.next(after: nil)?.imageID, "i2")
        q.setStatus(.accepted, for: "i2")
        q.setStatus(.reverted, for: "i4")
        XCTAssertEqual(q.entries.map(\.imageID), ["i2", "i4", "i1"], "rows do not jump")
        XCTAssertEqual(q.summary, "1 to review · 1 accepted · 1 reverted")
        XCTAssertEqual(q.next(after: "i2")?.imageID, "i1")
        XCTAssertEqual(q.next(after: "i1")?.imageID, nil, "nothing else pending")
        q.setStatus(.needsReview, for: "i4")
        XCTAssertEqual(q.next(after: "i1")?.imageID, "i4", "wraps around")
    }

    func testRedoMergeReplacesOnlyThosePhotos() {
        var q = AgentReviewQueue(entries: [entry("i1", 0.9), entry("i2", 0.2)], provider: "scripted planner")
        q.setStatus(.accepted, for: "i1")
        var redone = entry("i2", 0.95)
        redone.steps = [.init(entryID: 7, title: "Temperature", rationale: "Redo “warmer”")]
        q.merge([redone])
        XCTAssertEqual(q.entries.map(\.imageID), ["i1", "i2"], "re-sorted by the new confidence")
        XCTAssertEqual(q.entry("i2")?.steps.first?.entryID, 7)
        XCTAssertEqual(q.entry("i1")?.status, .accepted, "others keep their review state")
        XCTAssertEqual(q.count, 2)
    }
}

final class AISettingsTests: XCTestCase {
    private func store() throws -> (AISettingsStore, InMemorySecretStore, URL) {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("ai-settings-\(UUID().uuidString)")
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        let secrets = InMemorySecretStore()
        return (AISettingsStore(directory: dir, secrets: secrets), secrets, dir)
    }

    func testKeysLiveOnlyInTheSecretStoreAndProvidersReadThemLate() throws {
        let (settings, secrets, dir) = try store()
        var prefs = settings.load()
        XCTAssertEqual(prefs, AIPreferences(), "defaults without a file")
        XCTAssertThrowsError(try settings.provider(.anthropic, prefs)) { error in
            XCTAssertEqual(error as? AISettingsError, .missingKey(.anthropic))
        }
        try settings.setAPIKey("  sk-ant-test-1234567890  ", for: .anthropic)
        XCTAssertEqual(try secrets.read(account: "anthropic-api-key"), "sk-ant-test-1234567890", "trimmed")
        XCTAssertTrue(settings.hasKey(for: .anthropic))
        XCTAssertFalse(settings.hasKey(for: .openAI))
        prefs.anthropicModel = "claude-test"
        prefs.provider = .anthropic
        try settings.save(prefs)
        XCTAssertEqual(try settings.provider(.anthropic, prefs), .anthropic(apiKey: "sk-ant-test-1234567890", model: "claude-test"))
        // Preferences on disk never contain the key.
        let file = try String(contentsOf: dir.appendingPathComponent("ai-preferences.json"), encoding: .utf8)
        XCTAssertFalse(file.contains("sk-ant"))
        XCTAssertTrue(file.contains("claude-test"))
        XCTAssertEqual(settings.load(), prefs, "round trip")
        XCTAssertEqual(AISettingsStore.masked("sk-ant-test-1234567890"), "sk-a…7890")
        XCTAssertEqual(AISettingsStore.masked("short"), "•••••")
        // Removing (nil or blank) deletes the item.
        try settings.setAPIKey("   ", for: .anthropic)
        XCTAssertNil(try secrets.read(account: "anthropic-api-key"))
        XCTAssertFalse(settings.hasKey(for: .anthropic))
        // Keyless providers.
        XCTAssertEqual(try settings.provider(.styleProfile, prefs), .styleProfile)
        XCTAssertEqual(try settings.provider(.scripted, prefs), .scripted)
        XCTAssertEqual(try settings.provider(.ollama, prefs), .ollama(host: "http://localhost:11434", model: "qwen2.5:7b", vision: false))
        try settings.setAPIKey(nil, for: .styleProfile)
        XCTAssertEqual(secrets.writes, 2, "keyless providers never touch the store")
    }

    func testPreferencesToleratePartialFilesAndClampGuardrails() throws {
        let (settings, _, dir) = try store()
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        try Data(#"{"provider":"ollama","maxIterations":99,"keepAbove":0.9}"#.utf8)
            .write(to: dir.appendingPathComponent("ai-preferences.json"))
        let prefs = settings.load()
        XCTAssertEqual(prefs.provider, .ollama)
        XCTAssertEqual(prefs.maxIterations, 20)
        XCTAssertEqual(prefs.keepAbove, 0.9)
        XCTAssertEqual(prefs.openAIModel, AIPreferences().openAIModel, "missing members keep defaults")
        XCTAssertEqual(prefs.guardrails.maxIterations, 20)
        XCTAssertFalse(prefs.guardrails.allowSkinRetouch)
        XCTAssertEqual(prefs.assistMode, .automated(rejectBelow: 0.25, keepAbove: 0.9))
        var assisted = prefs
        assisted.assistAutomated = false
        XCTAssertEqual(assisted.assistMode, .assisted)
        try Data("not json".utf8).write(to: dir.appendingPathComponent("ai-preferences.json"))
        XCTAssertEqual(settings.load(), AIPreferences(), "a corrupt file falls back to defaults")
    }

    func testKeychainStoreRoundTripUsesItsOwnService() throws {
        // A throwaway service name: never the app's real items.
        let keychain = KeychainSecretStore(service: "dev.tessera.tests.\(UUID().uuidString)")
        do {
            XCTAssertNil(try keychain.read(account: "probe"))
            try keychain.write("value-1", account: "probe")
        } catch let error as SecretStoreError {
            throw XCTSkip("Keychain unavailable in this environment: \(error.localizedDescription)")
        }
        XCTAssertEqual(try keychain.read(account: "probe"), "value-1")
        try keychain.write("value-2", account: "probe")
        XCTAssertEqual(try keychain.read(account: "probe"), "value-2", "update in place")
        try keychain.write(nil, account: "probe")
        XCTAssertNil(try keychain.read(account: "probe"))
        try keychain.write(nil, account: "probe")   // deleting twice is fine
    }
}

final class AssistBridgeTests: XCTestCase {
    private var root: URL {
        URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    }

    /// Sharp (grainy) and soft (smooth) frames: real analysis tells them apart.
    private func folder() throws -> (URL, URL) {
        let temp = root.appendingPathComponent("build/assist-test-\(UUID().uuidString)")
        let folder = temp.appendingPathComponent("shoot")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: temp) }
        for (name, sharp, x) in [("s1.jpg", true, 0), ("s2.jpg", true, 1), ("soft.jpg", false, 0)] {
            let w = 320, h = 200
            var pixels = [UInt8](repeating: 0, count: w * h * 4)
            for y in 0..<h {
                for xx in 0..<w {
                    let v: Int = sharp ? (((xx + x) / 2 + y / 2) % 2 == 0 ? 50 : 200) : 90 + xx / 16
                    let i = (y * w + xx) * 4
                    pixels[i] = UInt8(v); pixels[i + 1] = UInt8(v); pixels[i + 2] = UInt8(v); pixels[i + 3] = 255
                }
            }
            let provider = try XCTUnwrap(CGDataProvider(data: Data(pixels) as CFData))
            let image = try XCTUnwrap(CGImage(width: w, height: h, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: w * 4,
                                              space: CGColorSpace(name: CGColorSpace.sRGB)!,
                                              bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
                                              provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent))
            let dest = try XCTUnwrap(CGImageDestinationCreateWithURL(folder.appendingPathComponent(name) as CFURL,
                                                                     UTType.jpeg.identifier as CFString, 1, nil))
            CGImageDestinationAddImage(dest, image, [kCGImageDestinationLossyCompressionQuality: 0.95] as CFDictionary)
            XCTAssertTrue(CGImageDestinationFinalize(dest))
        }
        return (folder, temp.appendingPathComponent("support"))
    }

    func testAutomatedSuggestionsConfirmAndFacesThroughTheController() throws {
        let (folder, support) = try folder()
        let library = try EngineLibrary.scan(folder: folder, appSupport: support)
        let cull = library.makeCullController()
        XCTAssertEqual(library.analyze(Array(library.items.indices), faces: false).errors, [])
        let soft = try XCTUnwrap(library.items.first { $0.name == "soft.jpg" }?.id)
        let sharp = try XCTUnwrap(library.items.first { $0.name == "s1.jpg" }?.id)

        try cull.setAssistMode(.automated(rejectBelow: 0.3, keepAbove: 0.55))
        let review = try cull.review()
        XCTAssertEqual(review.count, 3)
        XCTAssertEqual(review.last?.itemID, soft, "likely rejects last")
        XCTAssertEqual(review.first { $0.itemID == soft }?.suggested, .reject)
        XCTAssertEqual(review.first { $0.itemID == sharp }?.suggested, .keep)
        XCTAssertTrue(review.first { $0.itemID == soft }!.explanationText.hasPrefix("sharpness"))

        try cull.dismissSuggestions([sharp])
        XCTAssertNil(try cull.review().first { $0.itemID == sharp }?.suggested)
        let pending = try cull.review().filter { $0.suggested != nil }.map(\.itemID)
        let change = try cull.confirmSuggestions(pending)
        XCTAssertEqual(Set(change.ids), Set(pending))
        XCTAssertEqual(cull[soft].decision, .reject)
        XCTAssertEqual(cull[sharp].decision, .undecided, "dismissed: nothing decided")
        XCTAssertEqual(try cull.assistStatus().labels, UInt64(pending.count))
        XCTAssertNotNil(try cull.undo())
        XCTAssertEqual(cull[soft].decision, .undecided, "one undo step")

        // Synthetic faces (the --seed-faces aid) feed the strip, people and the per-person filter.
        try library.seedSyntheticFaces()
        let strip = try cull.faceStrip(0)
        XCTAssertFalse(strip.isEmpty)
        XCTAssertEqual(strip[0].rect.minX, 180.0 / 1024, accuracy: 1e-9)
        let people = try cull.people(refresh: true)
        XCTAssertEqual(people.first?.items.count, 3, "person A is in every frame")
        XCTAssertEqual(try cull.items(withPerson: people[0].id).count, 3)
        XCTAssertEqual(strip[0].focusLevel, .good)
        XCTAssertEqual(try cull.faceStrip(2)[0].focusLevel, .poor, "frame 2 is out of focus (n % 5 == 2)")
    }
}
