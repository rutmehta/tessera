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
        SheetScaffold(title: d.editing == nil ? "New Smart Album" : "Edit Smart Album",
                      subtitle: "A saved search. It updates as photos change; removing a photo from it means changing the rule.") {
            EmptyView()
        } content: {
            VStack(alignment: .leading, spacing: 0) {
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
                .font(Theme.Fonts.label)
                .padding(.horizontal, Theme.Space.l)
                .padding(.vertical, Theme.Space.m)
                .onChange(of: d.parent) { library.refreshEditorCount() }
                Hairline()
                ScrollView {
                    RuleGroupEditor(node: tree, isRoot: true, depth: 0, onRemove: nil)
                        .padding(Theme.Space.m)
                }
                .frame(minHeight: 180, maxHeight: 320)
                Hairline()
                VStack(alignment: .leading, spacing: Theme.Space.xs) {
                    Text("Rule text").font(Theme.Fonts.captionMedium).foregroundStyle(Theme.textSecondary)
                    FieldContainer(invalid: d.diagnostic != nil) {
                        RuleTextField(text: text, diagnostic: d.diagnostic,
                                      placeholder: "keyword:beach AND rating>=2 AND NOT decision:reject",
                                      onSubmit: { library.saveEditor() }, plain: true, monospaced: true)
                    }
                    RuleMessage(diagnostic: d.diagnostic,
                                hint: "Edit either the conditions above or this text. AND, OR, NOT and ( ) nest; quote values with spaces.")
                }
                .padding(Theme.Space.l)
            }
        } leading: {
            if let n = d.matchCount {
                Text("\(n.formatted()) photo\(n == 1 ? "" : "s") in this folder match")
                    .monospacedDigit()
                    .accessibilityIdentifier("smartAlbumMatchCount")
            }
        } actions: {
            Button("Cancel") { library.editor = nil }
                .keyboardShortcut(.cancelAction)
                .sheetButton()
            Button(d.editing == nil ? "Create" : "Save") { library.saveEditor() }
                .keyboardShortcut(.defaultAction)
                .sheetButton(primary: true)
                .disabled(d.diagnostic != nil || d.name.trimmingCharacters(in: .whitespaces).isEmpty)
        }
        .frame(width: 620)
    }
}

/// One AND/OR/NOT group: a coloured rail shows its extent, so nesting is visible at a glance.
struct RuleGroupEditor: View {
    @Binding var node: RuleNode
    let isRoot: Bool
    let depth: Int
    let onRemove: (() -> Void)?

    /// Nesting is shown by indentation and a neutral rail; the kind is spelled out in the picker
    /// (semantic colours stay reserved for cull decisions). NOT keeps a dashed rail.
    static func dashed(_ kind: RuleNode.Kind) -> Bool { kind == .not }

    var body: some View {
        HStack(alignment: .top, spacing: Theme.Space.s) {
            Rectangle()
                .stroke(Theme.hairlineStrong, style: StrokeStyle(lineWidth: Theme.Space.xxs, dash: Self.dashed(node.kind) ? [4, 3] : []))
                .frame(width: Theme.Space.xxs)
            VStack(alignment: .leading, spacing: Theme.Space.s - Theme.Space.xxs) {
                HStack(spacing: Theme.Space.s - Theme.Space.xxs) {
                    Text(isRoot ? "Match" : "Group:").font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                    Picker("", selection: $node.kind) {
                        Text("all (AND)").tag(RuleNode.Kind.all)
                        Text("any (OR)").tag(RuleNode.Kind.any)
                        Text("none (NOT)").tag(RuleNode.Kind.not)
                    }
                    .labelsHidden()
                    .fixedSize()
                    .accessibilityIdentifier("ruleGroupKind")
                    Text("of the following:").font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                    Spacer()
                    Menu("Add") {
                        Button("Condition") { node.children.append(.condition()) }
                        Button("Group (all)") { node.children.append(RuleNode(kind: .all, children: [.condition()])) }
                        Button("Group (any)") { node.children.append(RuleNode(kind: .any, children: [.condition()])) }
                        Button("Group (none)") { node.children.append(RuleNode(kind: .not, children: [.condition()])) }
                    }
                    .menuStyle(ThemeMenuStyle(height: Theme.Height.small))
                    .disabled(depth >= 8)
                    if let onRemove {
                        IconButton(symbol: "minus", help: "Remove this group", size: Theme.Height.small) { onRemove() }
                    }
                }
                ForEach($node.children) { $child in
                    let remove = { node.children.removeAll { $0.id == child.id } }
                    if child.isGroup {
                        RuleGroupEditor(node: $child, isRoot: false, depth: depth + 1, onRemove: remove)
                            .padding(.leading, Theme.Space.s - Theme.Space.xxs)
                    } else {
                        RuleConditionRow(node: $child, onRemove: remove)
                    }
                }
                if node.children.isEmpty {
                    Hint("Empty group: add a condition")
                }
            }
            .padding(.vertical, Theme.Space.xs)
        }
        .padding(Theme.Space.s - Theme.Space.xxs)
        .background(RoundedRectangle(cornerRadius: Theme.Radius.control).fill(isRoot ? Theme.clear : Theme.hover))
        .controlSize(.small)
    }
}

struct RuleConditionRow: View {
    @Binding var node: RuleNode
    let onRemove: () -> Void

    var body: some View {
        let field = RuleField.named(node.field)
        HStack(spacing: Theme.Space.s - Theme.Space.xxs) {
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
            IconButton(symbol: "minus", help: "Remove this condition", size: Theme.Height.small) { onRemove() }
        }
        .font(Theme.Fonts.caption)
    }
}
