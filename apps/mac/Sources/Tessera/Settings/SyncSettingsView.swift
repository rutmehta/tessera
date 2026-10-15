import SwiftUI
import TesseraCore

/// Settings ▸ Sync (SPEC §3–4, CS-2): the account the catalog follows, the switch, what syncs,
/// when it last did and how much it takes. With no container the reason is shown with its
/// one-line fix, and the switch is disabled; everything else in the app works as before.
struct SyncSettingsView: View {
    let sync: AccountSync

    private static let items: [(String, String)] = [
        ("AI and appearance preferences", "Provider and model choices, guardrails, culling thresholds, accent and ground. API keys stay in the Keychain."),
        ("Keyword hierarchy", "The keyword list and its parents; the tags on each photo travel with the photo."),
        ("Style profile and culling learner", "What Tessera learned from your edits and decisions, per shoot, merged into one account profile."),
        ("People names", "Names and merges by person; face data stays on the drive."),
        ("Drives", "Each drive's opened folders, counts and where it was last seen, so the folder popover lists it on any Mac."),
    ]

    var body: some View {
        Form {
            Section("iCloud") {
                LabeledContent("Account") {
                    Text("Synced as \(sync.accountDescription)")
                        .foregroundStyle(Theme.textPrimary)
                        .accessibilityIdentifier("sync-account")
                }
                Toggle("Sync the catalog with iCloud", isOn: Binding(get: { sync.isEnabled }, set: { sync.setEnabled($0) }))
                    .disabled(sync.status != .available)
                    .accessibilityIdentifier("sync-enabled")
                switch sync.status {
                case .resolving:
                    Hint("Checking iCloud…")
                case .unavailable(let reason):
                    StatusLine(text: reason.message, kind: .warning)
                        .accessibilityIdentifier("sync-unavailable")
                    Hint(reason.fix)
                case .available:
                    LabeledContent("Last sync") {
                        Text(lastSyncText).font(Theme.Fonts.bodyNumeric).foregroundStyle(Theme.textSecondary)
                            .accessibilityIdentifier("sync-last")
                    }
                    LabeledContent("Storage used") {
                        Text(storageText).font(Theme.Fonts.bodyNumeric).foregroundStyle(Theme.textSecondary)
                            .accessibilityIdentifier("sync-storage")
                    }
                    Hint(sync.isEnabled ? "Changes made on another Mac apply as they arrive."
                         : "Off: nothing is written to or read from the account. Everything stays on this Mac.")
                }
            }
            Section("What syncs") {
                ForEach(Self.items, id: \.0) { title, detail in
                    VStack(alignment: .leading, spacing: Theme.Space.xxs) {
                        Text(title).font(Theme.Fonts.body).foregroundStyle(Theme.textPrimary)
                        Text(detail).font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
                Hint("Edits, decisions, albums and photos stay with the drive they are on; nothing from a photo is uploaded.")
            }
        }
        .formStyle(.grouped)
        .frame(width: 520, height: 520)
        .tint(Theme.accent)
        .onAppear { sync.refreshStats() }
    }

    private var lastSyncText: String {
        guard let date = sync.stats.lastSync else { return "Not yet" }
        return date.formatted(.relative(presentation: .named)) + " · " + date.formatted(date: .abbreviated, time: .shortened)
    }

    private var storageText: String {
        let bytes = ByteCountFormatter.string(fromByteCount: sync.stats.bytes, countStyle: .file)
        return sync.stats.files == 0 ? "Nothing yet" : "\(bytes) in \(sync.stats.files) file\(sync.stats.files == 1 ? "" : "s")"
    }
}
