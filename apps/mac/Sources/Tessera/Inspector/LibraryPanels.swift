import AppKit
import SwiftUI
import TesseraCore
import TesseraFFI

/// Keywords applied to the focused photo, a field to add keywords to the whole selection, and the
/// keyword hierarchy (library.json) with per-folder counts and bulk apply / remove.
struct KeywordsPanel: View {
    let model: AppModel
    @Bindable var library: LibraryModel
    @State private var entry = ""

    var body: some View {
        let applied = library.metadata?.keywords ?? []
        let n = max(model.selectionCount, 1)
        VStack(alignment: .leading, spacing: 8) {
            if applied.isEmpty {
                Text(model.focusedItem == nil ? "No image selected" : "No keywords")
                    .font(.system(size: 11)).foregroundStyle(.secondary)
            } else {
                FlowRow(spacing: 4) {
                    ForEach(applied, id: \.self) { k in
                        KeywordChip(name: k, mixed: library.mixed.contains("keywords")) {
                            library.applyKeywords([k], add: false)
                        }
                    }
                }
            }
            TextField(n > 1 ? "Add keywords to \(n) photos (comma separated)" : "Add keywords (comma separated)", text: $entry)
                .textFieldStyle(.roundedBorder)
                .font(.system(size: 11))
                .onSubmit {
                    library.applyKeywords(entry.split(separator: ",").map(String.init), add: true)
                    entry = ""
                }
                .disabled(model.focusedItem == nil)
                .accessibilityIdentifier("keywordEntry")
            HStack {
                Text("Keyword list").font(.system(size: 10, weight: .medium)).foregroundStyle(.tertiary)
                Spacer()
                Button("New…") { library.newKeyword(parent: nil) }
                    .buttonStyle(.link).font(.system(size: 11))
            }
            if library.keywords.isEmpty {
                Text("Keywords you add appear here as a hierarchy. Searching a parent finds its children.")
                    .font(.system(size: 11)).foregroundStyle(.tertiary)
            } else {
                ScrollView {
                    VStack(alignment: .leading, spacing: 1) {
                        ForEach(library.keywords, id: \.name) { k in
                            KeywordTreeRow(keyword: k, applied: applied.contains(k.name), library: library,
                                           all: library.keywords, selectionCount: n)
                        }
                    }
                }
                .frame(maxHeight: 220)
            }
        }
    }
}

private struct KeywordChip: View {
    let name: String
    let mixed: Bool
    let remove: () -> Void
    var body: some View {
        HStack(spacing: 3) {
            Text(name).font(.system(size: 11))
            Button(action: remove) { Text("×").font(.system(size: 11, weight: .semibold)) }
                .buttonStyle(.plain)
                .foregroundStyle(.secondary)
                .help("Remove from the selected photos")
        }
        .padding(.horizontal, 6).padding(.vertical, 2)
        .background(RoundedRectangle(cornerRadius: 4).fill(Color.white.opacity(mixed ? 0.04 : 0.08)))
        .overlay(RoundedRectangle(cornerRadius: 4).strokeBorder(Color.white.opacity(0.1)))
    }
}

private struct KeywordTreeRow: View {
    let keyword: KeywordInfo
    let applied: Bool
    let library: LibraryModel
    let all: [KeywordInfo]
    let selectionCount: Int

    var body: some View {
        HStack(spacing: 6) {
            Text(applied ? "✓" : " ").font(.system(size: 10, weight: .bold, design: .monospaced))
                .foregroundStyle(Color(nsColor: Theme.keep)).frame(width: 10)
            Text(keyword.name)
                .font(.system(size: 11))
                .foregroundStyle(keyword.inTree ? .primary : .secondary)
                .padding(.leading, CGFloat(keyword.depth) * 12)
                .lineLimit(1)
                .help(keyword.inTree ? "" : "Found in sidecars; not in the keyword list")
            Spacer(minLength: 4)
            if keyword.count > 0 {
                Text(keyword.count.formatted()).font(.system(size: 10).monospacedDigit()).foregroundStyle(.tertiary)
            }
            Button { library.applyKeywords([keyword.name], add: true) } label: { Text("+").frame(width: 12) }
                .buttonStyle(.plain).foregroundStyle(.secondary)
                .help("Add to \(selectionCount) selected photo\(selectionCount == 1 ? "" : "s")")
            Button { library.applyKeywords([keyword.name], add: false) } label: { Text("−").frame(width: 12) }
                .buttonStyle(.plain).foregroundStyle(.secondary)
                .help("Remove from \(selectionCount) selected photo\(selectionCount == 1 ? "" : "s")")
        }
        .padding(.vertical, 1)
        .contentShape(Rectangle())
        .contextMenu {
            Button("New Keyword Inside “\(keyword.name)”…") { library.newKeyword(parent: keyword.name) }
            Menu("Move Into") {
                ForEach(all.filter { $0.name != keyword.name && $0.inTree }, id: \.name) { other in
                    Button(String(repeating: "  ", count: Int(other.depth)) + other.name) {
                        library.moveKeyword(keyword.name, to: other.name)
                    }
                }
            }
            Button("Move to Top Level") { library.moveKeyword(keyword.name, to: nil) }
                .disabled(keyword.parent == nil && keyword.inTree)
            Divider()
            Button("Delete from Keyword List") { library.deleteKeyword(keyword.name) }
                .disabled(!keyword.inTree)
        }
    }
}

/// Wrapping row of chips.
struct FlowRow: Layout {
    var spacing: CGFloat = 4

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? 240
        var x: CGFloat = 0, y: CGFloat = 0, line: CGFloat = 0
        for v in subviews {
            let s = v.sizeThatFits(.unspecified)
            if x > 0, x + s.width > width { x = 0; y += line + spacing; line = 0 }
            x += s.width + spacing
            line = max(line, s.height)
        }
        return CGSize(width: width, height: y + line)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var x = bounds.minX, y = bounds.minY, line: CGFloat = 0
        for v in subviews {
            let s = v.sizeThatFits(.unspecified)
            if x > bounds.minX, x + s.width > bounds.maxX { x = bounds.minX; y += line + spacing; line = 0 }
            v.place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(s))
            x += s.width + spacing
            line = max(line, s.height)
        }
    }
}

/// Editable IPTC core (title, caption, creator, copyright, keywords) written to XMP sidecars for
/// the whole selection, then read-only file / camera / EXIF facts.
struct MetadataPanel: View {
    let model: AppModel
    @Bindable var library: LibraryModel

    private enum Field: Hashable { case title, caption, creator, copyright, keywords }
    @State private var values: [Field: String] = [:]
    @State private var loaded: [Field: String] = [:]
    @FocusState private var focus: Field?
    /// Photos an edit applies to, captured when a field gains focus (the selection may change
    /// before the field loses focus and commits).
    @State private var targets: [Int] = []

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            if model.focusedItem == nil {
                Text("No image selected").font(.system(size: 11)).foregroundStyle(.secondary)
            } else {
                field("Title", .title, key: "title")
                field("Caption", .caption, key: "caption", lines: 3)
                field("Creator", .creator, key: "creator", prompt: "Name; Name")
                field("Copyright", .copyright, key: "copyright", prompt: "© 2026 Name")
                field("Keywords", .keywords, key: "keywords", prompt: "comma separated")
                Text(model.selectionCount > 1
                     ? "Edits apply to \(model.selectionCount) selected photos (XMP sidecars)."
                     : "Saved to the photo's XMP sidecar when you press Return or leave the field.")
                    .font(.system(size: 10)).foregroundStyle(.tertiary)
                if let fields = library.metadata?.fields, !fields.isEmpty {
                    Divider().padding(.vertical, 4)
                    ForEach(Array(fields.enumerated()), id: \.offset) { _, f in
                        HStack(alignment: .firstTextBaseline) {
                            Text(f.name).foregroundStyle(.secondary).frame(width: 96, alignment: .leading)
                                .lineLimit(1).truncationMode(.tail)
                            Text(f.value).lineLimit(2).truncationMode(.middle).textSelection(.enabled)
                            Spacer(minLength: 0)
                        }
                        .font(.system(size: 11))
                        .help("\(f.group): \(f.name)")
                    }
                }
            }
        }
        .onAppear(perform: load)
        .onChange(of: library.metadataRevision) { load() }
        .onChange(of: focus) { old, new in
            if let old { commit(old) }
            if new != nil { targets = model.targetIDs }
        }
    }

    private func field(_ label: String, _ f: Field, key: String, prompt: String = "", lines: Int = 1) -> some View {
        let mixed = library.mixed.contains(key)
        return HStack(alignment: .firstTextBaseline) {
            Text(label).font(.system(size: 11)).foregroundStyle(.secondary).frame(width: 64, alignment: .leading)
            TextField(mixed ? "Mixed" : prompt, text: Binding(get: { values[f] ?? "" }, set: { values[f] = $0 }),
                      axis: lines > 1 ? .vertical : .horizontal)
                .lineLimit(1...max(lines, 1))
                .textFieldStyle(.roundedBorder)
                .font(.system(size: 11))
                .focused($focus, equals: f)
                .onSubmit { commit(f) }
                .accessibilityIdentifier("iptc-\(key)")
        }
    }

    private func load() {
        if let f = focus { commit(f) }
        let m = library.metadata
        let mixed = library.mixed
        let fresh: [Field: String] = [
            .title: mixed.contains("title") ? "" : m?.title ?? "",
            .caption: mixed.contains("caption") ? "" : m?.caption ?? "",
            .creator: mixed.contains("creator") ? "" : m?.creator ?? "",
            .copyright: mixed.contains("copyright") ? "" : m?.copyright ?? "",
            .keywords: mixed.contains("keywords") ? "" : (m?.keywords ?? []).joined(separator: ", "),
        ]
        values = fresh
        loaded = fresh
    }

    private func commit(_ f: Field) {
        let value = values[f] ?? ""
        guard value != loaded[f] else { return }
        var edit = IptcEdit(title: nil, caption: nil, copyright: nil, creator: nil, keywords: nil)
        switch f {
        case .title: edit.title = value
        case .caption: edit.caption = value
        case .creator: edit.creator = value
        case .copyright: edit.copyright = value
        case .keywords: edit.keywords = value.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }
        }
        loaded[f] = value
        library.saveIPTC(edit, items: targets.isEmpty ? model.targetIDs : targets)
    }
}
