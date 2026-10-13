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

    /// Visibly soft, relative to the shoot (`CullTriage.BlurThresholds`): sharpness in the shoot's
    /// bottom slice and below the absolute floor, or strong motion blur on a frame that is also
    /// among the softer ones. ml-quality's global sharpness drops for shallow depth of field and
    /// its motion score rises on fine texture, so neither signal is read against a fixed cutoff.
    public func isBlurry(_ t: CullTriage.BlurThresholds) -> Bool {
        guard let s = sharpness else { return false }
        if s <= t.softBelow { return true }
        if let m = motionBlur, m >= CullTriage.strongMotion, s <= t.motionSoftBelow { return true }
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

    /// Blur is judged against the shoot (SPEC §4 "blurry"): the sharpness cut is the lower of the
    /// shoot's 15th percentile and `softFloor`, so a uniformly sharp shoot flags nothing; strong
    /// motion counts only on a frame in the shoot's softer 30 % and under `motionSoftFloor`.
    public struct BlurThresholds: Sendable, Equatable {
        public var softBelow: Double
        public var motionSoftBelow: Double
        public init(softBelow: Double, motionSoftBelow: Double) {
            self.softBelow = softBelow
            self.motionSoftBelow = motionSoftBelow
        }
        /// Nothing is blurry until the shoot is measured.
        public static let none = BlurThresholds(softBelow: -1, motionSoftBelow: -1)

        /// From the measured sharpness values of the shoot.
        public static func from(sharpness values: [Double]) -> BlurThresholds {
            guard !values.isEmpty else { return .none }
            let sorted = values.sorted()
            func percentile(_ p: Double) -> Double { sorted[min(sorted.count - 1, Int((Double(sorted.count) * p).rounded(.down)))] }
            return BlurThresholds(softBelow: min(percentile(0.15), softFloor),
                                  motionSoftBelow: min(percentile(0.30), motionSoftFloor))
        }
    }
    /// Below this global sharpness a frame reads soft on any shoot (the Sony shoot's visibly soft
    /// frames measured 0.03–0.08; sharp faces with blurred backgrounds measured 0.10–0.13).
    public static let softFloor = 0.08
    /// Motion blur at or above this is strong (the scale tops out near 0.7).
    public static let strongMotion = 0.45
    public static let motionSoftFloor = 0.25
    /// Near-duplicates share a preview dHash within this many bits (the grouper's rule).
    public static let nearDuplicateBits = 6
    /// A single frame with a quality score at or above this is a likely keeper.
    public static let keeperQuality = 0.6
    public var blur = BlurThresholds.none

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
    /// `signals` the measured frames (missing: not analysed yet), `hashes` the preview dHashes
    /// the grouper compared (a burst's other frame is a near-duplicate of the pick only when
    /// its hash is within `nearDuplicateBits`; a frame that merely shares the capture window
    /// has nothing to flag).
    public static func compute(groups: [Range<Int>], best: [Int], signals: [Int: FrameSignals],
                               hashes: [Int: UInt64] = [:]) -> CullTriage {
        var t = CullTriage()
        t.total = groups.reduce(0) { $0 + $1.count }
        t.measured = signals.values.filter(\.isMeasured).count
        t.blur = BlurThresholds.from(sharpness: signals.values.compactMap(\.sharpness))
        for (g, range) in groups.enumerated() {
            let pick = best.indices.contains(g) ? best[g] : range.lowerBound
            if range.count > 1 {
                t.bursts.append(range)
                t.burstItems.append(contentsOf: range)
            }
            for id in range {
                guard let s = signals[id], s.isMeasured else { continue }
                if s.isBlurry(t.blur) {
                    t.rejects[id] = .blurry
                } else if s.eyesClosed {
                    t.rejects[id] = .eyesClosed
                } else if range.count > 1 {
                    if id == pick {
                        t.keepers.insert(id)
                    } else if let a = hashes[id], let b = hashes[pick], (a ^ b).nonzeroBitCount <= nearDuplicateBits {
                        t.rejects[id] = .nearDuplicate
                    } else {
                        t.rest.insert(id)
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
        t.blur = blur
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
