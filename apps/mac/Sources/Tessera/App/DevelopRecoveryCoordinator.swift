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

    struct Gate {
        let id: UUID
        private let coordinator: DevelopRecoveryCoordinator

        fileprivate init(id: UUID, coordinator: DevelopRecoveryCoordinator) {
            self.id = id
            self.coordinator = coordinator
        }

        func result() async -> BarrierOutcome { await coordinator.evaluate(id) }
        func finish() { coordinator.finishGate(id) }
    }

    private final class Record {
        let id: SessionID
        let key: Key?
        let owner: EngineLibrary?
        let controller: DevelopController
        let displayName: String
        var phase: Phase = .active
        var attemptID: UUID?
        var task: Task<Outcome, Never>?

        init(id: SessionID, owner: EngineLibrary?, controller: DevelopController, displayName: String) {
            self.id = id
            self.owner = owner
            self.controller = controller
            self.key = owner.map { Key(owner: $0, imageID: controller.imageID) }
            self.displayName = displayName
        }
    }

    private struct GateRecord {
        let owner: EngineLibrary
        let imageIDs: Set<String>
        let initiate: Bool
    }

    private struct OpenTicket {
        let owner: EngineLibrary
        let key: Key
        let token: UUID
        let task: Task<Void, Never>
        var produced: DevelopController?
        var transferred = false
    }

    private(set) var presentations: [Presentation] = []
    @ObservationIgnored private var records: [SessionID: Record] = [:]
    @ObservationIgnored private var gates: [UUID: GateRecord] = [:]
    @ObservationIgnored private var opens: [UUID: OpenTicket] = [:]
    @ObservationIgnored private var closedSessions: [SessionID] = []
    @ObservationIgnored private var generation: UInt64 = 0

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
        guard !records.values.contains(where: { $0.key == key || $0.key == nil }) else { return false }
        guard !opens.values.contains(where: { $0.key == key }) else { return false }
        return !gates.values.contains {
            $0.owner === owner && $0.imageIDs.contains(imageID)
        }
    }

    func canMutate(owner: EngineLibrary, imageID: String) -> Bool {
        !gates.values.contains { $0.owner === owner && $0.imageIDs.contains(imageID) }
    }

    func beginOpen(owner: EngineLibrary, imageID: String, token: UUID, task: Task<Void, Never>) {
        opens[token] = OpenTicket(owner: owner, key: Key(owner: owner, imageID: imageID),
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

    func transferOpen(token: UUID) {
        opens[token]?.transferred = true
        changed()
    }

    func finishOpen(token: UUID) {
        guard let ticket = opens.removeValue(forKey: token) else { return }
        if let controller = ticket.produced, !ticket.transferred {
            let id = register(owner: ticket.owner, controller: controller, displayName: controller.imageID)
            _ = requestClose(id)
        }
        changed()
    }

    func cancelOpen(owner: EngineLibrary, imageID: String) {
        let key = Key(owner: owner, imageID: imageID)
        for ticket in opens.values where ticket.key == key { ticket.task.cancel() }
    }

    func requestClose(_ id: SessionID) -> Task<Outcome, Never> {
        guard let record = records[id] else {
            if closedSessions.contains(id) { return Task { .saved } }
            return Task { .failed(sessionID: id, message: "Develop session is no longer available") }
        }
        if let task = record.task { return task }
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

    private func finishClose(_ id: SessionID, attemptID: UUID, outcome: Outcome) {
        guard let record = records[id], record.attemptID == attemptID else { return }
        record.task = nil
        switch outcome {
        case .saved:
            records.removeValue(forKey: id)
            closedSessions.append(id)
            if closedSessions.count > 128 { closedSessions.removeFirst(closedSessions.count - 128) }
        case .failed(_, let message):
            record.phase = .failed(message)
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
        gates[id] = GateRecord(owner: owner, imageIDs: imageIDs, initiate: initiate)
        changed()
        return Gate(id: id, coordinator: self)
    }

    private func evaluate(_ id: UUID) async -> BarrierOutcome {
        guard let gate = gates[id] else { return .blocked([]) }
        let ownerID = ObjectIdentifier(gate.owner)
        if gate.initiate {
            for imageID in gate.imageIDs { cancelOpen(owner: gate.owner, imageID: imageID) }
        }
        // A cancelled open may publish a failed cleanup during this wait. Re-read
        // the registry after draining rather than trusting one captured task list.
        while true {
            if gate.initiate {
                for record in records.values where record.key.map({ $0.owner == ownerID && gate.imageIDs.contains($0.imageID) }) ?? false {
                    if case .active = record.phase { _ = requestClose(record.id) }
                }
            }
            let seen = generation
            let pendingOpens = opens.values.filter {
                $0.key.owner == ownerID && gate.imageIDs.contains($0.key.imageID)
            }.map(\.task)
            let pendingCloses = records.values.compactMap { record -> Task<Outcome, Never>? in
                guard record.key.map({ $0.owner == ownerID && gate.imageIDs.contains($0.imageID) }) ?? false else { return nil }
                return record.task
            }
            for task in pendingOpens { await task.value }
            for task in pendingCloses { _ = await task.value }
            guard gates[id] != nil else { return .blocked([]) }
            if seen != generation { continue }
            let blocked = records.values.filter { record in
                record.key == nil || record.key.map { $0.owner == ownerID && gate.imageIDs.contains($0.imageID) } == true
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
