import XCTest
@testable import TesseraCore
@testable import Tessera

/// OF-2 (docs/design/offline/SPEC.md): the background builder against a fake `SmartPreviewAPI`,
/// the reconnect sync counts, the automatic route to the preview, the settings defaults and the
/// copy people read.
@MainActor
final class OfflinePreviewsTests: XCTestCase {
    private enum Failure: Error { case refused }

    /// An engine stand-in: candidates are what has no preview yet (failed ones stay candidates,
    /// as the engine's rule says), ensure appends to `ensured` and grows `storage`.
    @MainActor
    private final class Fake {
        var candidates: [String]
        var ensured: [String] = []
        var ensureStarts = 0
        var storage: UInt64 = 0
        var bytesPerPreview: UInt64 = 10_000_000
        var failing: Set<String> = []
        var ensureDelay: Duration = .zero
        var coverageReads = 0
        var pending: [String] = []
        var synchronized: [String] = []
        var syncFailing: Set<String> = []
        /// A status that is not the plain rule (a pending id is dirty, the rest ready).
        var infoOverrides: [String: SmartPreviewSnapshot] = [:]

        init(_ candidates: [String]) { self.candidates = candidates }

        func snapshot(_ id: String, state: SmartPreviewSnapshot.State = .ready, online: Bool = true, dirty: Bool = false,
                      message: String = "") -> SmartPreviewSnapshot {
            .init(imageID: id, state: state, originalAvailable: online, dirty: dirty, width: 2048, height: 1365, message: message)
        }

        var api: SmartPreviewAPI {
            SmartPreviewAPI(
                info: { id in
                    self.infoOverrides[id] ?? self.snapshot(id, state: self.pending.contains(id) ? .dirty : .ready,
                                                            dirty: self.pending.contains(id))
                },
                build: { id in self.snapshot(id) },
                discard: { _ in },
                synchronize: { id in
                    self.synchronized.append(id)
                    if self.syncFailing.contains(id) { throw Failure.refused }
                    return self.snapshot(id)
                },
                coverage: { folder in
                    self.coverageReads += 1
                    return OfflineCoverage(folder: folder, raws: UInt64(self.candidates.count), ready: UInt64(self.ensured.count),
                                           pendingSync: 0, needsAttention: 0, bytes: self.storage)
                },
                candidates: { _, limit in
                    let remaining = self.candidates.filter { !self.ensured.contains($0) }
                    return limit == 0 ? remaining : Array(remaining.prefix(Int(limit)))
                },
                ensure: { id in
                    self.ensureStarts += 1
                    if self.ensureDelay > .zero { try await Task.sleep(for: self.ensureDelay) }
                    if self.failing.contains(id) { throw Failure.refused }
                    self.ensured.append(id)
                    self.storage += self.bytesPerPreview
                    return self.snapshot(id)
                },
                pendingSync: { self.pending },
                storage: { self.storage })
        }
    }

    private static let fast = OfflinePreviews.Timing(startDelay: .milliseconds(1), idleRetry: .milliseconds(5), frameQuiet: .milliseconds(150))

    private func defaults() throws -> UserDefaults {
        let suite = "OfflinePreviewsTests.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        addTeardownBlock { defaults.removePersistentDomain(forName: suite) }
        return defaults
    }

    private func controller(_ fake: Fake, gate: @escaping @MainActor () -> String? = { nil },
                            isKeeper: @escaping @MainActor (String) -> Bool = { _ in false },
                            budget: UInt64? = nil, mode: OfflinePreviewsMode = .everyRaw,
                            timing: OfflinePreviews.Timing = fast) throws -> OfflinePreviews {
        let defaults = try defaults()
        OfflinePreviewsSettings.write(mode, to: defaults)
        if let budget { OfflinePreviewsSettings.write(budget: budget, to: defaults) }
        return OfflinePreviews(api: fake.api, folder: "/Volumes/Rut photo/2026", isKeeper: isKeeper, gate: gate,
                               timing: timing, defaults: defaults, freeBytes: 1_000 * OfflinePreviewsSettings.gigabyte)
    }

    private func wait(_ what: String, timeout: Duration = .seconds(3), until condition: @MainActor () -> Bool) async {
        let clock = ContinuousClock()
        let deadline = clock.now + timeout
        while !condition() {
            if clock.now > deadline { return XCTFail("timed out waiting for \(what)") }
            try? await Task.sleep(for: .milliseconds(5))
        }
    }

    // MARK: Queue

    func testBuildsCandidatesInEngineOrderAndRefreshesCoverage() async throws {
        let fake = Fake(["keep-new", "keep-old", "new", "old"])
        let previews = try controller(fake)
        var built: [String] = []
        previews.onPhotoBuilt = { built.append($0) }
        previews.start()
        await wait("queue to finish") { previews.finished }
        XCTAssertEqual(fake.ensured, ["keep-new", "keep-old", "new", "old"], "one at a time, in the engine's order")
        XCTAssertEqual(built, fake.ensured)
        XCTAssertEqual(previews.built, 4)
        XCTAssertEqual(previews.coverage?.ready, 4)
        XCTAssertEqual(previews.coverageText, "4 of 4 editable offline")
        XCTAssertFalse(previews.isBuilding)
        XCTAssertNil(previews.current)
        XCTAssertTrue(previews.statusText.hasPrefix("Up to date"), previews.statusText)
    }

    func testGateHoldsTheQueueAndResumesOnItsOwn() async throws {
        let fake = Fake(["a", "b"])
        var reason: String? = "the folder is being measured for culling"
        let previews = try controller(fake, gate: { reason })
        previews.start()
        try await Task.sleep(for: .milliseconds(60))
        XCTAssertTrue(fake.ensured.isEmpty, "nothing builds while a gate names a reason")
        XCTAssertEqual(previews.waitingReason, reason)
        XCTAssertTrue(previews.statusText.contains("Waiting"), previews.statusText)
        reason = nil
        await wait("resume") { previews.finished }
        XCTAssertEqual(fake.ensured, ["a", "b"])
        XCTAssertNil(previews.waitingReason)
    }

    func testGateIsReadBetweenPhotosNotOnlyAtStart() async throws {
        let fake = Fake(["a", "b", "c"])
        var reason: String?
        let previews = try controller(fake, gate: { reason })
        previews.onPhotoBuilt = { id in if id == "a" { reason = "an export or print is running" } }
        previews.start()
        await wait("first photo") { fake.ensured == ["a"] }
        try await Task.sleep(for: .milliseconds(60))
        XCTAssertEqual(fake.ensured, ["a"], "the export gate stops the queue after the current photo")
        reason = nil
        await wait("finish") { previews.finished }
        XCTAssertEqual(fake.ensured, ["a", "b", "c"])
    }

    func testLoupeFrameKeepsTheQueueQuiet() async throws {
        let fake = Fake(["a"])
        let previews = try controller(fake)
        previews.noteLoupeFrame()
        previews.start()
        try await Task.sleep(for: .milliseconds(60))
        XCTAssertTrue(fake.ensured.isEmpty, "quiet for Timing.frameQuiet after a frame")
        await wait("quiet period to end") { previews.finished }
        XCTAssertEqual(fake.ensured, ["a"])
    }

    func testBudgetStopsTheQueueBeforeThePhotoThatWouldNotFit() async throws {
        let fake = Fake(["a", "b", "c", "d"])
        // 10 MB a preview, 12 MB headroom: 0 and 10 MB fit under 25 MB, 20 MB does not.
        let previews = try controller(fake, budget: 25_000_000)
        previews.start()
        await wait("budget stop") { previews.budgetReached }
        XCTAssertEqual(fake.ensured, ["a", "b"])
        XCTAssertFalse(previews.finished)
        XCTAssertEqual(previews.coverageText, "2 of 4 editable offline · budget reached")
        XCTAssertTrue(previews.statusText.hasPrefix("Budget reached"), previews.statusText)
        XCTAssertFalse(previews.isRunning)
        // A bigger budget starts it again.
        previews.setBudget(60_000_000)
        await wait("finish after raise") { previews.finished }
        XCTAssertEqual(fake.ensured, ["a", "b", "c", "d"])
    }

    func testCancelOnLibraryChangeStopsAfterTheCurrentPhoto() async throws {
        let fake = Fake(["a", "b", "c"])
        fake.ensureDelay = .milliseconds(40)
        let previews = try controller(fake)
        previews.start()
        await wait("first build to start") { previews.isBuilding }
        XCTAssertEqual(previews.current, "a")
        previews.cancel()
        XCTAssertFalse(previews.isBuilding)
        XCTAssertFalse(previews.isRunning)
        try await Task.sleep(for: .milliseconds(150))
        XCTAssertEqual(fake.ensureStarts, 1, "no photo starts after cancel")
        XCTAssertFalse(previews.finished)
        previews.start()
        previews.buildNow()
        try await Task.sleep(for: .milliseconds(30))
        XCTAssertEqual(fake.ensureStarts, 1, "a cancelled controller never restarts")
    }

    func testBuildNowIgnoresIdleGatesAndTheOffModeButNotTheBudget() async throws {
        let fake = Fake(["a", "b", "c"])
        let previews = try controller(fake, gate: { "a photo is opening" }, budget: 25_000_000, mode: .off)
        previews.noteLoupeFrame()
        previews.start()
        XCTAssertFalse(previews.isRunning, "off: nothing starts on its own")
        previews.buildNow()
        await wait("budget stop") { previews.budgetReached }
        XCTAssertEqual(fake.ensured, ["a", "b"], "the gates were ignored, the budget was not")
        XCTAssertNil(previews.waitingReason)
    }

    func testKeepersOnlyEndsAtTheFirstCandidateThatIsNotAKeep() async throws {
        let fake = Fake(["keep-1", "keep-2", "undecided-1", "undecided-2"])
        let previews = try controller(fake, isKeeper: { $0.hasPrefix("keep") }, mode: .keepersOnly)
        previews.start()
        await wait("finish") { previews.finished }
        XCTAssertEqual(fake.ensured, ["keep-1", "keep-2"])
    }

    func testRefusedPhotoIsSkippedOnceAndReported() async throws {
        let fake = Fake(["a", "b", "c"])
        fake.failing = ["b"]
        let previews = try controller(fake)
        previews.start()
        await wait("finish") { previews.finished }
        XCTAssertEqual(fake.ensured, ["a", "c"])
        XCTAssertEqual(fake.ensureStarts, 3, "the refused photo is asked once, not forever")
        XCTAssertEqual(previews.built, 2)
    }

    func testOffModeStillReadsCoverageForThePopover() async throws {
        let fake = Fake(["a"])
        let previews = try controller(fake, mode: .off)
        previews.start()
        await wait("coverage") { previews.coverage != nil }
        XCTAssertEqual(previews.coverageText, "0 of 1 editable offline")
        XCTAssertTrue(fake.ensured.isEmpty)
        XCTAssertTrue(previews.statusText.hasPrefix("Off"), previews.statusText)
        previews.setMode(.everyRaw)
        await wait("finish after switching on") { previews.finished }
        XCTAssertEqual(fake.ensured, ["a"])
    }

    func testOfflineLibraryReadsCoverageButBuildsNothing() async throws {
        let fake = Fake(["a"])
        let previews = OfflinePreviews(api: fake.api, folder: "/Volumes/Rut photo/2026", originalAvailable: false,
                                       timing: Self.fast, defaults: try defaults(), freeBytes: 0)
        previews.start()
        await wait("coverage") { previews.coverage != nil }
        XCTAssertFalse(previews.canBuild)
        XCTAssertFalse(previews.isRunning)
        XCTAssertTrue(fake.ensured.isEmpty)
        XCTAssertEqual(previews.statusText, "Open a folder on an attached drive to make previews")
    }

    func testEngineWithoutOfflineMethodsLeavesTheBuilderIdleWithAMessage() async throws {
        // The seam before OF-1 lands: the default closures refuse, nothing crashes or spins.
        let api = SmartPreviewAPI(info: { id in Fake([]).snapshot(id) }, build: { id in Fake([]).snapshot(id) },
                                  discard: { _ in }, synchronize: { id in Fake([]).snapshot(id) })
        let previews = OfflinePreviews(api: api, folder: "/x", timing: Self.fast, defaults: try defaults(), freeBytes: 0)
        previews.start()
        await wait("error") { previews.lastError != nil }
        XCTAssertEqual(previews.lastError, SmartPreviewAPI.offlineUnavailable)
        XCTAssertNil(previews.coverage)
    }

    // MARK: Reconnect sync

    func testReconnectSyncWritesPendingEditsOfTheOpenLibraryAndCountsThem() async throws {
        let fake = Fake([])
        fake.pending = ["a", "open", "fails", "other-folder"]
        fake.syncFailing = ["fails"]
        var refreshed: [String] = []
        let outcome = await OfflineEditSync.run(api: fake.api, libraryIDs: ["a", "open", "fails"],
                                                isOpen: { $0 == "open" },
                                                didSynchronize: { id, _ in refreshed.append(id) })
        XCTAssertEqual(fake.synchronized, ["a", "fails"], "one at a time; the open photo and the other folder wait")
        XCTAssertEqual(outcome, .init(wrote: 1, failed: 1, skipped: 1))
        XCTAssertEqual(refreshed, ["a", "fails"])
        XCTAssertEqual(outcome.toast, "Wrote 1 offline edit to the drive · 1 edit could not be written · see Library ▸ Smart Previews")
    }

    func testReconnectSyncToastCounts() {
        XCTAssertNil(OfflineEditSync.Outcome().toast, "nothing pending: no toast")
        XCTAssertNil(OfflineEditSync.Outcome(skipped: 2).toast, "only open photos: no toast")
        XCTAssertEqual(OfflineEditSync.Outcome(wrote: 3).toast, "Wrote 3 offline edits to the drive")
        XCTAssertEqual(OfflineEditSync.Outcome(failed: 1).toast, "1 edit could not be written · see Library ▸ Smart Previews")
    }

    func testReconnectSyncStopsWhenTheLibraryChanges() async {
        let fake = Fake([])
        fake.pending = ["a", "b", "c"]
        var calls = 0
        let outcome = await OfflineEditSync.run(api: fake.api, libraryIDs: ["a", "b", "c"], isOpen: { _ in false },
                                                shouldContinue: { calls += 1; return calls < 3 })
        XCTAssertEqual(fake.synchronized, ["a", "b"])
        XCTAssertEqual(outcome.wrote, 2)
    }

    // MARK: UX-16 item 29 — a clean preview whose baseline moved is refreshed, never a conflict

    static let engineConflict = "Original sidecars differ from the offline baseline; both copies retained"

    func testReconnectSyncRefreshesACleanBehindPreviewSilently() async {
        let fake = Fake([])
        // The engine's dirty-journal scan listed it earlier; by now the person has edited the
        // original (its sidecars moved) and the journal is clean: `info` says Conflict, not dirty.
        fake.pending = ["behind", "dirty"]
        fake.infoOverrides["behind"] = fake.snapshot("behind", state: .conflict, dirty: false, message: Self.engineConflict)
        var refreshed: [(String, SmartPreviewSnapshot?)] = []
        let outcome = await OfflineEditSync.run(api: fake.api, libraryIDs: ["behind", "dirty"], isOpen: { _ in false },
                                                didSynchronize: { id, info in refreshed.append((id, info)) })
        XCTAssertEqual(fake.synchronized, ["dirty"], "only the journal with edits is written")
        XCTAssertEqual(fake.ensured, ["behind"], "the clean one is rebuilt on the new baseline")
        XCTAssertEqual(outcome, .init(wrote: 1, failed: 0, skipped: 0, refreshed: 1))
        XCTAssertEqual(outcome.toast, "Wrote 1 offline edit to the drive", "nothing about a conflict")
        XCTAssertEqual(refreshed.map(\.0), ["behind", "dirty"], "badges and thumbnails refresh for both")
        XCTAssertNil(OfflineEditSync.Outcome(refreshed: 3).toast, "refreshes alone say nothing")
    }

    func testReconnectSyncLeavesACleanReadyPreviewAlone() async {
        let fake = Fake([])
        fake.pending = ["clean"]
        fake.infoOverrides["clean"] = fake.snapshot("clean")   // synchronized from the menu meanwhile
        let outcome = await OfflineEditSync.run(api: fake.api, libraryIDs: ["clean"], isOpen: { _ in false })
        XCTAssertEqual(fake.synchronized, [])
        XCTAssertEqual(fake.ensured, [])
        XCTAssertEqual(outcome, .init())
    }

    func testSnapshotTellsABehindBaselineFromAConflict() {
        let fake = Fake([])
        let behind = fake.snapshot("a", state: .conflict, dirty: false, message: Self.engineConflict)
        XCTAssertTrue(behind.isBaselineBehind)
        XCTAssertFalse(behind.isConflict)
        XCTAssertTrue(behind.badge.contains("Behind the original"))
        XCTAssertFalse(behind.badge.contains("Conflict"))
        let conflict = fake.snapshot("a", state: .conflict, dirty: true, message: Self.engineConflict)
        XCTAssertFalse(conflict.isBaselineBehind)
        XCTAssertTrue(conflict.isConflict)
        XCTAssertTrue(conflict.badge.contains("Conflict"))
        let offline = fake.snapshot("a", state: .conflict, online: false, dirty: false)
        XCTAssertFalse(offline.isBaselineBehind, "without the original there is no baseline to compare")
    }

    func testOpeningEditOnACleanBehindPreviewOpensTheOriginalWithoutAConflict() throws {
        let fake = Fake([])
        let behind = fake.snapshot("raw", state: .conflict, dirty: false, message: Self.engineConflict)
        XCTAssertEqual(try SmartPreviewRouting.route(behind, preferPreview: false), .original)
        XCTAssertEqual(try SmartPreviewRouting.route(behind, preferPreview: true), .original,
                       "the preview is behind the original; the original is the truth until it is rebuilt")
        let conflict = fake.snapshot("raw", state: .conflict, dirty: true, message: Self.engineConflict)
        XCTAssertThrowsError(try SmartPreviewRouting.route(conflict, preferPreview: false)) { error in
            XCTAssertEqual(error.localizedDescription, Self.engineConflict, "a real conflict still stops the open")
        }
    }

    // MARK: Routing

    func testOriginalOfflineWithAPreviewOpensThePreviewWithoutAMenuTrip() throws {
        let fake = Fake([])
        for dirty in [false, true] {
            let existing = fake.snapshot("raw", state: .originalOffline, online: false, dirty: dirty)
            XCTAssertEqual(try SmartPreviewRouting.route(existing, preferPreview: false), .smartPreview)
            XCTAssertEqual(try SmartPreviewRouting.route(existing, preferPreview: true), .smartPreview)
        }
        XCTAssertEqual(try SmartPreviewRouting.route(fake.snapshot("raw", online: true), preferPreview: false), .original,
                       "with the original attached the explicit choice still rules")
    }

    func testOriginalOfflineWithoutAPreviewNamesTheDriveAndSettings() {
        let missing = Fake([]).snapshot("raw", state: .missing, online: false)
        XCTAssertThrowsError(try SmartPreviewRouting.route(missing, preferPreview: true, volumeName: "Rut photo")) { error in
            XCTAssertEqual(error.localizedDescription,
                           "“Rut photo” isn't attached and this photo has no offline preview yet. Previews are made in the background while the drive is attached (Settings ▸ Offline).")
        }
        XCTAssertThrowsError(try SmartPreviewRouting.route(missing, preferPreview: false)) { error in
            XCTAssertTrue(error.localizedDescription.hasPrefix("The drive isn't attached"), error.localizedDescription)
        }
    }

    func testEditHeaderLabelOnlyWhileThePreviewIsOpen() {
        let model = AppModel()
        XCTAssertNil(model.editingOfflinePreviewLabel)
        XCTAssertEqual(OfflineCopy.editingOfflinePreview, "Editing offline preview")
        XCTAssertNil(model.libraryVolumeName, "no folder, no drive")
    }

    // MARK: Settings

    func testSettingsDefaults() throws {
        let defaults = try defaults()
        XCTAssertEqual(OfflinePreviewsSettings.readMode(from: defaults), .everyRaw)
        XCTAssertNil(defaults.object(forKey: OfflinePreviewsSettings.modeKey), "reading the default stores nothing")
        let gb = OfflinePreviewsSettings.gigabyte
        XCTAssertEqual(OfflinePreviewsSettings.readBudget(from: defaults, freeBytes: 100 * gb), 10 * gb, "10 % of the free space")
        XCTAssertEqual(OfflinePreviewsSettings.readBudget(from: defaults, freeBytes: 1_000 * gb), 25 * gb, "capped at 25 GB")
        XCTAssertNil(defaults.object(forKey: OfflinePreviewsSettings.budgetKey))
        OfflinePreviewsSettings.write(budget: 40 * gb, to: defaults)
        XCTAssertEqual(OfflinePreviewsSettings.readBudget(from: defaults, freeBytes: 100 * gb), 40 * gb, "a stored budget wins")
        OfflinePreviewsSettings.write(.keepersOnly, to: defaults)
        XCTAssertEqual(OfflinePreviewsSettings.readMode(from: defaults), .keepersOnly)
        XCTAssertEqual(OfflinePreviewsSettings.fitsText(budget: 25 * gb), "fits about 2,500 photos")
        XCTAssertTrue(OfflinePreviewsSettings.fits(storage: 13 * gb, budget: 25 * gb))
        XCTAssertFalse(OfflinePreviewsSettings.fits(storage: 25 * gb - 11_000_000, budget: 25 * gb), "storage + 12 MB > budget stops")
        let previews = OfflinePreviews(defaults: try self.defaults(), freeBytes: 100 * gb)
        XCTAssertEqual(previews.mode, .everyRaw)
        XCTAssertEqual(previews.budget, 10 * gb)
        XCTAssertEqual(previews.usageText, "Used: Zero KB")
    }

    func testSettingsWritesPersist() throws {
        let defaults = try defaults()
        let previews = OfflinePreviews(defaults: defaults, freeBytes: 0)
        previews.setMode(.off)
        previews.setBudget(3 * OfflinePreviewsSettings.gigabyte)
        XCTAssertEqual(OfflinePreviewsSettings.readMode(from: defaults), .off)
        XCTAssertEqual(OfflinePreviewsSettings.readBudget(from: defaults, freeBytes: 0), 3 * OfflinePreviewsSettings.gigabyte)
    }

    // MARK: Copy

    func testOfflineCopyReadsAboutTheDriveNeverAboutCaches() throws {
        XCTAssertEqual(OfflineCopy.banner(volume: "Rut photo"),
                       "“Rut photo” isn't attached · editing offline previews · culling resumes when it's back")
        XCTAssertEqual(OfflineCopy.emptyMessage(volume: "Rut photo"),
                       "No offline previews for this folder yet. Attach “Rut photo” and Tessera makes them in the background.")
        XCTAssertEqual(OfflineCopy.editable(ready: 180, raws: 226), "180 of 226 editable offline")
        XCTAssertEqual(OfflineCoverage(folder: "/f", raws: 226, ready: 170, pendingSync: 10, needsAttention: 3, bytes: 0).editableText,
                       "180 of 226 editable offline", "a preview with journaled edits is still editable")
        XCTAssertEqual(OfflineCopy.openedOffline(volume: "Rut photo", previews: 180), "“Rut photo” isn't attached · 180 offline previews ready to edit")
        XCTAssertEqual(OfflineCopy.openedOffline(volume: nil, previews: 1), "The drive isn't attached · 1 offline preview ready to edit")
        var visible = [
            OfflineCopy.banner(volume: nil), OfflineCopy.bannerHelp, OfflineCopy.emptyTitle, OfflineCopy.emptyMessage(volume: nil),
            OfflineCopy.missingPreview(volume: nil), OfflineCopy.editingOfflinePreview, OfflineCopy.openedOffline(volume: nil, previews: 2),
            OfflineCopy.decisionsWait(volume: "Rut photo"), OfflineCopy.budgetReached,
            LibraryAccessMode.cachedSmartPreviews.label, LibraryAccessMode.cachedSmartPreviews.emptyMessage,
        ] + OfflinePreviewsMode.allCases.map(\.title)
        XCTAssertThrowsError(try LibraryAccessMode.cachedSmartPreviews.requireCatalogMutation(volume: "Rut photo")) { visible.append($0.localizedDescription) }
        for text in visible {
            for banned in ["declaration", "cached", "Cached", "read-only", "catalog", "Catalog"] {
                XCTAssertFalse(text.contains(banned), "“\(banned)” in: \(text)")
            }
        }
    }
}
