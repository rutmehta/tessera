import AppKit
import SwiftUI
import TesseraCore

/// Navigable, session-local review. All mutations go through the captured-owner controller API.
struct AgentReviewWorkspace: View {
    @Bindable var model: AppModel

    var body: some View {
        if model.agent.queue.isEmpty {
            EmptyStateContent(symbol: "checklist", title: "Nothing to review",
                              message: "Auto Edit results appear here. This review queue is available during this app session.") {
                Button("Back to Library") { model.returnToLibrary() }
                    .buttonStyle(.theme(.bordered, height: Theme.Height.regular))
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .accessibilityIdentifier("review-empty")
        } else {
            HStack(spacing: 0) {
                VStack(alignment: .leading, spacing: 0) {
                    Text(model.agent.queue.summary)
                        .font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                        .padding(Theme.Space.m).fixedSize(horizontal: false, vertical: true)
                    Hairline()
                    ScrollView {
                        LazyVStack(spacing: Theme.Space.xxs) {
                            ForEach(model.agent.queue.entries) { entry in
                                Button { model.selectReviewPhoto(entry.imageID) } label: {
                                    ReviewDestinationRow(entry: entry, selected: model.reviewNavigation.selectedID == entry.imageID,
                                                         busy: model.agent.busy.contains(entry.imageID))
                                }
                                .buttonStyle(.plain)
                                .id(entry.imageID)
                                .accessibilityIdentifier("review-photo-\(entry.imageID)")
                                .accessibilityAddTraits(model.reviewNavigation.selectedID == entry.imageID ? .isSelected : [])
                            }
                        }
                        .scrollTargetLayout()
                        .padding(Theme.Space.s)
                    }
                    .scrollPosition(id: $model.reviewNavigation.anchorID, anchor: .top)
                    .accessibilityIdentifier("agent-review-list")
                }
                .frame(width: 220)
                .background(Theme.panel)
                Hairline(vertical: true)
                ReviewCurrentPreview(model: model)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
    }
}

struct ReviewDestinationRow: View {
    let entry: AgentReviewEntry
    let selected: Bool
    let busy: Bool
    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            HStack(spacing: Theme.Space.xs) {
                Text(entry.name).font(Theme.Fonts.labelMedium).foregroundStyle(Theme.textPrimary)
                    .lineLimit(2).truncationMode(.middle)
                if busy {
                    ProgressView().controlSize(.small)
                        .accessibilityLabel("Updating photo")
                        .accessibilityIdentifier("review-row-busy")
                }
            }
            Text(entry.error == nil ? entry.status.title : "Failed")
                .font(Theme.Fonts.captionMedium).foregroundStyle(entry.error == nil ? Theme.textSecondary : Theme.reject)
            Text(entry.confidenceText).font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(Theme.Space.m)
        .background(selected ? Theme.accent.opacity(0.13) : Color.clear)
        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.chip))
        .overlay(RoundedRectangle(cornerRadius: Theme.Radius.chip).stroke(selected ? Theme.accent : Color.clear))
        .contentShape(Rectangle())
    }
}

private struct ReviewCurrentPreview: View {
    let model: AppModel
    @State private var image: CGImage?
    @State private var request: PreviewRequest?
    @State private var loadTask: Task<Void, Never>?
    @State private var loadToken = UUID()
    @State private var loading = false
    private var identity: String {
        "\(model.agent.reviewGeneration):\(model.reviewNavigation.selectedID ?? ""): \(model.libraryRevision):\(model.agentRevision)"
    }
    var body: some View {
        VStack(spacing: Theme.Space.m) {
            Text("Current preview").font(Theme.Fonts.captionMedium).foregroundStyle(Theme.textSecondary)
            if let reason = model.reviewUnavailableReason {
                EmptyStateContent(symbol: "photo.badge.exclamationmark", title: "Photo unavailable", message: reason) { EmptyView() }
            } else if let image {
                Image(decorative: image, scale: 1).resizable().aspectRatio(contentMode: .fit)
                    .accessibilityLabel(model.selectedReviewEntry?.name ?? "Current photo")
            } else {
                Text(loading ? "Loading current preview…" : "Preview unavailable")
                    .font(Theme.Fonts.label).foregroundStyle(Theme.textSecondary)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
            Text("Current saved photo adjustments")
                .font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary)
        }
        .padding(Theme.Space.gutter)
        .accessibilityIdentifier("review-current-preview")
        .onChange(of: identity, initial: true) { _, _ in load() }
        .onDisappear { request?.cancel(); request = nil; loadTask?.cancel(); loadTask = nil; loadToken = UUID() }
    }

    private func load() {
        request?.cancel(); request = nil; image = nil
        loadTask?.cancel(); loadTask = nil
        let token = UUID()
        loadToken = token
        guard let item = model.reviewTargetItem, let owner = model.engineLibrary,
              let selected = model.reviewNavigation.selectedID, model.selectedReviewEntry?.error == nil else {
            loading = false
            return
        }
        loading = true
        let generation = model.agent.reviewGeneration
        let barrier = model.pendingDevelopSaveBarrier(imageID: selected, library: owner)
        loadTask = Task {
            await barrier.value
            guard !Task.isCancelled, loadToken == token, model.engineLibrary === owner,
                  model.reviewNavigation.selectedID == selected, model.agent.reviewGeneration == generation else { return }
            // A just-closed session may have saved after this item's previous preview was cached.
            model.loader.invalidate(item)
            request = model.loader.request(item, tier: .preview, priority: .veryHigh) { next in
                guard loadToken == token, model.engineLibrary === owner,
                      model.reviewNavigation.selectedID == selected, model.agent.reviewGeneration == generation else { return }
                image = next
                loading = false
            }
            // ThumbnailLoader reports successful delivery only; a failed decode must not spin forever.
            try? await Task.sleep(for: .seconds(30))
            if !Task.isCancelled, loadToken == token { loading = false }
        }
    }
}

struct AgentReviewInspector: View {
    @Bindable var model: AppModel
    /// Presentation inputs also allow deterministic busy-state layout coverage without an engine operation.
    let busy: Bool
    let canEdit: Bool

    var body: some View {
        ScrollView {
            if let entry = model.selectedReviewEntry {
                VStack(alignment: .leading, spacing: 0) {
                    PanelSection("Review target") {
                        Text(entry.name).font(Theme.Fonts.labelMedium).foregroundStyle(Theme.textPrimary)
                            .fixedSize(horizontal: false, vertical: true)
                        Text("Actions apply to this photo only").font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                        Text("Your review: \(entry.status.title)").font(Theme.Fonts.label)
                            .accessibilityIdentifier("review-user-status")
                        Text("Critic: \(entry.error != nil ? "Failed" : entry.accepted ? "Passed" : "Needs attention") · \(entry.confidenceText)")
                            .font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                            .accessibilityIdentifier("review-critic-status")
                        if let reason = model.reviewUnavailableReason { Hint(reason) }
                        else if model.reviewTargetItem != nil && !canEdit {
                            Hint("This photo is being updated. Editing resumes when the operation finishes.")
                        }
                        if let error = entry.error { Text(error).font(Theme.Fonts.caption).foregroundStyle(Theme.reject) }
                    }
                    PanelSection("Actions") {
                        actions(entry)
                        Hint("Accept teaches your style profile. Edit photo to undo recipe changes.")
                    }
                    PanelSection("Rationale") {
                        Text(entry.summary).font(Theme.Fonts.label).foregroundStyle(Theme.textSecondary)
                            .fixedSize(horizontal: false, vertical: true)
                        ForEach(entry.steps) { step in
                            VStack(alignment: .leading, spacing: Theme.Space.xxs) {
                                Text(step.title + (step.enabled ? "" : " · Disabled")).font(Theme.Fonts.captionMedium)
                                Text(step.rationale).font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                            }
                            .fixedSize(horizontal: false, vertical: true)
                        }
                        if !entry.criticReasons.isEmpty {
                            Text(entry.criticReasons.joined(separator: "\n")).font(Theme.Fonts.caption)
                                .foregroundStyle(Theme.textSecondary).fixedSize(horizontal: false, vertical: true)
                        }
                    }
                }
            } else {
                PanelSection("Review") { Hint("Choose a review photo to see its result and actions.") }
            }
        }
        .background(Theme.panel)
        .accessibilityIdentifier("agent-review-inspector")
        .accessibilityValue(busy ? "Operation in progress; review actions unavailable" : canEdit ? "Review actions available" : "Photo unavailable for editing")
    }

    private func actions(_ entry: AgentReviewEntry) -> some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            Button("Edit photo") { model.editReviewedPhoto() }
                .disabled(!canEdit)
                .accessibilityIdentifier("review-edit-photo")
            HStack {
                Button("Accept") { model.acceptReviewedPhoto(advance: false) }
                Button("Accept & next") { model.acceptReviewedPhoto(advance: true) }
                    .accessibilityIdentifier("review-accept-next")
            }
            .disabled(busy || model.reviewTargetItem == nil || entry.error != nil || entry.groupID == nil || entry.status == .accepted)
            if model.reviewNavigation.isDrafting {
                TextField("Instruction for this photo", text: $model.reviewNavigation.instruction, axis: .vertical)
                    .textFieldStyle(.roundedBorder).lineLimit(2...5)
                    .accessibilityIdentifier("agent-review-instruction")
                HStack {
                    Button("Redo") { model.redoReviewedPhoto() }
                        .disabled(busy || model.reviewTargetItem == nil || model.reviewNavigation.instruction.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    Button("Cancel") { model.reviewNavigation.cancelRedo() }
                }
            } else {
                Button("Redo with instruction…") { model.reviewNavigation.beginRedo() }
                    .disabled(busy || model.reviewTargetItem == nil)
            }
            Button("Revert group") { model.revertReviewedPhoto() }
                .disabled(busy || model.reviewTargetItem == nil || entry.groupID == nil || entry.status == .reverted)
                .accessibilityIdentifier("review-revert")
        }
        .buttonStyle(.theme(.bordered, height: Theme.Height.regular))
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}
