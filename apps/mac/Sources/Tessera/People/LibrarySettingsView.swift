import SwiftUI
import TesseraCore

/// Settings ▸ Library (WP M2-40): what naming a person writes. Both off by default: names live in
/// the catalog only and no sidecar is touched.
struct LibrarySettingsView: View {
    @Bindable var people: PeopleModel

    var body: some View {
        Form {
            Section("People") {
                Toggle("Write face regions to XMP", isOn: $people.naming.writeFaceRegions)
                    .accessibilityIdentifier("people-setting-write-regions")
                Toggle("Add person keywords", isOn: $people.naming.personKeywords)
                    .disabled(!people.naming.writeFaceRegions)
                    .accessibilityIdentifier("people-setting-person-keywords")
                Hint("When on, naming a person writes Metadata Working Group face regions (and, optionally, the name as a "
                     + "keyword) to every photo of that person, so other apps see the names. Existing keywords are never "
                     + "removed. Off: names are kept in the library only.")
            }
        }
        .formStyle(.grouped)
        .frame(width: 520, height: 240)
        .tint(Theme.accent)
    }
}
