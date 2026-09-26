import Foundation
import TesseraFFI

/// A frame's keep prediction (docs/06 §3 "Learning"), in item ids.
public struct AssistPrediction: Sendable, Equatable {
    public var itemID: Int
    public var pKeep: Double
    /// Pre-filled decision awaiting confirmation (automated mode only).
    public var suggested: Decision?
    public var likelyReject: Bool
    /// Up to five signed log-odds terms, largest first ("sharpness −1.8").
    public var explanation: [(feature: String, contribution: Double)]

    public static func == (a: AssistPrediction, b: AssistPrediction) -> Bool {
        a.itemID == b.itemID && a.pKeep == b.pKeep && a.suggested == b.suggested && a.likelyReject == b.likelyReject
            && a.explanation.map(\.feature) == b.explanation.map(\.feature)
    }

    /// "Sharpness pulls toward reject" style one-liner for the inspector.
    public var explanationText: String {
        explanation.prefix(3).map { term in
            let name = term.feature.replacingOccurrences(of: "_", with: " ")
            return String(format: "%@ %+.1f", name, term.contribution)
        }.joined(separator: " · ")
    }
}

/// A person in the open queue (descriptor clusters; session-local identities).
public struct PersonSummary: Sendable, Equatable, Identifiable {
    public var id: String
    public var name: String
    public var items: [Int]
    public var faces: Int
    public var coverItem: Int?
    public var coverOrdinal: UInt32
    /// The engine has a name for this person (`name` is otherwise a "Person <id>" placeholder).
    public var named: Bool = false
    /// Confirmed faces, same scope as `faces`.
    public var confirmedCount: Int = 0
    /// The indexed medoid member, when it is in this library.
    public var medoid: PersonFaceRef? = nil
}

public struct FaceChip: Sendable, Equatable, Identifiable {
    public var ordinal: UInt32
    /// Normalized rectangle in the displayed preview (origin top-left).
    public var rect: CGRect
    public var focus: Double
    public var eyesOpen: Double?
    public var personID: String?
    public var id: UInt32 { ordinal }

    public enum Level: Sendable { case good, fair, poor, unknown }
    /// Green ≥ 0.55, yellow ≥ 0.30, red below (docs/06 §3 "Face strip").
    public var focusLevel: Level { focus >= 0.55 ? .good : focus >= 0.30 ? .fair : .poor }
    /// Open ≥ 0.5, closed < 0.3 (a weak geometric proxy, shown as such), unsure between.
    public var eyesLevel: Level {
        guard let e = eyesOpen else { return .unknown }
        return e >= 0.5 ? .good : e >= 0.3 ? .fair : .poor
    }
}

extension CullController {
    private func engine() throws -> EngineLibrary {
        guard case .engine(let lib) = backend else { throw CullError.unavailable("Assisted culling needs a folder opened on the engine") }
        return lib
    }

    /// Switches assisted culling; the learner is per library and persists.
    @discardableResult
    public func setAssistMode(_ mode: AssistMode) throws -> AssistStatus {
        try engine().session.setAssistMode(mode: mode)
    }

    public func assistStatus() throws -> AssistStatus { try engine().session.assistStatus() }

    /// Predictions for every frame, in review order (likely keepers first, likely rejects last).
    public func review() throws -> [AssistPrediction] {
        let lib = try engine()
        return try lib.session.review().compactMap { p in
            guard let item = lib.itemOfImage[p.imageId] else { return nil }
            let suggested: Decision? = switch p.suggested {
            case .keep: .keep
            case .reject: .reject
            case .undecided, nil: nil
            }
            return AssistPrediction(itemID: item, pKeep: p.pKeep, suggested: suggested, likelyReject: p.likelyReject,
                                  explanation: p.explanation.map { ($0.feature, $0.contribution) })
        }
    }

    /// Confirms pre-filled decisions as one undo step (and teaches the learner).
    @discardableResult
    public func confirmSuggestions(_ ids: [Int]) throws -> CullChange {
        let lib = try engine()
        guard !ids.isEmpty else { return CullChange(ids: [], current: nil, albumsChanged: false) }
        return absorb(try lib.session.confirmSuggestions(imageIds: ids.map { lib.imageIDs[$0] }), library: lib)
    }

    /// Rejects (dismisses) suggestions without deciding anything.
    public func dismissSuggestions(_ ids: [Int]) throws {
        let lib = try engine()
        try lib.session.dismissSuggestions(imageIds: ids.map { lib.imageIDs[$0] })
    }

    public func faceStrip(_ id: Int) throws -> [FaceChip] {
        let lib = try engine()
        return try lib.session.faceStrip(imageId: lib.imageIDs[id]).map {
            FaceChip(ordinal: $0.ordinal, rect: CGRect(x: $0.x, y: $0.y, width: $0.width, height: $0.height),
                     focus: $0.focus, eyesOpen: $0.eyesOpen, personID: $0.personId)
        }
    }

    public func people(refresh: Bool = false) throws -> [PersonSummary] {
        guard case .engine(let lib) = backend else { return [] }
        return try lib.session.people(refresh: refresh).map {
            PersonSummary(id: $0.id, name: $0.name, items: $0.images.compactMap { lib.itemOfImage[$0] },
                          faces: Int($0.faces), coverItem: lib.itemOfImage[$0.coverImage], coverOrdinal: $0.coverOrdinal,
                          named: $0.named, confirmedCount: Int($0.confirmedCount),
                          medoid: $0.medoidFace.flatMap { f in
                              lib.itemOfImage[f.imageId].map { PersonFaceRef(item: $0, ordinal: f.ordinal) }
                          })
        }
    }

    /// Per-person filter: frames with this person; with `eyesClosedBelow`, only where their
    /// eyes-open proxy is below it.
    public func items(withPerson person: String, eyesClosedBelow: Double? = nil) throws -> [Int] {
        let lib = try engine()
        return try lib.session.framesWithPerson(personId: person, eyesClosedBelow: eyesClosedBelow)
            .compactMap { lib.itemOfImage[$0] }
    }
}

extension EngineLibrary {
    /// Real culling signals for `itemIDs` (blocking; call off the main actor). `progress` gets
    /// (done, total, name) before each image; returning false stops early. Failures are
    /// collected per file rather than stopping the pass.
    public func analyze(_ itemIDs: [Int], faces: Bool, force: Bool = false,
                        progress: (Int, Int, String) -> Bool = { _, _, _ in true }) -> (analyzed: Int, errors: [String]) {
        analyze(images: analysisTargets(itemIDs), faces: faces, force: force, progress: progress)
    }

    /// (image id, file name) of `itemIDs`, resolved now: the layout can change in place
    /// while a background pass runs, so resolve on the main actor before handing off.
    public func analysisTargets(_ itemIDs: [Int]) -> [(imageID: String, name: String)] {
        itemIDs.filter(items.indices.contains).map { (imageIDs[$0], items[$0].name) }
    }

    /// `analyze` over targets resolved with `analysisTargets` (safe off the main actor).
    public func analyze(images: [(imageID: String, name: String)], faces: Bool, force: Bool = false,
                        progress: (Int, Int, String) -> Bool = { _, _, _ in true }) -> (analyzed: Int, errors: [String]) {
        var analyzed = 0
        var errors: [String] = []
        let options = AnalysisOptions(quality: true, faces: faces, force: force)
        for (n, image) in images.enumerated() {
            guard progress(n, images.count, image.name) else { break }
            do {
                if !(try engine.analyzeImage(imageId: image.imageID, options: options)).skipped { analyzed += 1 }
            } catch {
                errors.append("\(image.name): \(error.localizedDescription)")
                // Without the face models nothing else will succeed: stop at the first such error.
                if faces, error.localizedDescription.contains("face models") { break }
            }
        }
        _ = progress(images.count, images.count, "")
        return (analyzed, errors)
    }

    /// Hidden test aid (`--seed-faces`): deterministic synthetic faces so the face strip, people and
    /// per-person filters can be exercised on the generated sample folder, which has no faces.
    /// Two people: A in every frame, B in frames of odd groups. Frame n (display order, 0-based) has
    /// A's eyes closed when n % 6 == 5 and A out of focus when n % 5 == 2 (the frames
    /// `make-sample-folder.swift --defects` blurs).
    public func seedSyntheticFaces() throws {
        func descriptor(_ axis: Int, _ n: Int) -> [Float] {
            var v = [Float](repeating: 0.01, count: 128)
            v[axis] = 1
            v[(axis + 7) % 128] = 0.05 * Float(n % 3)
            return v
        }
        for (n, imageID) in imageIDs.enumerated() {
            var faces = [FaceInput(x: 180, y: 120, width: 150, height: 190, focus: n % 5 == 2 ? 0.18 : 0.82,
                                   eyesOpen: n % 6 == 5 ? 0.12 : 0.86, embedding: descriptor(3, n))]
            if items[n].groupID % 2 == 1 {
                faces.append(FaceInput(x: 620, y: 160, width: 130, height: 170, focus: 0.45, eyesOpen: 0.4,
                                       embedding: descriptor(40, n)))
            }
            // Faces are measured on the ≤ 1024 px analysis preview (the samples are 1200 × 800).
            try engine.setFaces(imageId: imageID, faces: faces, width: 1024, height: 683)
        }
    }
}
