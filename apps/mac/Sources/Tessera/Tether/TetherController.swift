import AppKit
import ImageIO
import Observation
import TesseraCore
import TesseraFFI

/// Tethered capture (WP M3-12b, docs/01 §2.21, docs/06 §3 "tethered/ingest culling").
///
/// The engine owns the camera session (`Engine.tether*`, main thread only): downloads land in a
/// private staging folder, are renamed into the session folder, indexed, previewed and scored
/// before a frame is published. This controller polls it on a 100 ms main-thread timer, keeps
/// the incoming strip and files each frame into the session's album. Frames join the open
/// library in place through the engine's change feed (`AppModel.syncLibrary`), so the undo
/// history, filters and selection survive every frame; they can be culled with the usual keys
/// at once (auto-advance opens the newest in the loupe).
///
/// Hidden test aid: `--fake-tether <folder>` swaps the camera for a test camera that shoots that
/// folder's images on Capture and every `--fake-tether-interval <s>` seconds (default 6; 0 = only
/// on Capture).
@MainActor @Observable
final class TetherController {
    @ObservationIgnored weak var app: AppModel?

    var showPanel = false
    private(set) var devices: [TetherDevice] = []
    private(set) var searching = false
    private(set) var searched = false
    private(set) var connecting = false
    private(set) var connected = false
    /// The camera the session opened (for the header while connected).
    private(set) var connectedDevice: TetherDevice?
    var sessionName = TetherNaming.defaultSessionName()
    /// Where session folders are made; nil: inside the open folder (or ~/Pictures/Tessera Tether).
    var parentFolder: URL?
    var template = TetherNaming.defaultTemplate
    /// Open each new frame in the loupe as it arrives.
    var autoAdvance = true
    var intervalSeconds: Double = 10
    var intervalCount = 10
    /// Observed by the incoming strip; cull states themselves live in an unobserved controller.
    private(set) var decisionRevision = 0
    private(set) var stripStates: [String: CullState] = [:]
    private(set) var interval: IntervalPlan?
    private(set) var strip = IncomingStrip()
    private(set) var thumbnails: [UInt64: CGImage] = [:]
    /// Inline error (no camera, permission, capture refused, disconnect).
    private(set) var error: String?
    private(set) var notice: String?
    private(set) var sessionFolder: URL?
    private(set) var sessionAlbum: String?
    private(set) var sessionSmartAlbum: (id: Int64, name: String)?

    let fakeSource: URL?
    let fakeInterval: Double

    @ObservationIgnored private var engine: Engine?
    @ObservationIgnored private var discoveryEngine: Engine?
    @ObservationIgnored private var pollTimer: Timer?
    @ObservationIgnored private var intervalTimer: Timer?
    @ObservationIgnored private var pollFailures = 0
    @ObservationIgnored private var sessionAlbumID: Int64?
    /// Newest frame (engine image id) to open once it is in the library (auto-advance).
    @ObservationIgnored private var pendingFocus: String?
    /// Filed frames not yet shown by the current source (they reach the library first).
    @ObservationIgnored private var pendingAdmit: Set<String> = []

    static let smartAlbumName = "Session"
    static let smartAlbumRule = "decision!=reject"
    static let intervalOptions: [Double] = [2, 5, 10, 15, 30, 60, 120]
    static let countOptions: [Int] = [5, 10, 20, 50, 100, 0]

    func commitIntervalCount(_ text: String) {
        guard let value = Int(text.trimmingCharacters(in: .whitespacesAndNewlines)) else { return }
        intervalCount = min(999, max(0, value))
    }

    func decisionsDidChange() {
        guard let app else { return }
        stripStates = Dictionary(uniqueKeysWithValues: strip.frames.compactMap { frame in
            guard let key = frame.imageID, let id = item(for: frame) else { return nil }
            return (key, app.state(id: id))
        })
        decisionRevision &+= 1
    }

    func state(for frame: IncomingFrame) -> CullState? {
        guard let key = frame.imageID else { return nil }
        return stripStates[key]
    }

    init(arguments: [String] = ProcessInfo.processInfo.arguments) {
        func value(_ flag: String) -> String? {
            guard let i = arguments.firstIndex(of: flag), i + 1 < arguments.count else { return nil }
            return arguments[i + 1]
        }
        fakeSource = value("--fake-tether").map { URL(fileURLWithPath: ($0 as NSString).expandingTildeInPath, isDirectory: true) }
        fakeInterval = value("--fake-tether-interval").flatMap(Double.init) ?? 6
    }

    var isFake: Bool { fakeSource != nil }

    // MARK: Folder and naming

    /// Folder that session folders are created in.
    var baseFolder: URL {
        if let parentFolder { return parentFolder }
        if let app, app.isEngineBacked, let folder = app.library.folder { return folder }
        return FileManager.default.urls(for: .picturesDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Tessera Tether", isDirectory: true)
    }

    var plannedFolder: URL {
        sessionFolder ?? baseFolder.appendingPathComponent(TetherNaming.sessionFolderName(sessionName), isDirectory: true)
    }

    /// The live example under the naming field.
    var namingExample: Result<String, TetherNaming.Problem> { TetherNaming.example(template) }

    /// The + menu. The naming field may still be editing, its field editor holding the text: end
    /// editing first (committing it into `template`), or the field would write its stale text
    /// back over the token when it later loses focus (on Connect).
    func insertToken(_ token: String) {
        NSApp.keyWindow?.makeFirstResponder(nil)
        template = TetherNaming.inserting(token, into: template)
    }

    func resetTemplate() {
        NSApp.keyWindow?.makeFirstResponder(nil)
        template = TetherNaming.defaultTemplate
    }

    /// Whether the session folder is inside the open library (frames join it; otherwise the
    /// session folder is opened as the library when the session starts).
    func isInsideLibrary(_ folder: URL) -> Bool {
        guard let app, app.isEngineBacked, let root = app.library.folder else { return false }
        let r = Self.resolved(root), f = Self.resolved(folder)
        return f == r || f.hasPrefix(r.hasSuffix("/") ? r : r + "/")
    }

    static func resolved(_ url: URL) -> String { url.standardizedFileURL.resolvingSymlinksInPath().path }

    func chooseParentFolder() {
        let panel = NSOpenPanel()
        panel.title = "Session Location"
        panel.message = "Choose where the session folder is created. Inside the open folder, frames join this library."
        panel.prompt = "Choose"
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = true
        panel.directoryURL = baseFolder
        guard let window = app?.mainWindow else { return }
        panel.beginSheetModal(for: window) { [weak self] response in
            guard response == .OK, let url = panel.url else { return }
            MainActor.assumeIsolated { self?.parentFolder = url }
        }
    }

    // MARK: Panel

    func togglePanel() {
        showPanel.toggle()
        if showPanel, !connected, !searched { refreshDevices() }
    }

    private func discovery() throws -> Engine {
        if let engine { return engine }
        if let lib = app?.library as? EngineLibrary { return lib.engine }
        if let discoveryEngine { return discoveryEngine }
        let e = try Engine.open(appSupportDir: EngineLibrary.defaultSupportDirectory.path)
        discoveryEngine = e
        return e
    }

    private func configureBackend(_ engine: Engine) throws {
        try engine.tetherUseFake(sourceFolder: fakeSource?.path, intervalMs: UInt64(max(0, fakeInterval) * 1000))
    }

    /// Discovery pumps the camera run loop for up to 2 s on the main thread, so the "Looking…"
    /// state is drawn first.
    func refreshDevices() {
        guard !searching else { return }
        searching = true
        error = nil
        DispatchQueue.main.async {
            MainActor.assumeIsolated {
                defer { self.searching = false; self.searched = true }
                do {
                    let engine = try self.discovery()
                    try self.configureBackend(engine)
                    self.devices = try engine.tetherDevices()
                    if let d = self.connectedDevice, let fresh = self.devices.first(where: { $0.id == d.id }) {
                        self.connectedDevice = fresh
                    }
                } catch {
                    self.devices = []
                    self.error = Self.explain(Self.message(error))
                }
            }
        }
    }

    /// Adds a next step to permission failures (colour is never the only signal; neither is jargon).
    /// An engine failure's own words (the generated `localizedDescription` spells out the type).
    static func message(_ error: Error) -> String {
        if case BridgeError.Failure(let message) = error { return message }
        return error.localizedDescription
    }

    static func explain(_ message: String) -> String {
        let m = message.lowercased()
        if m.contains("denied") || m.contains("not authorized") || m.contains("permission") || m.contains("not permitted") {
            return message + ". Allow Tessera to use the camera in System Settings ▸ Privacy & Security, then try again."
        }
        return message
    }

    // MARK: Session

    var canConnect: Bool {
        // A camera without remote capture still tethers: its own shutter button delivers frames.
        !connected && !connecting && devices.count == 1
            && (try? namingExample.get()) != nil
            && !TetherNaming.sessionFolderName(sessionName).isEmpty
    }

    func connect() {
        guard !connected, !connecting, let app else { return }
        if case .failure(let problem) = namingExample { error = "Naming: \(problem)"; return }
        let folder = baseFolder.appendingPathComponent(TetherNaming.sessionFolderName(sessionName), isDirectory: true)
        do {
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        } catch {
            self.error = Self.explain("Could not create the session folder: \(Self.message(error))")
            return
        }
        error = nil
        notice = nil
        connecting = true
        NSApp.keyWindow?.makeFirstResponder(nil)   // culling keys go to the grid, not a field
        if isInsideLibrary(folder), let lib = app.library as? EngineLibrary {
            DispatchQueue.main.async { MainActor.assumeIsolated { self.start(folder: folder, engine: lib.engine) } }
        } else {
            // A folder outside the open library: open the session folder as the library first.
            app.openFolder(folder, message: "Tether session folder \(folder.lastPathComponent)") { app, loaded in
                guard let lib = app.library as? EngineLibrary, loaded else {
                    self.connecting = false
                    self.error = "Could not open the session folder as a library"
                    return
                }
                self.start(folder: folder, engine: lib.engine)
            }
        }
    }

    /// `--tether-connect`: connects once the folder has loaded and discovery found one camera.
    func connectWhenReady(attempts: Int = 0) {
        guard let app, attempts < 80 else { return }
        if app.isLoading || searching || !searched {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.25) {
                MainActor.assumeIsolated { self.connectWhenReady(attempts: attempts + 1) }
            }
            return
        }
        if canConnect { connect() }
    }

    private func start(folder: URL, engine: Engine) {
        defer { connecting = false }
        do {
            try configureBackend(engine)
            try engine.tetherStart(sessionFolder: folder.path, naming: template)
        } catch {
            self.error = Self.explain(Self.message(error))
            return
        }
        self.engine = engine
        sessionFolder = folder
        connected = true
        connectedDevice = devices.first
        strip.reset()
        stripStates = [:]
        thumbnails = [:]
        pollFailures = 0
        pendingFocus = nil
        pendingAdmit = []
        setUpCollections()
        let timer = Timer(timeInterval: 0.1, repeats: true) { _ in
            MainActor.assumeIsolated { [weak self] in self?.poll() }
        }
        RunLoop.main.add(timer, forMode: .common)
        pollTimer = timer
        app?.statusMessage = "Tethered to \(connectedDevice?.name ?? "the camera") · saving to \(folder.lastPathComponent)"
            + (isFake ? " (test camera)" : "")
    }

    /// Group "<session>", album "<session> captures" (every frame, in arrival order: the live
    /// source) and a smart album "Session" scoped to the group (this shoot, rejects hidden).
    private func setUpCollections() {
        guard let app, let catalog = app.collections.catalog else {
            notice = "Albums are unavailable for this folder; frames still arrive in the session folder."
            return
        }
        let name = sessionName.trimmingCharacters(in: .whitespacesAndNewlines)
        let albumName = "\(name) captures"
        do {
            var nodes = app.collections.flatNodes
            let group = try nodes.first { $0.kind == .group && $0.name == name }?.id
                ?? catalog.store.createGroup(name: name, parent: nil)
            let album = try nodes.first { $0.kind == .album && $0.name == albumName }?.id
                ?? catalog.store.createAlbum(name: albumName, parent: group)
            nodes = CollectionNode.flatten(try catalog.nodes())
            let smart = try nodes.first { $0.kind == .smartAlbum && $0.parent == group }?.id
                ?? catalog.store.createSmartAlbum(name: Self.smartAlbumName, rule: Self.smartAlbumRule, parent: group, scoped: true)
            sessionAlbumID = album
            sessionAlbum = albumName
            sessionSmartAlbum = (smart, Self.smartAlbumName)
            app.collections.reloadNodes()
            app.libraryDidChange()
            app.setSource(.album(albumName))
        } catch {
            notice = "Could not create the session albums: \(Self.message(error))"
        }
    }

    func disconnect() {
        stopInterval()
        pollTimer?.invalidate()
        pollTimer = nil
        guard connected, let engine else { connected = false; return }
        do {
            let tail = try engine.tetherStop()
            receive(tail)
        } catch {
            self.error = "Camera closed with an error: \(Self.message(error))"
        }
        connected = false
        self.engine = nil
        app?.statusMessage = "Tether session ended: \(strip.summary)"
        searched = false
        if showPanel { refreshDevices() }
    }

    func poll() {
        guard connected, let engine else { return }
        do {
            let frames = try engine.tetherPoll()
            pollFailures = 0
            receive(frames)
        } catch {
            pollFailures += 1
            self.error = Self.explain("Camera: \(Self.message(error))")
            // A second of consecutive failures (unplugged, switched off) ends the session.
            if pollFailures >= 10 {
                disconnect()
                self.error = Self.explain("Camera disconnected: \(Self.message(error))")
            }
        }
    }

    // MARK: Capture

    func capture() {
        guard connected, let engine else {
            app?.statusMessage = "Connect a camera first: File ▸ Tethered Capture…"
            showPanel = true
            return
        }
        do {
            try engine.tetherCapture()
            strip.captureRequested()
            error = nil
        } catch {
            self.error = Self.explain("Capture: \(Self.message(error))")
            stopInterval()
        }
    }

    func startInterval() {
        guard connected, interval == nil else { return }
        var plan = IntervalPlan(seconds: intervalSeconds, count: intervalCount)
        _ = plan.shoot()
        interval = plan
        capture()
        guard interval != nil else { return }
        let timer = Timer(timeInterval: plan.seconds, repeats: true) { _ in
            MainActor.assumeIsolated { [weak self] in self?.intervalTick() }
        }
        RunLoop.main.add(timer, forMode: .common)
        intervalTimer = timer
    }

    private func intervalTick() {
        guard var plan = interval else { return }
        guard plan.shoot() else { stopInterval(); return }
        interval = plan
        capture()
        if plan.isFinished { stopInterval() }
    }

    func stopInterval() {
        intervalTimer?.invalidate()
        intervalTimer = nil
        interval = nil
    }

    // MARK: Frames

    private func receive(_ raw: [TetherFrame]) {
        guard !raw.isEmpty else { return }
        let frames = raw.map(IncomingFrame.init)
        let target = strip.receive(frames)
        loadThumbnails(frames)
        if let failed = frames.last(where: \.failed) {
            error = "Frame \(String(format: "%04llu", failed.sequence)) failed: \(failed.error ?? "")"
        }
        let ids = frames.compactMap(\.imageID)
        if let album = sessionAlbumID, !ids.isEmpty, let catalog = app?.collections.catalog {
            do { try catalog.store.addToAlbum(id: album, imageIds: ids) } catch {
                notice = "Could not add frames to \(sessionAlbum ?? "the session album"): \(Self.message(error))"
            }
        }
        if isFake, connected { refreshFakeDevice() }
        if autoAdvance, let id = target?.imageID { pendingFocus = id }
        if let latest = strip.latest { app?.statusMessage = "Tether: \(latest.name) · \(strip.summary)" }
        pendingAdmit.formUnion(ids)
        refreshLibrary()
    }

    /// The test camera's "shots remaining" is cheap to read; a real camera is not polled here.
    private func refreshFakeDevice() {
        guard let engine, let d = try? engine.tetherDevices().first else { return }
        connectedDevice = d
        devices = [d]
    }

    private func loadThumbnails(_ frames: [IncomingFrame]) {
        for frame in frames {
            guard let url = frame.preview ?? (frame.failed ? nil : frame.url) else { continue }
            let seq = frame.sequence
            Task.detached(priority: .userInitiated) {
                let image = Self.thumbnail(url)
                await MainActor.run { [weak self] in
                    guard let self, let image, self.strip.frames.contains(where: { $0.sequence == seq }) else { return }
                    self.thumbnails[seq] = image
                }
            }
        }
    }

    nonisolated static func thumbnail(_ url: URL, maxPixels: Int = 240) -> CGImage? {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil) else { return nil }
        let options: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceThumbnailMaxPixelSize: maxPixels,
        ]
        return CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary)
    }

    // MARK: Library

    /// New frames reach the open library through the change feed, in place. Frames already
    /// applied are filed into view now; the rest when the pull lands (`libraryDidUpdate`).
    private func refreshLibrary() {
        guard let app, let session = sessionFolder, isInsideLibrary(session) else {
            if let session = sessionFolder, app?.library.folder != nil, !isInsideLibrary(session) {
                notice = "The open folder is not the session's; frames are saved to \(session.lastPathComponent)."
            }
            return
        }
        libraryDidUpdate()
        app.syncLibrary { [weak self] in self?.libraryDidUpdate() }
    }

    /// The library changed in place (a pull landed): show filed frames the source takes, and
    /// open the newest in the loupe when auto-advance asked for it.
    func libraryDidUpdate() {
        guard let app, let lib = app.engineLibrary else { return }
        let known = pendingAdmit.filter { lib.itemOfImage[$0] != nil }
        if !known.isEmpty {
            pendingAdmit.subtract(known)
            app.admit(known.compactMap { lib.itemOfImage[$0] })
        }
        if let key = pendingFocus, let id = lib.itemOfImage[key] {
            pendingFocus = nil
            app.select(id: id)
            if app.focusedItem?.id == id, app.viewMode != .compare { app.viewMode = .loupe }
        }
        decisionsDidChange()
    }

    func item(for frame: IncomingFrame) -> Int? {
        guard let lib = app?.engineLibrary, let id = frame.imageID else { return nil }
        return lib.itemOfImage[id]
    }

    /// Click on an incoming tile: focus that frame (the usual keys then decide it).
    func reveal(_ frame: IncomingFrame, loupe: Bool) {
        guard let app, let id = item(for: frame) else {
            app?.statusMessage = frame.failed ? "Frame \(frame.name) failed: \(frame.error ?? "")"
                : "\(frame.name) is not in the open library yet"
            return
        }
        if loupe { app.showInLoupe(id) } else {
            app.select(id: id)
            if app.focusedItem?.id != id, let album = sessionAlbum {
                app.setSource(.album(album))
                app.select(id: id)
            }
        }
        NSApp.keyWindow?.makeFirstResponder(nil)
    }

    func showSessionSmartAlbum() {
        guard let s = sessionSmartAlbum else { return }
        app?.setSource(.smartAlbum(id: s.id, name: s.name))
    }
}
