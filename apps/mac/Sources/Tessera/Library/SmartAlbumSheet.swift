import SwiftUI
import TesseraCore
import TesseraFFI

/// New / edit smart album. The rule is edited as a visible tree of AND ("all"), OR ("any") and
/// NOT ("none") groups, or as grammar text; both stay in sync through the engine, which also
/// locates errors in the text (red highlight). docs/06 §4.2: "visible nesting".
struct SmartAlbumSheet: View {
    @Bindable var library: LibraryModel

    var body: some View {
        if let draft = library.editor {
            content(draft)
        }
    }

    private var draft: Binding<SmartAlbumDraft> {
        Binding(get: { library.editor ?? SmartAlbumDraft(name: "", scoped: false, tree: RuleNode(kind: .all), text: "") },
                // Dismissal can commit a focused field after the draft is gone: never resurrect it.
                set: { new in if library.editor?.id == new.id { library.editor = new } })
    }

    private var tree: Binding<RuleNode> {
        Binding(get: { library.editor?.tree ?? RuleNode(kind: .all) },
                set: { new in
                    guard var d = library.editor else { return }
                    library.setTree(new, in: &d)
                    library.editor = d
                })
    }

    private var text: Binding<String> {
        Binding(get: { library.editor?.text ?? "" },
                set: { new in
                    guard var d = library.editor, d.text != new else { return }
                    library.setText(new, in: &d)
                    library.editor = d
                })
    }

    private func content(_ d: SmartAlbumDraft) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            VStack(alignment: .leading, spacing: 4) {
                Text(d.editing == nil ? "New Smart Album" : "Edit Smart Album").font(.system(size: 15, weight: .semibold))
                Text("A saved search. It updates as photos change; removing a photo from it means changing the rule.")
                    .font(.system(size: 11)).foregroundStyle(.secondary)
            }
            .padding(16)
            Divider()
            Form {
                TextField("Name", text: draft.name)
                Picker("Location", selection: draft.parent) {
                    Text("Top level").tag(Int64?.none)
                    ForEach(library.groups) { g in
                        Text(String(repeating: "   ", count: g.depth) + g.name).tag(Int64?.some(g.id))
                    }
                }
                Toggle("Only search albums in this group", isOn: Binding(
                    get: { d.scoped && d.parent != nil },
                    set: { library.editor?.scoped = $0; library.refreshEditorCount() }))
                    .disabled(d.parent == nil)
                    .help("Capture One–style project scope: match only photos in this group's albums (and nested groups)")
            }
            .formStyle(.columns)
            .padding(.horizontal, 16)
            .padding(.vertical, 10)
            .onChange(of: d.parent) { library.refreshEditorCount() }
            Divider()
            ScrollView {
                RuleGroupEditor(node: tree, isRoot: true, depth: 0, onRemove: nil)
                    .padding(12)
            }
            .frame(minHeight: 180, maxHeight: 320)
            Divider()
            VStack(alignment: .leading, spacing: 4) {
                Text("Rule text").font(.system(size: 11, weight: .medium)).foregroundStyle(.secondary)
                RuleTextField(text: text, diagnostic: d.diagnostic,
                              placeholder: "keyword:beach AND rating>=2 AND NOT decision:reject",
                              onSubmit: { library.saveEditor() })
                    .frame(height: 24)
                RuleMessage(diagnostic: d.diagnostic,
                            hint: "Edit either the conditions above or this text. AND, OR, NOT and ( ) nest; quote values with spaces.")
            }
            .padding(16)
            Divider()
            HStack {
                if let n = d.matchCount {
                    Text("\(n.formatted()) photo\(n == 1 ? "" : "s") in this folder match")
                        .font(.system(size: 11).monospacedDigit()).foregroundStyle(.secondary)
                        .accessibilityIdentifier("smartAlbumMatchCount")
                }
                Spacer()
                Button("Cancel") { library.editor = nil }
                    .keyboardShortcut(.cancelAction)
                Button(d.editing == nil ? "Create" : "Save") { library.saveEditor() }
                    .keyboardShortcut(.defaultAction)
                    .disabled(d.diagnostic != nil || d.name.trimmingCharacters(in: .whitespaces).isEmpty)
            }
            .padding(16)
        }
        .frame(width: 620)
        .background(Color(nsColor: Theme.windowBackground))
    }
}

/// One AND/OR/NOT group: a coloured rail shows its extent, so nesting is visible at a glance.
struct RuleGroupEditor: View {
    @Binding var node: RuleNode
    let isRoot: Bool
    let depth: Int
    let onRemove: (() -> Void)?

    static func color(_ kind: RuleNode.Kind) -> Color {
        switch kind {
        case .all: Color(nsColor: Theme.basket)
        case .any: Color(nsColor: Theme.accent)
        case .not: Color(nsColor: Theme.reject)
        case .rule: .secondary
        }
    }

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            RoundedRectangle(cornerRadius: 1.5)
                .fill(Self.color(node.kind).opacity(0.85))
                .frame(width: 3)
            VStack(alignment: .leading, spacing: 6) {
                HStack(spacing: 6) {
                    Text(isRoot ? "Match" : "Group:").font(.system(size: 11)).foregroundStyle(.secondary)
                    Picker("", selection: $node.kind) {
                        Text("all (AND)").tag(RuleNode.Kind.all)
                        Text("any (OR)").tag(RuleNode.Kind.any)
                        Text("none (NOT)").tag(RuleNode.Kind.not)
                    }
                    .labelsHidden()
                    .fixedSize()
                    .accessibilityIdentifier("ruleGroupKind")
                    Text("of the following:").font(.system(size: 11)).foregroundStyle(.secondary)
                    Spacer()
                    Menu("Add") {
                        Button("Condition") { node.children.append(.condition()) }
                        Button("Group (all)") { node.children.append(RuleNode(kind: .all, children: [.condition()])) }
                        Button("Group (any)") { node.children.append(RuleNode(kind: .any, children: [.condition()])) }
                        Button("Group (none)") { node.children.append(RuleNode(kind: .not, children: [.condition()])) }
                    }
                    .menuStyle(.button)
                    .fixedSize()
                    .disabled(depth >= 8)
                    if let onRemove {
                        Button { onRemove() } label: { Text("−").frame(width: 14) }
                            .help("Remove this group")
                    }
                }
                ForEach($node.children) { $child in
                    let remove = { node.children.removeAll { $0.id == child.id } }
                    if child.isGroup {
                        RuleGroupEditor(node: $child, isRoot: false, depth: depth + 1, onRemove: remove)
                            .padding(.leading, 6)
                    } else {
                        RuleConditionRow(node: $child, onRemove: remove)
                    }
                }
                if node.children.isEmpty {
                    Text("Empty group: add a condition").font(.system(size: 11)).foregroundStyle(.tertiary)
                }
            }
            .padding(.vertical, 4)
        }
        .padding(6)
        .background(RoundedRectangle(cornerRadius: 5).fill(Self.color(node.kind).opacity(isRoot ? 0.04 : 0.07)))
        .controlSize(.small)
    }
}

struct RuleConditionRow: View {
    @Binding var node: RuleNode
    let onRemove: () -> Void

    var body: some View {
        let field = RuleField.named(node.field)
        HStack(spacing: 6) {
            Picker("", selection: Binding(get: { node.field }, set: { key in
                node.field = key
                let ops = RuleField.named(key).ops.map(\.op)
                if !ops.contains(node.op) { node.op = ops.first ?? ":" }
            })) {
                ForEach(RuleField.all) { Text($0.title).tag($0.key) }
                if !RuleField.all.contains(where: { $0.key == node.field }) { Text(node.field).tag(node.field) }
            }
            .labelsHidden()
            .frame(width: 120)
            Picker("", selection: $node.op) {
                ForEach(field.ops, id: \.op) { Text($0.title).tag($0.op) }
                if !field.ops.contains(where: { $0.op == node.op }) { Text(node.op).tag(node.op) }
            }
            .labelsHidden()
            .frame(width: 84)
            TextField(field.placeholder, text: $node.value)
                .textFieldStyle(.roundedBorder)
            Button { onRemove() } label: { Text("−").frame(width: 14) }
                .help("Remove this condition")
        }
        .font(.system(size: 11))
    }
}
