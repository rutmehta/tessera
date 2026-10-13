import AppKit
import TesseraCore

/// How Cull shows the shoot (redesign SPEC §4, WP UX-14): one burst at a time with the agent's
/// pick large, or the grid grouped by burst. G toggles; the choice is remembered.
enum CullLayout: String, CaseIterable, Identifiable {
    case burst, grid
    var id: String { rawValue }
    var title: String { self == .burst ? "Burst" : "Grid" }
}

/// What the Cull view is narrowed to: a triage row, or one reason from a quick action.
enum CullFilter: Equatable {
    case bucket(CullTriage.Bucket)
    case reason(CullTriage.RejectReason)

    func items(in triage: CullTriage) -> Set<Int> {
        switch self {
        case .bucket(let b): triage.items(b)
        case .reason(let r): Set(triage.items(r))
        }
    }

    var title: String {
        switch self {
        case .bucket(let b): b.title
        case .reason(let r): r == .eyesClosed ? "Closed eyes" : r.title.capitalizedSentence
        }
    }
}

extension AppModel {
    static let cullLayoutKey = "Cull.layout"

    /// Cull's own views (the triage column, the burst view, the grouped grid) are shown over the
    /// Library grid: an engine or stub folder, not People, Deliver, the loupe or Compare.
    var isCullWorkspace: Bool {
        isLibraryWorkspace && viewMode == .grid && source != .people && !deliverMode && !library.items.isEmpty
    }

    /// The burst view is on screen (its keys clamp to the burst, decisions advance within it).
    var isBurstViewShown: Bool { isCullWorkspace && cullLayout == .burst }
    /// The grouped grid is on screen (rows per burst, singles at the bottom).
    var isGroupedGridShown: Bool { isCullWorkspace && cullLayout == .grid }

    /// G in Cull.
    func toggleCullLayout() {
        cullLayout = cullLayout == .burst ? .grid : .burst
    }

    func cullLayoutDidChange() {
        UserDefaults.standard.set(cullLayout.rawValue, forKey: Self.cullLayoutKey)
        liveObservers.forEach { $0.cullLayoutDidChange() }
        // The grouped grid and the burst view both scroll to the focused frame.
        notifySelection(scroll: true)
    }

    /// "review" on a grid row, or a click on the big photo's strip: the burst view on that frame.
    func openBurst(id: Int) {
        if cullLayout != .burst { cullLayout = .burst }
        select(id: id)
    }

    // MARK: Bursts

    /// The burst (group of 2+ frames) the focused frame belongs to, and its pick.
    var focusedBurst: (group: Int, frames: Range<Int>, pick: Int)? {
        guard let item = focusedItem, cull.groups.indices.contains(item.groupID) else { return nil }
        let range = cull.groups[item.groupID]
        let pick = cull.bestOfGroup.indices.contains(item.groupID) ? cull.bestOfGroup[item.groupID] : range.lowerBound
        return (item.groupID, range, pick)
    }

    /// 1-based index of the focused group among the bursts, for "Burst n of N" (nil: a single).
    var focusedBurstOrdinal: Int? {
        guard let item = focusedItem, cull.groups[item.groupID].count > 1 else { return nil }
        return cull.groups[..<item.groupID].filter { $0.count > 1 }.count + 1
    }

    /// Bursts whose every frame is decided (the column's progress bar).
    var burstsDecided: Int {
        cull.groups.filter { $0.count > 1 && $0.allSatisfy { cull[$0].decision != .undecided } }.count
    }

    func isPick(_ id: Int) -> Bool { cull.isSuggestedBest(id) }

    /// The position of the frame to land on when entering group `g`: its pick in the burst
    /// view, else its first visible frame.
    private func landingPosition(group g: Int) -> Int? {
        guard cull.groups.indices.contains(g) else { return nil }
        let frames = cull.groups[g].filter { positionOfID[$0] >= 0 }
        guard let first = frames.first else { return nil }
        if isBurstViewShown, cull.bestOfGroup.indices.contains(g), positionOfID[cull.bestOfGroup[g]] >= 0 {
            return positionOfID[cull.bestOfGroup[g]]
        }
        return positionOfID[first]
    }

    /// ⌥→ / ⌥← and the Next / Previous burst buttons: the next (previous) group in view,
    /// landing on its pick. Returns false at the ends.
    @discardableResult
    func moveBurst(_ delta: Int) -> Bool {
        guard let f = focus else { return false }
        var g = item(at: f).groupID + (delta > 0 ? 1 : -1)
        while cull.groups.indices.contains(g) {
            if let p = landingPosition(group: g) { select(position: p); return true }
            g += delta > 0 ? 1 : -1
        }
        statusMessage = delta > 0 ? "That was the last burst" : "First burst"
        return false
    }

    /// After a decision in the burst view (auto-advance on, as everywhere): the burst's next
    /// undecided frame, else the next burst. Called with the position the decision was made at.
    func advanceInBurst(from f: Int) {
        guard isBurstViewShown, autoAdvance, visible.indices.contains(f) else { return }
        let item = self.item(at: f)
        let range = cull.groups[item.groupID]
        if range.count > 1 {
            let next = range.first { cull[$0].decision == .undecided && positionOfID[$0] >= 0 && positionOfID[$0] != f }
            if let next { select(position: positionOfID[next]); return }
            moveBurst(1)
        } else if f + 1 < visible.count {
            select(position: f + 1)
        }
    }

    /// ← → in the burst view: one frame within the burst; the ends say so.
    func stepInBurst(_ delta: Int) {
        guard let f = focus else { return }
        let g = item(at: f).groupID
        let p = f + delta
        guard visible.indices.contains(p), item(at: p).groupID == g else {
            statusMessage = delta > 0 ? "Last frame in the burst · ⌥→ next burst" : "First frame in the burst · ⌥← previous burst"
            return
        }
        select(position: p)
    }

    // MARK: Triage filters and quick actions (SPEC §4)

    /// A triage row: on, or off again when it is the current filter.
    func setCullFilter(_ filter: CullFilter?) {
        let new = filter == cullFilter ? nil : filter
        guard new != cullFilter else { return }
        refreshVisible { cullFilter = new }
        if let new {
            let n = visibleCount
            statusMessage = "Showing \(new.title.lowercased()): \(n.formatted()) frame\(n == 1 ? "" : "s")"
        }
    }

    /// The triage was recomputed (the pass progressed): a filtered view follows it.
    func triageDidChange() {
        guard cullFilter != nil else { return }
        refreshVisible(keepingExisting: true)
    }

    /// Keep the pick in every burst, reject its other undecided frames: one undo step.
    func keepPickInEveryBurst() {
        var decisions: [(Int, Decision)] = []
        var bursts = 0
        for (g, range) in cull.groups.enumerated() where range.count > 1 {
            let pick = cull.bestOfGroup.indices.contains(g) ? cull.bestOfGroup[g] : range.lowerBound
            var touched = false
            if cull[pick].decision != .keep { decisions.append((pick, .keep)); touched = true }
            for id in range where id != pick && cull[id].decision == .undecided {
                decisions.append((id, .reject))
                touched = true
            }
            if touched { bursts += 1 }
        }
        guard !decisions.isEmpty else { statusMessage = "Every burst is decided"; return }
        guard runCull({ try self.cull.decide(decisions) }) else { return }
        showToast("Kept the pick in \(bursts) burst\(bursts == 1 ? "" : "s") · one undo step", undoable: true)
    }

    /// Reject every undecided frame the triage calls blurry: one undo step.
    func rejectBlurry() {
        let ids = assist.triage.items(.blurry).filter { cull[$0].decision == .undecided }
        guard !ids.isEmpty else {
            statusMessage = assist.isRunning ? "Still measuring; try again when the pass finishes" : "No blurry frames to reject"
            return
        }
        guard runCull({ try self.cull.apply(.reject, to: ids) }) else { return }
        showToast("Rejected \(ids.count) frame\(ids.count == 1 ? "" : "s") with missed focus or motion blur", undoable: true)
        if let first = ids.first { select(id: first) }
    }

    /// Show the frames whose eyes proxy reads closed.
    func findClosedEyes() {
        let ids = assist.triage.items(.eyesClosed)
        guard !ids.isEmpty else {
            statusMessage = assist.signals.values.contains { $0.eyesOpen != nil }
                ? "No closed eyes found" : "No faces measured yet: Cull ▸ Analyze Faces finds them"
            return
        }
        if cullFilter != .reason(.eyesClosed) { refreshVisible { cullFilter = .reason(.eyesClosed) } }
        showToast("\(ids.count) frame\(ids.count == 1 ? "" : "s") with closed eyes", undoable: false)
        if let first = ids.first { select(id: first) }
    }

    /// The first named person of the shoot (People), for the "Show me <name>" chip.
    var firstNamedPerson: (summary: PersonSummary, name: String)? {
        for person in assist.people {
            if let tile = people.person(person.id), tile.isNamed { return (person, tile.displayName) }
            if person.named { return (person, person.name) }
        }
        return nil
    }

    /// Display names of the shoot's named people (agent phrase matching).
    var namedPeople: [String] {
        assist.people.compactMap { person in
            if let tile = people.person(person.id) { return tile.isNamed ? tile.displayName : nil }
            return person.named ? person.name : nil
        }
    }

    func showPerson(named name: String) {
        guard let person = assist.people.first(where: { p in
            (people.person(p.id)?.displayName ?? p.name).caseInsensitiveCompare(name) == .orderedSame
        }) else { statusMessage = "No one called \(name) in this shoot"; return }
        assist.filter(person: person, eyesClosed: false)
        showToast("Showing the \(person.items.count) frame\(person.items.count == 1 ? "" : "s") with \(name)", undoable: false)
    }

    /// The Cull agent field: simple phrases become quick actions; anything else is reported.
    /// Returns false when nothing matched.
    @discardableResult
    func cullAgent(_ text: String) -> Bool {
        guard let phrase = CullAgentPhrase.parse(text, people: namedPeople) else { return false }
        switch phrase {
        case .rejectBlurry: rejectBlurry()
        case .keepPicks: keepPickInEveryBurst()
        case .findClosedEyes: findClosedEyes()
        case .showPerson(let name): showPerson(named: name)
        }
        return true
    }
}
