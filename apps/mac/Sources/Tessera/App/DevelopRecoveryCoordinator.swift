import Foundation
import Observation
import TesseraCore

/// App-lifetime ownership for Develop sessions whose save/close has not finished.
/// MainActor isolation makes publishing a record and reserving admission atomic.
@MainActor @Observable
final class DevelopRecoveryCoordinator {
    struct Key: Hashable {
        let owner: ObjectIdentifier
        let imageID: String

        init(owner: EngineLibrary, imageID: String) {
            self.owner = ObjectIdentifier(owner)
            self.imageID = imageID
        }
    }

    struct SessionID: Hashable { let value: UUID }

    /// Both names are kept: a symlink or hard link can name the same source,
    /// while replacement at the same path must remain conservatively blocked.
    private struct SourceIdentity {
        struct FileID: Equatable {
            let device: UInt64
            let inode: UInt64
        }
        let path: URL
        let fileID: FileID?

        func overlaps(_ other: SourceIdentity) -> Bool {
            path == other.path || (fileID != nil && fileID == other.fileID)
        }
    }

    enum Outcome {
        case saved
        case failed(sessionID: SessionID, message: String)
    }

    enum BarrierOutcome {
        case saved
        case blocked([SessionID])

        var isSaved: Bool {
            if case .saved = self { return true }
            return false
        }
    }

    enum Phase {
        case active
        case saving
        case failed(String)
    }

    struct Presentation: Identifiable {
        let id: SessionID
        let imageID: String
        let displayName: String
        let phase: Phase
    }

    @MainActor struct Gate {
        let id: UUID
        private let coordinator: DevelopRecoveryCoordinator

        fileprivate init(id: UUID, coordinator: DevelopRecoveryCoordinator) {
            self.id = id
            self.coordinator = coordinator
        }

        func result() async -> BarrierOutcome { await coordinator.evaluate(id) }
        func finish() { coordinator.finishGate(id) }
    }

    @MainActor private final class Record {
        let id: SessionID
        let key: Key?
        let source: SourceIdentity?
        let owner: EngineLibrary?
        let controller: DevelopController
        let displayName: String
        var phase: Phase = .active
        var lastFailure: Outcome?
        var attemptID: UUID?
        var task: Task<Outcome, Never>?

        init(id: SessionID, owner: EngineLibrary?, controller: DevelopController, displayName: String) {
            self.id = id
            self.owner = owner
            self.controller = controller
            self.key = owner.map { Key(owner: $0, imageID: controller.imageID) }
            self.source = owner.flatMap {
                DevelopRecoveryCoordinator.sourceIdentity(owner: $0, imageID: controller.imageID)
            }
            self.displayName = displayName
        }
    }

    private struct GateRecord {
        let owner: EngineLibrary
        let imageIDs: Set<String>
        let sources: [SourceIdentity]
        let initiate: Bool
        let order: UInt64
    }

    private struct OpenTicket {
        let owner: EngineLibrary
        let key: Key
        let source: SourceIdentity?
        let token: UUID
        let task: Task<Void, Never>
        var produced: DevelopController?
        var recipient: SessionID?
    }

    private struct ClosedSession {
        let id: SessionID
        let key: Key?
        let controller: ObjectIdentifier
    }

    private(set) var presentations: [Presentation] = []
    @ObservationIgnored private var records: [SessionID: Record] = [:]
    @ObservationIgnored private var gates: [UUID: GateRecord] = [:]
    @ObservationIgnored private var opens: [UUID: OpenTicket] = [:]
    @ObservationIgnored private var closedSessions: [ClosedSession] = []
    @ObservationIgnored private var generation: UInt64 = 0
    @ObservationIgnored private var nextGateOrder: UInt64 = 0

    var hasUnresolvedSessions: Bool { !records.isEmpty || !opens.isEmpty }
    var hasActiveReservations: Bool { !gates.isEmpty }

    private static func sourceIdentity(owner: EngineLibrary, imageID: String) -> SourceIdentity? {
        guard let index = owner.itemOfImage[imageID], owner.items.indices.contains(index),
              let url = owner.items[index].url else { return nil }
        let canonical = url.standardizedFileURL.resolvingSymlinksInPath()
        let attributes = try? FileManager.default.attributesOfItem(atPath: canonical.path)
        let fileID: SourceIdentity.FileID?
        if let device = attributes?[.systemNumber] as? NSNumber,
           let inode = attributes?[.systemFileNumber] as? NSNumber {
            fileID = .init(device: device.uint64Value, inode: inode.uint64Value)
        } else { fileID = nil }
        return SourceIdentity(path: canonical, fileID: fileID)
    }

    private static func overlaps(_ source: SourceIdentity?, _ sources: [SourceIdentity]) -> Bool {
        guard let source else { return false }
        return sources.contains { source.overlaps($0) }
    }

    private static func overlaps(_ lhs: SourceIdentity?, _ rhs: SourceIdentity?) -> Bool {
        guard let lhs, let rhs else { return false }
        return lhs.overlaps(rhs)
    }

    func register(owner: EngineLibrary?, controller: DevelopController, displayName: String) -> SessionID {
        let id = SessionID(value: UUID())
        records[id] = Record(id: id, owner: owner, controller: controller, displayName: displayName)
        changed()
        return id
    }

    func matchingSession(owner: EngineLibrary, imageID: String) -> SessionID? {
        let key = Key(owner: owner, imageID: imageID)
        return records.values.first { $0.key == key }?.id
    }

    func canOpen(owner: EngineLibrary, imageID: String) -> Bool {
        let key = Key(owner: owner, imageID: imageID)
        let source = Self.sourceIdentity(owner: owner, imageID: imageID)
        guard !records.values.contains(where: {
            $0.key == key || $0.key == nil || Self.overlaps(source, $0.source)
        }) else { return false }
        guard !opens.values.contains(where: {
            $0.key == key || Self.overlaps(source, $0.source)
        }) else { return false }
        return !gates.values.contains {
            ($0.owner === owner && $0.imageIDs.contains(imageID)) || Self.overlaps(source, $0.sources)
        }
    }

    /// Only checks an in-app scoped gate. It is not a global writer permission.
    func isUnreservedForHostMutation(owner: EngineLibrary, imageID: String) -> Bool {
        let source = Self.sourceIdentity(owner: owner, imageID: imageID)
        return !gates.values.contains {
            ($0.owner === owner && $0.imageIDs.contains(imageID)) || Self.overlaps(source, $0.sources)
        }
    }

    func beginOpen(owner: EngineLibrary, imageID: String, token: UUID, task: Task<Void, Never>) {
        opens[token] = OpenTicket(owner: owner, key: Key(owner: owner, imageID: imageID),
                                  source: Self.sourceIdentity(owner: owner, imageID: imageID),
                                  token: token, task: task, produced: nil)
        changed()
    }

    @discardableResult
    func producedOpen(_ controller: DevelopController, owner: EngineLibrary, token: UUID) -> Bool {
        guard opens[token] != nil else {
            // A late backend result is still a live native session. Retain it and
            // attempt cleanup; any failure remains visible by session ID.
            let id = register(owner: owner, controller: controller, displayName: controller.imageID)
            _ = requestClose(id)
            return false
        }
        opens[token]?.produced = controller
        changed()
        return true
    }

    @discardableResult
    func transferOpen(token: UUID, to sessionID: SessionID) -> Bool {
        guard let ticket = opens[token], let produced = ticket.produced,
              let recipient = records[sessionID], recipient.controller === produced,
              recipient.key == ticket.key else { return false }
        opens[token]?.recipient = sessionID
        changed()
        return true
    }

    func finishOpen(token: UUID) {
        guard let ticket = opens.removeValue(forKey: token) else { return }
        if let controller = ticket.produced {
            let handedOff = ticket.recipient.map { id in
                (records[id]?.controller === controller && records[id]?.key == ticket.key)
                    || closedSessions.contains {
                        $0.id == id && $0.key == ticket.key && $0.controller == ObjectIdentifier(controller)
                    }
            } ?? false
            if !handedOff {
                let id = register(owner: ticket.owner, controller: controller, displayName: controller.imageID)
                _ = requestClose(id)
            }
        }
        changed()
    }

    func cancelOpen(owner: EngineLibrary, imageID: String) {
        let key = Key(owner: owner, imageID: imageID)
        let source = Self.sourceIdentity(owner: owner, imageID: imageID)
        for ticket in opens.values where ticket.key == key || Self.overlaps(source, ticket.source) {
            ticket.task.cancel()
        }
    }

    func requestClose(_ id: SessionID) -> Task<Outcome, Never> {
        guard let record = records[id] else {
            if closedSessions.contains(where: { $0.id == id }) { return Task { .saved } }
            return Task { .failed(sessionID: id, message: "Develop session is no longer available") }
        }
        if let task = record.task { return task }
        if let failure = record.lastFailure { return Task { failure } }
        let attemptID = UUID()
        record.attemptID = attemptID
        record.phase = .saving
        let task = Task { @MainActor [weak self, record] in
            let result = await record.controller.close()
            let outcome: Outcome
            switch result {
            case .success:
                outcome = .saved
            case .failure(let error):
                outcome = .failed(sessionID: id, message: error.localizedDescription)
            }
            self?.finishClose(id, attemptID: attemptID, outcome: outcome)
            return outcome
        }
        record.task = task
        changed()
        return task
    }

    func retryClose(_ id: SessionID) -> Task<Outcome, Never> {
        if let record = records[id], record.task == nil, record.lastFailure != nil {
            record.lastFailure = nil
            record.phase = .active
            changed()
        }
        return requestClose(id)
    }

    private func finishClose(_ id: SessionID, attemptID: UUID, outcome: Outcome) {
        guard let record = records[id], record.attemptID == attemptID else { return }
        record.task = nil
        switch outcome {
        case .saved:
            records.removeValue(forKey: id)
            closedSessions.append(ClosedSession(id: id, key: record.key,
                                                controller: ObjectIdentifier(record.controller)))
            if closedSessions.count > 128 { closedSessions.removeFirst(closedSessions.count - 128) }
        case .failed(_, let message):
            record.phase = .failed(message)
            record.lastFailure = outcome
        }
        changed()
    }

    func reserveObserve(owner: EngineLibrary, imageIDs: Set<String>) -> Gate {
        reserve(owner: owner, imageIDs: imageIDs, initiate: false)
    }

    func reserveInitiate(owner: EngineLibrary, imageIDs: Set<String>) -> Gate {
        reserve(owner: owner, imageIDs: imageIDs, initiate: true)
    }

    private func reserve(owner: EngineLibrary, imageIDs: Set<String>, initiate: Bool) -> Gate {
        let id = UUID()
        nextGateOrder &+= 1
        gates[id] = GateRecord(owner: owner, imageIDs: imageIDs,
                               sources: imageIDs.compactMap { Self.sourceIdentity(owner: owner, imageID: $0) },
                               initiate: initiate, order: nextGateOrder)
        changed()
        return Gate(id: id, coordinator: self)
    }

    private func evaluate(_ id: UUID) async -> BarrierOutcome {
        guard let gate = gates[id] else { return .blocked([]) }
        let ownerID = ObjectIdentifier(gate.owner)
        func matches(_ key: Key?, source: SourceIdentity?) -> Bool {
            key.map { $0.owner == ownerID && gate.imageIDs.contains($0.imageID) } == true
                || Self.overlaps(source, gate.sources)
        }
        // A later gate may be queued while another consumer still owns the
        // saved pixels. It must fail closed before starting a close attempt.
        let precedingConflict = gates.contains { entry in
            let (otherID, other) = entry
            guard otherID != id, other.order < gate.order else { return false }
            if other.owner === gate.owner && !other.imageIDs.isDisjoint(with: gate.imageIDs) { return true }
            return other.sources.contains { source in gate.sources.contains { source.overlaps($0) } }
        }
        if precedingConflict { return .blocked([]) }
        if gate.initiate {
            for imageID in gate.imageIDs { cancelOpen(owner: gate.owner, imageID: imageID) }
        }
        // A cancelled open may publish a failed cleanup during this wait. Re-read
        // the registry after draining rather than trusting one captured task list.
        while true {
            if gate.initiate {
                for record in records.values where matches(record.key, source: record.source) {
                    if case .active = record.phase { _ = requestClose(record.id) }
                }
            }
            let seen = generation
            let pendingOpens = opens.values.filter {
                matches($0.key, source: $0.source)
            }.map(\.task)
            let pendingCloses = records.values.compactMap { record -> Task<Outcome, Never>? in
                guard matches(record.key, source: record.source) else { return nil }
                return record.task
            }
            for task in pendingOpens { await task.value }
            for task in pendingCloses { _ = await task.value }
            guard gates[id] != nil else { return .blocked([]) }
            if seen != generation { continue }
            let blocked = records.values.filter { record in
                record.key == nil || matches(record.key, source: record.source)
            }.map(\.id)
            return blocked.isEmpty ? .saved : .blocked(blocked)
        }
    }

    private func finishGate(_ id: UUID) {
        guard gates.removeValue(forKey: id) != nil else { return }
        changed()
    }

    private func changed() {
        generation &+= 1
        presentations = records.values.map {
            Presentation(id: $0.id, imageID: $0.controller.imageID,
                         displayName: $0.displayName, phase: $0.phase)
        }
    }
}
