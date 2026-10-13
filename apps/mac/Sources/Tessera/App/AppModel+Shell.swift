import AppKit
import SwiftUI
import TesseraCore

/// The three modes of the shell (redesign SPEC §1, WP UX-11): one segmented control, top centre.
/// Cull is the Library (grid, loupe, compare, people); Edit is Photo Edit; Deliver covers the canvas
/// with the export entry until UX-15 makes it a workspace. Review stays reachable from the View and
/// Develop menus and is shown as Cull in the control.
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
            if isPhotoEditing || isReviewing { returnToLibrary() }
            deliverMode = true
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

    // MARK: Agent field

    /// ⌘K: the agent field of the current mode takes the keyboard.
    func focusAgentField() {
        guard viewMode != .document else { return }
        agentFieldFocusRequest += 1
    }

    /// Return in the agent field. In Edit the text is the instruction of an agent run on the focused
    /// photo (the planner's free-text redo path, one named group in its history, reviewed in the
    /// queue as every run is). In Cull simple phrases (blurry, the pick, eyes, a person's name) are
    /// the quick actions (UX-14); other text, and Deliver's until UX-15, is reported.
    func submitAgentInstruction(_ raw: String) {
        let text = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        if shellMode == .cull, isLibraryWorkspace, cullAgent(text) { return }
        guard shellMode == .edit else { statusMessage = "Agent: \(text)"; return }
        guard let item = editTarget, item.engineImage != nil, isEngineBacked else {
            statusMessage = "The agent edits photos opened from a folder on the engine"
            return
        }
        guard !agent.isRunning else { statusMessage = "An agent edit is already running"; return }
        agent.start(itemIDs: [item.id], instruction: text)
        if agent.isRunning {
            statusMessage = "Agent: “\(text)” on \(item.name)"
        } else {
            // A missing key, a read-only library or a busy review action: the Auto Edit sheet
            // names the blocker and offers Settings ▸ AI.
            statusMessage = agent.error ?? "Agent: could not start"
            agent.present()
        }
    }
}
