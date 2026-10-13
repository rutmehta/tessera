import Foundation

/// The real signals of one frame (`Engine.analyzeImage`, ml-quality and the face models), read
/// back from the index for the burst view's HUD and the triage. Missing signals are unknown,
/// never defective.
public struct FrameSignals: Sendable, Equatable {
    /// Global sharpness, 0…1.
    public var sharpness: Double?
    /// Motion blur, 0…1 (1 is the most blurred).
    public var motionBlur: Double?
    public var noise: Double?
    /// The weak eyes-open proxy of the lowest face in the frame, 0…1.
    public var eyesOpen: Double?
    /// Sharpness of the least sharp face, 0…1.
    public var faceSharpness: Double?
    /// ml-quality's overall score, 0…1.
    public var quality: Double?

    public init(sharpness: Double? = nil, motionBlur: Double? = nil, noise: Double? = nil,
                eyesOpen: Double? = nil, faceSharpness: Double? = nil, quality: Double? = nil) {
        self.sharpness = sharpness
        self.motionBlur = motionBlur
        self.noise = noise
        self.eyesOpen = eyesOpen
        self.faceSharpness = faceSharpness
        self.quality = quality
    }

    /// Any quality signal has been measured.
    public var isMeasured: Bool { sharpness != nil || quality != nil }

    /// Missed focus or motion blur (the defect sweep's thresholds).
    public var isBlurry: Bool {
        if let s = sharpness, s < DefectRule.focus.threshold { return true }
        if let m = motionBlur, m > CullTriage.motionBlurAbove { return true }
        return false
    }

    /// The eyes proxy reads closed (below the defect sweep's threshold).
    public var eyesClosed: Bool {
        guard let e = eyesOpen else { return false }
        return e < DefectRule.closedEyes.threshold
    }

    /// The engine's best-of-group value: quality weighted by face sharpness when both exist.
    public var technicalScore: Double? {
        switch (quality, faceSharpness) {
        case (let q?, let f?): q * (0.5 + 0.5 * f)
        case (let q?, nil): q
        case (nil, let f?): f
        case (nil, nil): nil
        }
    }
}

/// The agent's triage of a shoot (redesign SPEC §4): computed from the burst groups, the engine's
/// best of each group and the frames' signals, as the analysis pass fills them in. Pure, so the
/// buckets are unit tested without an engine. Buckets overlap (a burst's pick is a likely keeper,
/// its other frames near-duplicates); decisions do not move frames between buckets, so the counts
/// stay put while the person works.
public struct CullTriage: Sendable, Equatable {
    public enum Bucket: String, Sendable, CaseIterable, Identifiable {
        case keepers, bursts, rejects, rest
        public var id: String { rawValue }
        public var title: String {
            switch self {
            case .keepers: "Likely keepers"
            case .bursts: "Bursts to decide"
            case .rejects: "Suggested rejects"
            case .rest: "Nothing to flag"
            }
        }
    }

    public enum RejectReason: String, Sendable, CaseIterable {
        case blurry, eyesClosed, nearDuplicate
        public var title: String {
            switch self {
            case .blurry: "blurry"
            case .eyesClosed: "eyes closed"
            case .nearDuplicate: "near-duplicates"
            }
        }
    }

    /// Motion blur above this counts as blurry (the sweep has no motion rule; ml-quality's scale
    /// puts hand-held softness around 0.3–0.5).
    public static let motionBlurAbove = 0.7
    /// A single frame with a quality score at or above this is a likely keeper.
    public static let keeperQuality = 0.6

    /// Likely keepers: the pick of every burst without a defect, and single frames that score well.
    public var keepers: Set<Int> = []
    /// Item ids of every burst (groups of 2+ frames), in display order.
    public var burstItems: [Int] = []
    /// Suggested rejects with the reason the agent would give.
    public var rejects: [Int: RejectReason] = [:]
    /// Everything measured that is neither a keeper nor a suggested reject.
    public var rest: Set<Int> = []
    /// Frames measured so far (the triage fills in as the pass runs).
    public var measured = 0
    public var total = 0

    public init() {}

    public var burstCount: Int { bursts.count }
    /// Groups of 2+ frames, in display order.
    public var bursts: [Range<Int>] = []

    public func count(_ reason: RejectReason) -> Int { rejects.values.filter { $0 == reason }.count }

    public func count(_ bucket: Bucket) -> Int {
        switch bucket {
        case .keepers: keepers.count
        case .bursts: bursts.count
        case .rejects: rejects.count
        case .rest: rest.count
        }
    }

    /// Item ids shown by a bucket's filter.
    public func items(_ bucket: Bucket) -> Set<Int> {
        switch bucket {
        case .keepers: keepers
        case .bursts: Set(burstItems)
        case .rejects: Set(rejects.keys)
        case .rest: rest
        }
    }

    public func items(_ reason: RejectReason) -> [Int] {
        rejects.filter { $0.value == reason }.map(\.key).sorted()
    }

    /// `groups` are the library's display groups, `best` the engine's pick per group,
    /// `signals` the measured frames (missing: not analysed yet).
    public static func compute(groups: [Range<Int>], best: [Int], signals: [Int: FrameSignals]) -> CullTriage {
        var t = CullTriage()
        t.total = groups.reduce(0) { $0 + $1.count }
        t.measured = signals.values.filter(\.isMeasured).count
        for (g, range) in groups.enumerated() {
            let pick = best.indices.contains(g) ? best[g] : range.lowerBound
            if range.count > 1 {
                t.bursts.append(range)
                t.burstItems.append(contentsOf: range)
            }
            for id in range {
                guard let s = signals[id], s.isMeasured else { continue }
                if s.isBlurry {
                    t.rejects[id] = .blurry
                } else if s.eyesClosed {
                    t.rejects[id] = .eyesClosed
                } else if range.count > 1 {
                    if id == pick {
                        t.keepers.insert(id)
                    } else {
                        t.rejects[id] = .nearDuplicate
                    }
                } else if (s.quality ?? 0) >= keeperQuality {
                    t.keepers.insert(id)
                } else {
                    t.rest.insert(id)
                }
            }
        }
        return t
    }

    /// Why the engine's pick wins its burst, from the signals ("sharpest of 5, eyes open").
    /// Nil while the burst is unmeasured (the engine then picks by file size).
    public static func pickReason(pick: Int, group: Range<Int>, signals: [Int: FrameSignals]) -> String? {
        guard let s = signals[pick], s.isMeasured else { return nil }
        var parts: [String] = []
        let others = group.filter { $0 != pick }.compactMap { signals[$0] }
        if let sharp = s.sharpness, others.allSatisfy({ ($0.sharpness ?? 0) <= sharp }) {
            parts.append("sharpest of \(group.count)")
        } else if let q = s.quality, others.allSatisfy({ ($0.quality ?? 0) <= q }) {
            parts.append("best score of \(group.count)")
        } else {
            parts.append("best face of \(group.count)")
        }
        if let e = s.eyesOpen { parts.append(e >= 0.5 ? "eyes open" : e >= DefectRule.closedEyes.threshold ? "eyes unsure" : "eyes closed") }
        return parts.joined(separator: ", ")
    }

    /// The library was renumbered in place: follow the frames, drop those that left. The next
    /// analysis refresh recomputes the buckets from the new groups.
    public func remapped(_ remap: (Int) -> Int?) -> CullTriage {
        var t = CullTriage()
        t.keepers = Set(keepers.compactMap(remap))
        t.burstItems = burstItems.compactMap(remap)
        t.rejects = Dictionary(rejects.compactMap { id, r in remap(id).map { ($0, r) } }, uniquingKeysWith: { a, _ in a })
        t.rest = Set(rest.compactMap(remap))
        t.measured = measured
        t.total = total
        t.bursts = bursts
        return t
    }
}

/// What the Cull agent field understood from a typed phrase (SPEC §4 quick actions).
public enum CullAgentPhrase: Equatable {
    case rejectBlurry
    case keepPicks
    case findClosedEyes
    case showPerson(String)

    /// `people` are the named people of the shoot. Matching is by word, case-insensitive.
    public static func parse(_ text: String, people: [String]) -> CullAgentPhrase? {
        let lower = text.lowercased()
        if lower.contains("blur") || lower.contains("soft") || lower.contains("focus") { return .rejectBlurry }
        if lower.contains("eye") || lower.contains("blink") { return .findClosedEyes }
        if lower.contains("best") || lower.contains("pick") || lower.contains("sharpest") { return .keepPicks }
        let words = Set(lower.split { !$0.isLetter && !$0.isNumber }.map(String.init))
        if let person = people.first(where: { name in
            let parts = name.lowercased().split { !$0.isLetter && !$0.isNumber }.map(String.init)
            return !parts.isEmpty && parts.allSatisfy(words.contains)
        }) {
            return .showPerson(person)
        }
        return nil
    }
}
