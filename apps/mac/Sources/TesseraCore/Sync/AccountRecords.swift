import Foundation

// Account-resident state (docs/design/cloud-sync/SPEC.md §3, WP CS-2): the records Tessera keeps
// in the iCloud Drive container. Every record carries `modified` (ISO 8601) and `machine` (the
// writing Mac's id); last writer wins per record, except people records, whose cluster ids are
// unioned so two Macs naming the same person converge. People are keyed by person id, drives by
// volume UUID, so a record means the same thing on every Mac the drive visits.

/// Where a record lives under `Tessera/` in the container: one file per singleton kind, a folder
/// of `<id>.json` for the keyed kinds.
public enum RecordKind: String, CaseIterable, Sendable {
    case preferences
    case keywords
    case styleProfile = "style-profile"
    case learning
    case person = "people"
    case drive = "drives"

    public var isKeyed: Bool { self == .person || self == .drive }
}

/// One record's address: a kind and, for keyed kinds, the id (person id, volume UUID).
public struct RecordKey: Hashable, Sendable {
    public var kind: RecordKind
    public var id: String?
    public init(_ kind: RecordKind, id: String? = nil) { self.kind = kind; self.id = id }
}

public protocol SyncRecord: Codable, Equatable, Sendable {
    static var kind: RecordKind { get }
    /// When this record was last written, by `machine`.
    var modified: Date { get set }
    var machine: String { get set }
    /// Two copies of the same record from two Macs: the one to keep. Last writer wins unless a
    /// kind says otherwise (people union their cluster ids, shoots union by library).
    static func merge(_ a: Self, _ b: Self) -> Self
}

extension SyncRecord {
    public static func merge(_ a: Self, _ b: Self) -> Self { Self.newer(a, b) }

    /// Last writer wins; a tie (same second on two Macs) goes to the higher machine id, so both
    /// Macs pick the same one.
    public static func newer(_ a: Self, _ b: Self) -> Self {
        if a.modified != b.modified { return a.modified > b.modified ? a : b }
        return a.machine >= b.machine ? a : b
    }

    /// Equal apart from the stamp: no reason to write (and bump the stamp) again.
    public func sameContent(as other: Self) -> Bool {
        var copy = self
        copy.modified = other.modified
        copy.machine = other.machine
        return copy == other
    }
}

/// `preferences.json`: Settings ▸ AI (never API keys; those stay in the Keychain), Settings ▸
/// Appearance and the culling preferences that already live in `AIPreferences`.
public struct PreferencesRecord: SyncRecord {
    public static let kind = RecordKind.preferences
    public var ai: AIPreferences
    /// Accent as "RRGGBB"; nil until Settings ▸ Appearance has been touched on some Mac.
    public var accent: String?
    /// `AppearanceSettings.Ground` raw value.
    public var ground: String?
    public var modified: Date
    public var machine: String

    public init(ai: AIPreferences, accent: String?, ground: String?, modified: Date, machine: String) {
        self.ai = ai; self.accent = accent; self.ground = ground; self.modified = modified; self.machine = machine
    }
}

/// `keywords.json`: the keyword hierarchy (names and parents; counts are per folder and stay local).
public struct KeywordsRecord: SyncRecord {
    public static let kind = RecordKind.keywords
    public struct Keyword: Codable, Equatable, Hashable, Sendable {
        public var name: String
        public var parent: String?
        public init(name: String, parent: String?) { self.name = name; self.parent = parent }
    }
    public var keywords: [Keyword]
    public var modified: Date
    public var machine: String

    public init(keywords: [Keyword], modified: Date, machine: String) {
        self.keywords = keywords.sorted { ($0.parent ?? "", $0.name) < ($1.parent ?? "", $1.name) }
        self.modified = modified; self.machine = machine
    }

    /// Parents before children, so another Mac can create them in order.
    public var inCreationOrder: [Keyword] {
        let byName = Dictionary(keywords.map { ($0.name, $0) }, uniquingKeysWith: { a, _ in a })
        func depth(_ k: Keyword, _ seen: Set<String> = []) -> Int {
            guard let p = k.parent, !seen.contains(p), let parent = byName[p] else { return 0 }
            return 1 + depth(parent, seen.union([k.name]))
        }
        return keywords.sorted { (depth($0), $0.name) < (depth($1), $1.name) }
    }
}

/// The questionnaire (`StyleQuestionnaire` in the engine), kept free of FFI types on disk.
public struct QuestionnaireRecord: Codable, Equatable, Sendable {
    public var brightness: Double, contrast: Double, warmth: Double, saturation: Double, skinTonePriority: Double
    public init(brightness: Double = 0, contrast: Double = 0, warmth: Double = 0, saturation: Double = 0, skinTonePriority: Double = 0) {
        self.brightness = brightness; self.contrast = contrast; self.warmth = warmth
        self.saturation = saturation; self.skinTonePriority = skinTonePriority
    }
}

/// `style-profile.json`: the photographer's global profile. The app keeps one profile per shoot
/// (the engine's `style-profile/<library>.json` in app support); each is merged in by library id
/// on save, and the global questionnaire is the last one written anywhere.
public struct StyleProfileRecord: SyncRecord {
    public static let kind = RecordKind.styleProfile
    public struct Shoot: Codable, Equatable, Sendable {
        public var libraryID: String
        public var samples: Int
        public var questionnaire: QuestionnaireRecord
        public var modified: Date
        public var machine: String
        /// The engine's stored profile file (JSON), so another Mac restores the fitted model
        /// before it opens the shoot. Nil when the engine has not saved one.
        public var model: Data?
        public init(libraryID: String, samples: Int, questionnaire: QuestionnaireRecord, modified: Date, machine: String, model: Data?) {
            self.libraryID = libraryID; self.samples = samples; self.questionnaire = questionnaire
            self.modified = modified; self.machine = machine; self.model = model
        }
        var isNewer: (Shoot) -> Bool { { other in modified != other.modified ? modified > other.modified : machine >= other.machine } }
    }
    public var questionnaire: QuestionnaireRecord
    /// Samples over every shoot.
    public var samples: Int
    public var shoots: [String: Shoot]
    public var modified: Date
    public var machine: String

    public init(questionnaire: QuestionnaireRecord, shoots: [String: Shoot], modified: Date, machine: String) {
        self.questionnaire = questionnaire; self.shoots = shoots; self.modified = modified; self.machine = machine
        samples = shoots.values.reduce(0) { $0 + $1.samples }
    }

    /// Shoots union by library id (the newer copy of each); the record's questionnaire and stamp
    /// follow the newer record.
    public static func merge(_ a: Self, _ b: Self) -> Self {
        let lead = newer(a, b)
        var shoots = a.shoots
        for (id, shoot) in b.shoots {
            if let mine = shoots[id], mine.isNewer(shoot) { continue }
            shoots[id] = shoot
        }
        return Self(questionnaire: lead.questionnaire, shoots: shoots, modified: lead.modified, machine: lead.machine)
    }
}

/// `learning.json`: the culling learner, one entry per shoot (the engine's
/// `cull-learning/<library>.json`), merged by library id like the style profile.
public struct LearningRecord: SyncRecord {
    public static let kind = RecordKind.learning
    public struct Shoot: Codable, Equatable, Sendable {
        public var libraryID: String
        /// Confirmed keep / reject labels the learner has seen.
        public var labels: Int
        public var modified: Date
        public var machine: String
        public var model: Data?
        public init(libraryID: String, labels: Int, modified: Date, machine: String, model: Data?) {
            self.libraryID = libraryID; self.labels = labels; self.modified = modified; self.machine = machine; self.model = model
        }
        var isNewer: (Shoot) -> Bool { { other in modified != other.modified ? modified > other.modified : machine >= other.machine } }
    }
    public var labels: Int
    public var shoots: [String: Shoot]
    public var modified: Date
    public var machine: String

    public init(shoots: [String: Shoot], modified: Date, machine: String) {
        self.shoots = shoots; self.modified = modified; self.machine = machine
        labels = shoots.values.reduce(0) { $0 + $1.labels }
    }

    public static func merge(_ a: Self, _ b: Self) -> Self {
        let lead = newer(a, b)
        var shoots = a.shoots
        for (id, shoot) in b.shoots {
            if let mine = shoots[id], mine.isNewer(shoot) { continue }
            shoots[id] = shoot
        }
        return Self(shoots: shoots, modified: lead.modified, machine: lead.machine)
    }
}

/// `people/<id>.json`: a person's name and the cluster ids merged into them. Face embeddings and
/// members stay on the drive; this is what lets a second Mac name the same clusters.
public struct PersonRecord: SyncRecord {
    public static let kind = RecordKind.person
    public var id: String
    /// nil: the name was cleared.
    public var name: String?
    /// Person ids merged into this one (union-find: every id here is this person), sorted.
    public var clusters: [String]
    public var modified: Date
    public var machine: String

    public init(id: String, name: String?, clusters: some Sequence<String>, modified: Date, machine: String) {
        self.id = id; self.name = name; self.modified = modified; self.machine = machine
        self.clusters = Set(clusters).subtracting([id]).sorted()
    }

    /// Every id this record stands for.
    public var allIDs: Set<String> { Set(clusters).union([id]) }

    /// The name follows the last writer; cluster ids are unioned.
    public static func merge(_ a: Self, _ b: Self) -> Self {
        let lead = newer(a, b)
        return Self(id: lead.id, name: lead.name, clusters: Set(a.clusters).union(b.clusters).union([a.id, b.id]),
                    modified: lead.modified, machine: lead.machine)
    }
}

/// `drives/<volume-uuid>.json`: a drive's opened roots, display name, counts and where it was last
/// seen, so the folder popover on another Mac lists it before it is attached.
public struct DriveRecord: SyncRecord {
    public static let kind = RecordKind.drive
    public struct Root: Codable, Equatable, Sendable {
        /// Relative to the volume (no leading slash; "" is the volume itself).
        public var path: String
        /// Where the volume was mounted when this root was opened (a hint, not an identity).
        public var lastMountPath: String
        public var photos: Int
        public var lastSeen: Date
        public init(path: String, lastMountPath: String, photos: Int, lastSeen: Date) {
            self.path = path; self.lastMountPath = lastMountPath; self.photos = photos; self.lastSeen = lastSeen
        }
    }
    public var volumeUUID: String
    public var name: String
    public var roots: [Root]
    /// Photos over every root.
    public var photos: Int
    public var lastSeenMachine: String
    /// "Rut's MacBook Pro": the Mac's name at the time, for the popover.
    public var lastSeenMachineName: String?
    public var lastSeen: Date
    public var modified: Date
    public var machine: String

    public init(volumeUUID: String, name: String, roots: [Root], lastSeenMachine: String, lastSeenMachineName: String?,
                lastSeen: Date, modified: Date, machine: String) {
        self.volumeUUID = volumeUUID; self.name = name
        self.roots = roots.sorted { $0.path < $1.path }
        photos = roots.reduce(0) { $0 + $1.photos }
        self.lastSeenMachine = lastSeenMachine; self.lastSeenMachineName = lastSeenMachineName
        self.lastSeen = lastSeen; self.modified = modified; self.machine = machine
    }

    /// Roots union by path (the newer copy of each); name and last-seen follow the newer record.
    public static func merge(_ a: Self, _ b: Self) -> Self {
        let lead = newer(a, b)
        var roots = Dictionary(a.roots.map { ($0.path, $0) }, uniquingKeysWith: { x, _ in x })
        for root in b.roots {
            if let mine = roots[root.path], mine.lastSeen > root.lastSeen { continue }
            roots[root.path] = root
        }
        return Self(volumeUUID: lead.volumeUUID, name: lead.name, roots: Array(roots.values),
                    lastSeenMachine: lead.lastSeenMachine, lastSeenMachineName: lead.lastSeenMachineName,
                    lastSeen: lead.lastSeen, modified: lead.modified, machine: lead.machine)
    }
}
