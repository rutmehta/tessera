import SwiftUI

/// Loupe chrome (UX-16): in Edit nothing is drawn on the photo but the dial, the ring, the chips,
/// the crop overlay, the heal spots and the Try cards / agent row (`EditPhotoHUDs`, `AgentRow`);
/// the display-info popover is the toolbar's `i` (`LoupeInfoButton`). The library loupe (E from
/// Cull) keeps one scrim with the photo's name and its decision top left; the former Display info,
/// Masks and Shortcuts pills are gone (docs/shortcuts.md lists the keys).
struct LoupeOverlay: View {
    let model: AppModel

    private var proof: SoftProof { SoftProof.shared }

    static func proofBadgeText(enabled: Bool, profileName: String?, status: String, gamutWarning: Bool) -> String? {
        guard enabled else { return nil }
        if let profileName {
            return "Proof · \(profileName)" + (gamutWarning ? " · gamut warning" : "")
        }
        return status.isEmpty ? "Proof unavailable" : status
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

    var body: some View {
        VStack(spacing: 0) {
            if let item = model.focusedItem, !model.isPhotoEditing {
                HStack(alignment: .center, spacing: Theme.Space.s) {
                    Text(item.name)
                        .font(Theme.Fonts.labelMedium)
                        .foregroundStyle(Color(nsColor: Theme.Palette.OnImage.text))
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .accessibilityLabel("Photo: \(item.name)")
                        .padding(.horizontal, Theme.Space.s)
                        .background(Capsule().fill(Color(nsColor: Theme.Palette.OnImage.scrim)))
                    if let badge = model.focusedState.badgeText {
                        Chip(text: badge, color: Color(nsColor: model.focusedState.decision.color), style: .outlined,
                             height: Theme.Height.chip)
                            .help(badge)
                            .padding(.horizontal, Theme.Space.xxs)
                            .background(Capsule().fill(Color(nsColor: Theme.Palette.OnImage.scrim)))
                    }
                    Spacer(minLength: Theme.Space.s)
                }
                .padding(.horizontal, Theme.Space.panelInset)
                .frame(height: Theme.Height.sectionHeader)
                .allowsHitTesting(false)
            }
            Spacer()
                .allowsHitTesting(false)
        }
        .onDisappear {
            model.loupeDisclosurePresented = false
            model.dismissLoupeDisclosure = nil
        }
    }

    /// Esc while a loupe disclosure is up closes it (`KeyRouter`); the toolbar's info button and
    /// any other popover hand their bindings here.
    @MainActor
    static func makeDismissAction(displayInfo: Binding<Bool>, shortcuts: Binding<Bool>) -> @MainActor () -> Void {
        {
            displayInfo.wrappedValue = false
            shortcuts.wrappedValue = false
        }
    }

    /// The display-info popover: the photo (library loupe), the screen's colour space and EDR
    /// headroom, and the soft-proof status. Opened from the toolbar's `i` (`LoupeInfoButton`).
    var displayInfoPopover: some View {
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
}

/// UX-16: the `i` in the toolbar, right of Before, that opens the display-info popover (the pill
/// on the photo is gone). Esc closes it through the loupe's disclosure ownership, as before.
struct LoupeInfoButton: View {
    let model: AppModel
    @State private var presented = false

    var body: some View {
        Button { presented.toggle() } label: {
            Label("Info", systemImage: "info.circle").labelStyle(.iconOnly)
        }
        .buttonStyle(ToolbarButtonStyle(on: presented))
        .help("Display information and soft proof status")
        .accessibilityLabel("Display info")
        .accessibilityIdentifier("loupe-display-info")
        .popover(isPresented: $presented, arrowEdge: .bottom) {
            LoupeOverlay(model: model).displayInfoPopover
                .onExitCommand { presented = false }
        }
        .onChange(of: presented) { _, on in
            model.loupeDisclosurePresented = on
            if on {
                let dismiss: @MainActor () -> Void = { presented = false }
                model.dismissLoupeDisclosure = dismiss
            } else {
                model.dismissLoupeDisclosure = nil
            }
        }
    }
}
