import AppKit
import SwiftUI
import TesseraCore

/// The three modes of the shell (redesign SPEC §1, WP UX-11): one segmented control, top centre.
/// Cull is the Library (grid, loupe, compare, people); Edit is Photo Edit; Deliver is the export
/// workspace (UX-15, AppModel+Deliver.swift). Review stays reachable from the View and Develop
/// menus and is shown as Cull in the control.
enum ShellMode: String, CaseIterable, Identifiable {
    case cull, edit, deliver
    var id: String { rawValue }

    var title: String {
        switch self {
        case .cull: "Cull"
        case .edit: "Edit"
        case .deliver: "Deliver"
        }
    }

    var symbol: String {
        switch self {
        case .cull: "square.grid.2x2"
        case .edit: "slider.horizontal.3"
        case .deliver: "square.and.arrow.up"
        }
    }

    /// The agent field's placeholder (SPEC §1).
    var agentPlaceholder: String {
        switch self {
        case .cull: "Reject the blurry ones in this burst"
        case .edit: "Say what you want, or point at it"
        case .deliver: "Export the keeps as web JPEGs"
        }
    }

    /// Accessibility namespace of the mode's controls (`library.*` / `develop.*`, the two the
    /// identifier audit accepts).
    var axNamespace: String { self == .edit ? "develop" : "library" }
}

extension AppModel {
    private static let librarySidebarKey = "Shell.librarySidebar"
    private static func panelKey(_ mode: ShellMode) -> String { "Shell.panel." + mode.rawValue }

    var shellMode: ShellMode { isPhotoEditing ? .edit : deliverMode ? .deliver : .cull }

    /// The segmented control and ⌘1 / ⌘2 / ⌘3. Document mode keeps its own Library button.
    func selectShellMode(_ mode: ShellMode) {
        guard viewMode != .document else { return }
        switch mode {
        case .cull:
            deliverMode = false
            if isPhotoEditing || isReviewing { returnToLibrary() }
        case .edit:
            deliverMode = false
            guard !isPhotoEditing else { return }
            guard canEnterPhotoEdit else { statusMessage = "Choose a photo to edit"; return }
            enterPhotoEdit()
        case .deliver:
            // UX-15 (item 18): with a photo open, Deliver follows the save-and-close; it never
            // draws while the Edit chrome is still up.
            if isPhotoEditing || isReviewing { returnToLibraryThenDeliver() } else { enterDeliver() }
        }
    }

    // MARK: Panel (Tab)

    /// Tab, the toolbar button and View ▸ Hide / Show Inspector: animated, remembered per mode
    /// (Cull, Edit, Deliver) in user defaults. Document mode's Tab hides every panel (B5-02) and
    /// is routed separately.
    func toggleInspectorPanel() {
        withAnimation(Theme.Motion.appear) { showInspector.toggle() }
        guard viewMode != .document, !isReviewing else { return }
        UserDefaults.standard.set(showInspector, forKey: Self.panelKey(shellMode))
    }

    /// The remembered panel state of `mode`; shown until the person hides it (SPEC §1).
    func restorePanelState(for mode: ShellMode) {
        guard viewMode != .document, !isReviewing else { return }
        let defaults = UserDefaults.standard
        let shown = defaults.object(forKey: Self.panelKey(mode)) == nil ? true : defaults.bool(forKey: Self.panelKey(mode))
        if showInspector != shown { withAnimation(Theme.Motion.appear) { showInspector = shown } }
    }

    // MARK: Library sidebar

    /// The old source sidebar (folders, albums, people, culling lists) is hidden by default; the
    /// folder button in the toolbar shows the same content as a popover. View ▸ Show Library
    /// Sidebar brings the column back, and the choice persists.
    var librarySidebarShown: Bool { documents.columnVisibility != .detailOnly }

    func toggleLibrarySidebar() {
        let show = !librarySidebarShown
        documents.columnVisibility = show ? .all : .detailOnly
        UserDefaults.standard.set(show, forKey: Self.librarySidebarKey)
    }

    /// Launch: the sidebar and panel states from user defaults (AppDelegate, before the window).
    func applyShellDefaults() {
        documents.columnVisibility = UserDefaults.standard.bool(forKey: Self.librarySidebarKey) ? .all : .detailOnly
        restorePanelState(for: shellMode)
    }

    // MARK: Search and thumbnail size (UX-16)

    /// ⌘F: the search bar drops under the toolbar in Cull and takes the keyboard; ⌘F again (or
    /// its close button) hides it. The filter itself stays until Library ▸ Clear Filter.
    func toggleSearch() {
        guard viewMode != .document else { return }
        if showSearch, searchFieldFocusRequest > 0 {
            showSearch = false
        } else {
            showSearch = true
            searchFieldFocusRequest += 1
        }
    }

    static let thumbnailSizeRange: ClosedRange<Double> = 110...360

    /// View ▸ Thumbnail Size ▸ Larger / Smaller: one step of a quarter.
    func stepThumbnailSize(_ direction: Int) {
        let factor = direction > 0 ? 1.25 : 0.8
        setThumbnailSize(thumbnailSize * factor)
    }

    /// A pinch on the grid, or the menu: clamped to the grid's range.
    func setThumbnailSize(_ size: Double) {
        let clamped = min(max(size, Self.thumbnailSizeRange.lowerBound), Self.thumbnailSizeRange.upperBound)
        if clamped != thumbnailSize { thumbnailSize = clamped }
    }

    // MARK: Agent field

    /// ⌘K: the agent field of the current mode takes the keyboard.
    func focusAgentField() {
        guard viewMode != .document else { return }
        agentFieldFocusRequest += 1
    }

    /// Return in the agent field. In Edit the text is the instruction of an agent run on the focused
    /// photo (the planner's free-text redo path, one named group in its history, reviewed in the
    /// queue as every run is). In Cull simple phrases (blurry, the pick, eyes, a person's name) are
    /// the quick actions (UX-14); in Deliver a How and a What ("export the keeps for Instagram")
    /// pick the rows and run the export (UX-15); other text is reported.
    func submitAgentInstruction(_ raw: String) {
        let text = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        if shellMode == .cull, isLibraryWorkspace, cullAgent(text) { return }
        if shellMode == .deliver, deliverAgent(text) { return }
        guard shellMode == .edit else { statusMessage = "Agent: \(text)"; return }
        guard let item = editTarget, item.engineImage != nil, isEngineBacked else {
            statusMessage = "The agent edits photos opened from a folder on the engine"
            return
        }
        guard !agent.isRunning else { statusMessage = "An agent edit is already running"; return }
        agent.start(itemIDs: [item.id], instruction: text)
        if agent.isRunning {
            statusMessage = "Agent: “\(text)” on \(item.name)"
            if let ref = item.engineImage {
                agentFieldRun = AgentFieldRun(itemID: item.id, imageID: ref.imageID, instruction: text)
            }
        } else {
            // A missing key, a read-only library or a busy review action: the Auto Edit sheet
            // names the blocker and offers Settings ▸ AI.
            statusMessage = agent.error ?? "Agent: could not start"
            agent.present()
        }
    }
}

// MARK: - Agent change cards (UX-12)

/// A run the agent field started on one photo (SPEC §1 "agent field"): its answer is the plan as
/// change cards in the Edit panel, not the review queue. The queue's entry for the photo (one
/// named history group) is the plan; the cards are its steps.
struct AgentFieldRun: Equatable {
    let itemID: Int
    let imageID: String
    let instruction: String
}

extension AppModel {
    /// The finished field run's review entry for the photo being edited, while it still waits for
    /// Keep / Undo all. Nil while the run is going, for another photo, or once reviewed.
    var agentFieldEntry: AgentReviewEntry? {
        guard let run = agentFieldRun, !agent.isRunning, editTarget?.id == run.itemID,
              let entry = agent.queue.entry(run.imageID), entry.status == .needsReview,
              entry.isActionable else { return nil }
        return entry
    }

    /// The field run's review target (the queue's owner and the photo), for Keep / Undo all.
    var agentFieldTarget: AgentController.ReviewTarget? {
        guard let entry = agentFieldEntry else { return nil }
        return agent.queueTarget(entry)
    }

    /// Keep: the group stays in the Edits stack as one grouped step (it is already in the
    /// history); the queue entry is accepted (the style profile learns from it) and the cards go.
    func keepAgentChanges() {
        guard let target = agentFieldTarget else { return }
        agent.accept(target)
        agentFieldRun = nil
    }

    /// Undo all: reverts the group (one step in the photo's history) and clears the cards.
    func undoAgentChanges() {
        guard let target = agentFieldTarget else { return }
        agent.revert(target)
        agentFieldRun = nil
    }

    /// The other frames of the edited photo's burst, which "Apply to burst" runs the same
    /// instruction on (a multi-photo run: those land in the review queue).
    var agentFieldBurstFrames: [Int] {
        guard let item = editTarget, library.groups.indices.contains(item.groupID) else { return [] }
        return library.groups[item.groupID].filter { $0 != item.id }
    }

    func applyAgentChangesToBurst() {
        guard let run = agentFieldRun, agentFieldEntry != nil else { return }
        let frames = agentFieldBurstFrames
        guard !frames.isEmpty, !agent.isRunning else { return }
        agent.start(itemIDs: frames, instruction: run.instruction)
        statusMessage = agent.isRunning
            ? "Applying “\(run.instruction)” to \(frames.count) more frame\(frames.count == 1 ? "" : "s") of this burst (review in Agent Review)"
            : agent.error ?? "Agent: could not start"
    }

    /// "review" in the agent status row: shows the panel on the Develop tab, where the cards are.
    func revealAgentChanges() {
        guard viewMode != .document else { return }
        photoInspectorTab = .develop
        if !showInspector { toggleInspectorPanel() }
    }
}
