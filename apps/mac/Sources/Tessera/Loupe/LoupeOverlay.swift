import SwiftUI

/// Loupe chrome: target context and display controls above, mode shortcuts below. The top strip
/// keeps the mask toolbar's existing offset clear, and the spacer remains click-through.
struct LoupeOverlay: View {
    let model: AppModel
    @State private var displayInfoPresented = false
    @State private var shortcutsPresented = false

    private var proof: SoftProof { SoftProof.shared }

    static func proofBadgeText(enabled: Bool, profileName: String?, status: String, gamutWarning: Bool) -> String? {
        guard enabled else { return nil }
        if let profileName {
            return "Proof · \(profileName)" + (gamutWarning ? " · gamut warning" : "")
        }
        return status.isEmpty ? "Proof unavailable" : status
    }

    private var proofSummary: String {
        Self.proofBadgeText(enabled: proof.enabled, profileName: proof.lut?.profileName,
                            status: proof.status, gamutWarning: proof.gamutWarning) ?? ""
    }

    private var proofDetails: String {
        let status = proof.status.isEmpty ? "No proof status available." : proof.status
        return status + (proof.gamutWarning ? " · Gamut warning is on." : "")
    }

    static func proofHeading(profileName: String?) -> String {
        profileName.map { "Soft proof · \($0)" } ?? "Soft proof"
    }

    var photoNameDisclosure: String? {
        guard !model.isPhotoEditing else { return nil }
        return model.focusedItem?.name
    }

    var shortcutText: String {
        if model.isReviewEditing {
            return "← → previous / next review photo\nD Develop · M masks\n⌘Z photo undo · Esc tool / Back to Review"
        }
        if model.isPhotoEditing {
            return "← → previous / next photo\nD Develop · M masks\n⌘Z photo undo · Esc tool / Back to Library"
        }
        return "← → group · ↑ ↓ frame in group\nX U P decide · 1 2 3 grade · K keep best\nC compare · Y N suggestions · ⌘Z undo\nD Edit photo · Esc grid"
    }

    var body: some View {
        VStack(spacing: 0) {
            HStack(alignment: .center, spacing: Theme.Space.s) {
                if let item = model.focusedItem, !model.isPhotoEditing {
                    Text(item.name)
                        .font(Theme.Fonts.labelMedium)
                        .foregroundStyle(Color(nsColor: Theme.Palette.OnImage.text))
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .accessibilityLabel("Photo: \(item.name)")
                        .padding(.horizontal, Theme.Space.s)
                        .background(Capsule().fill(Color(nsColor: Theme.Palette.OnImage.scrim)))
                        .allowsHitTesting(false)

                    if let badge = model.focusedState.badgeText {
                        Chip(text: badge, color: Color(nsColor: model.focusedState.decision.color), style: .outlined,
                             height: Theme.Height.chip)
                            .help(badge)
                            .padding(.horizontal, Theme.Space.xxs)
                            .background(Capsule().fill(Color(nsColor: Theme.Palette.OnImage.scrim)))
                    }
                    if model.focusedIsBest {
                        Chip(text: "Suggested best", color: Theme.keep, style: .outlined,
                             height: Theme.Height.chip)
                            .help("Suggested best · K keeps it, rejects the rest")
                            .padding(.horizontal, Theme.Space.xxs)
                            .background(Capsule().fill(Color(nsColor: Theme.Palette.OnImage.scrim)))
                    }
                }

                Spacer(minLength: Theme.Space.s)

                if proof.enabled {
                    Text(proofSummary)
                        .font(Theme.Fonts.captionMedium)
                        .foregroundStyle(Color(nsColor: Theme.Palette.OnImage.text))
                        .lineLimit(1)
                        .help(proofDetails)
                        .accessibilityLabel(proofDetails)
                        .accessibilityIdentifier("softproof-badge")
                        .padding(.horizontal, Theme.Space.s)
                        .background(Capsule().fill(Color(nsColor: Theme.Palette.OnImage.scrim)))
                        .allowsHitTesting(false)
                }

                Button {
                    displayInfoPresented = true
                } label: {
                    Label("Display info", systemImage: "info.circle")
                        .labelStyle(.titleAndIcon)
                }
                .buttonStyle(.plain)
                .font(Theme.Fonts.captionMedium)
                .foregroundStyle(Color(nsColor: Theme.Palette.OnImage.text))
                .padding(.horizontal, Theme.Space.s)
                .frame(height: Theme.Height.small)
                .background(Capsule().fill(Color(nsColor: Theme.Palette.OnImage.scrim)))
                .help("Display information and soft proof status")
                .accessibilityLabel("Display info")
                .accessibilityIdentifier("loupe-display-info")
                .popover(isPresented: $displayInfoPresented, arrowEdge: .top) {
                    displayInfoPopover
                        .onExitCommand { dismissPresentedDisclosure() }
                }

                if model.isPhotoEditing, model.developStatus == .ready, !MaskTools.shared.active {
                    Button { MaskTools.shared.setActive(true) } label: {
                        Label("Masks", systemImage: "circle.lefthalf.striped.horizontal")
                    }
                    .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                    .help("Local adjustments with masks (M)")
                }
            }
            .padding(.horizontal, Theme.Space.gutter)
            .frame(height: Theme.Height.sectionHeader)

            Spacer()
                .allowsHitTesting(false)

            Button {
                shortcutsPresented = true
            } label: {
                Label("Shortcuts", systemImage: "keyboard")
                    .font(Theme.Fonts.captionMedium)
                    .foregroundStyle(Color(nsColor: Theme.Palette.OnImage.text))
                    .padding(.horizontal, Theme.Space.s)
                    .frame(height: Theme.Height.small)
                    .background(Capsule().fill(Color(nsColor: Theme.Palette.OnImage.scrim)))
            }
            .buttonStyle(.plain)
            .help("Keyboard shortcuts for this workspace")
            .accessibilityLabel("Shortcuts")
            .accessibilityIdentifier("loupe-shortcuts")
            .popover(isPresented: $shortcutsPresented, arrowEdge: .bottom) {
                shortcutsPopover
                    .onExitCommand { dismissPresentedDisclosure() }
            }
            .padding(.bottom, Theme.Space.s)
        }
        .onChange(of: displayInfoPresented) { updateDisclosureKeyOwnership() }
        .onChange(of: shortcutsPresented) { updateDisclosureKeyOwnership() }
        .onDisappear {
            model.loupeDisclosurePresented = false
            model.dismissLoupeDisclosure = nil
        }
    }

    private func updateDisclosureKeyOwnership() {
        let presented = displayInfoPresented || shortcutsPresented
        model.loupeDisclosurePresented = presented
        if presented {
            model.dismissLoupeDisclosure = Self.makeDismissAction(
                displayInfo: $displayInfoPresented, shortcuts: $shortcutsPresented
            )
        } else {
            model.dismissLoupeDisclosure = nil
        }
    }

    @MainActor
    static func makeDismissAction(displayInfo: Binding<Bool>, shortcuts: Binding<Bool>) -> @MainActor () -> Void {
        {
            displayInfo.wrappedValue = false
            shortcuts.wrappedValue = false
        }
    }

    func dismissPresentedDisclosure() {
        displayInfoPresented = false
        shortcutsPresented = false
        model.loupeDisclosurePresented = false
        model.dismissLoupeDisclosure = nil
    }

    private var displayInfoPopover: some View {
        VStack(alignment: .leading, spacing: Theme.Space.m) {
            Text("Display info")
                .font(Theme.Fonts.labelSemibold)
                .foregroundStyle(Theme.textPrimary)
            if let photoName = photoNameDisclosure {
                VStack(alignment: .leading, spacing: Theme.Space.xs) {
                    Text("Photo")
                        .font(Theme.Fonts.labelMedium)
                        .foregroundStyle(Theme.textPrimary)
                    Text(photoName)
                        .font(Theme.Fonts.caption)
                        .foregroundStyle(Theme.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityLabel("Photo: \(photoName)")
                        .accessibilityIdentifier("loupe-display-photo-name")
                }
            }
            Text(model.loupeInfo.isEmpty ? "Display information is unavailable." : model.loupeInfo)
                .font(Theme.Fonts.caption)
                .foregroundStyle(Theme.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityIdentifier("loupe-display-info-details")
            if proof.enabled {
                VStack(alignment: .leading, spacing: Theme.Space.xs) {
                    Text(Self.proofHeading(profileName: proof.lut?.profileName))
                        .font(Theme.Fonts.labelMedium)
                        .foregroundStyle(Theme.textPrimary)
                    Text(proofDetails)
                        .font(Theme.Fonts.caption)
                        .foregroundStyle(Theme.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
        }
        .padding(Theme.Space.m)
        .frame(minWidth: Theme.Width.toolbarSegment, alignment: .leading)
        .background(Theme.panel)
    }

    private var shortcutsPopover: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            Text("Shortcuts")
                .font(Theme.Fonts.labelSemibold)
                .foregroundStyle(Theme.textPrimary)
            Text(shortcutText)
                .font(Theme.Fonts.caption)
                .foregroundStyle(Theme.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityIdentifier("loupe-shortcut-details")
        }
        .padding(Theme.Space.m)
        .frame(minWidth: Theme.Width.toolbarSegment, alignment: .leading)
        .background(Theme.panel)
    }
}
