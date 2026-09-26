import SwiftUI
import TesseraCore

/// History panel: the document's states (the current one highlighted, click = checkout), the
/// snapshots with New Snapshot and restore, and the memory the states hold.
struct DocumentHistoryPanel: View {
    let document: DocumentController
    let workspace: DocumentWorkspace

    var body: some View {
        let items = document.history
        let atBase = document.info.historyHead == 0
        let onPath = pathToHead(items)
        VStack(alignment: .leading, spacing: 0) {
            ScrollViewReader { proxy in
                ScrollView {
                    VStack(alignment: .leading, spacing: 0) {
                        row(title: "Opened", symbol: "doc", current: atBase, dimmed: false, index: 0) { document.checkout(0) }
                        ForEach(Array(items.enumerated()), id: \.element.id) { i, item in
                            row(title: item.label, symbol: item.author == "user" ? "circle" : "sparkles", current: item.isCurrent,
                                dimmed: !onPath.contains(item.id), index: i + 1) { document.checkout(item.id) }
                                .id(item.id)
                        }
                    }
                }
                .frame(height: Theme.Height.row * 6)
                .onChange(of: document.info.historyHead) { _, head in if head != 0 { proxy.scrollTo(head) } }
            }
            SubHeader("Snapshots")
            if document.snapshots.isEmpty {
                Hint("None yet")
            }
            ForEach(Array(document.snapshots.enumerated()), id: \.element) { i, snap in
                HStack(spacing: Theme.Space.s) {
                    Image(systemName: "camera").font(Theme.Fonts.iconSmall).foregroundStyle(Theme.textTertiary)
                    Text(snap).font(Theme.Fonts.caption).foregroundStyle(Theme.textPrimary).lineLimit(1)
                    Spacer(minLength: 0)
                    Button("Restore") { document.restoreSnapshot(snap) }
                        .buttonStyle(.theme(.borderless, height: Theme.Height.small))
                        .accessibilityIdentifier("document.history.snapshot.\(i).restore")
                }
                .frame(height: Theme.Height.row)
                .accessibilityIdentifier("document.history.snapshot.\(i)")
            }
            HStack(spacing: Theme.Space.s) {
                Button("New Snapshot…") { workspace.promptSnapshot() }
                    .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                    .accessibilityIdentifier("document.history.newSnapshot")
                Spacer(minLength: 0)
                Text(memoryText).font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textTertiary)
                    .accessibilityIdentifier("document.history.memory")
            }
            .padding(.top, Theme.Space.s)
        }
    }

    private var memoryText: String {
        let n = document.history.count
        return "\(n) state\(n == 1 ? "" : "s") · " + ByteCountFormatter.string(fromByteCount: Int64(document.memoryBytes), countStyle: .memory)
    }

    /// Entries from the base to the head (the rest are undone steps redo would reapply).
    private func pathToHead(_ items: [DocHistoryEntry]) -> Set<DocHistoryID> {
        let parent = Dictionary(uniqueKeysWithValues: items.map { ($0.id, $0.parent) })
        var out = Set<DocHistoryID>()
        var p: DocHistoryID? = document.info.historyHead == 0 ? nil : document.info.historyHead
        while let id = p { out.insert(id); p = parent[id] ?? nil }
        return out
    }

    private func row(title: String, symbol: String, current: Bool, dimmed: Bool, index: Int,
                     action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack(spacing: Theme.Space.s - Theme.Space.xxs) {
                Image(systemName: symbol).font(Theme.Fonts.iconSmall).foregroundStyle(Theme.textTertiary)
                    .frame(width: Theme.Space.l)
                Text(title).font(Theme.Fonts.caption)
                    .foregroundStyle(dimmed ? Theme.textTertiary : Theme.textPrimary)
                    .lineLimit(1)
                Spacer(minLength: 0)
            }
            .padding(.horizontal, Theme.Space.xs)
            .frame(height: Theme.Height.row)
            .background(RoundedRectangle(cornerRadius: Theme.Radius.control).fill(current ? Theme.accentSubtle : Theme.clear))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier("document.history.row.\(index)")
        .accessibilityAddTraits(current ? .isSelected : [])
        .help(index == 0 ? "The document as opened" : "Go back to this state (later states stay, for redo)")
    }
}
