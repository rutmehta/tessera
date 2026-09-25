import SwiftUI
import TesseraCore
import TesseraFFI

/// Filter bar above the grid (docs/01 §1.9, adapted): grammar text plus facet menus whose counts
/// are live. Each facet counts over every *other* active filter, so alternatives stay visible.
/// Filters apply when changed, not while culling, so decided frames do not vanish mid-cull.
struct FilterBar: View {
    @Bindable var library: LibraryModel
    let model: AppModel
    @State private var showDates = false

    var body: some View {
        let facets = library.facets
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            HStack(spacing: Theme.Space.s) {
                FieldContainer(symbol: "magnifyingglass", invalid: library.diagnostic != nil) {
                    RuleTextField(text: $library.filter.text, diagnostic: library.diagnostic,
                                  placeholder: "Search or rule, e.g. beach rating>=2 NOT decision:reject",
                                  onSubmit: {}, plain: true)
                }
                .frame(minWidth: 180, maxWidth: 400)
                .help("Words search names, captions and keywords. Fields: keyword: camera: lens: rating>= "
                      + "decision: mark: date: album: (none / any / name), combined with AND, OR, NOT and ( )")
                if let d = library.diagnostic {
                    Text(d.message)
                        .font(Theme.Fonts.caption)
                        .foregroundStyle(Theme.reject)
                        .lineLimit(1)
                        .truncationMode(.tail)
                        .help(d.message)
                        .accessibilityIdentifier("filterDiagnostic")
                }
                Spacer(minLength: Theme.Space.xs)
                if let n = library.matchCount, !library.filter.isEmpty {
                    Text("\(n.formatted()) match\(n == 1 ? "" : "es")")
                        .font(Theme.Fonts.captionNumeric)
                        .foregroundStyle(Theme.textSecondary)
                        .fixedSize()
                        .accessibilityIdentifier("filterMatchCount")
                }
                if !library.filter.isEmpty {
                    Button("Clear") { library.clearFilter() }
                        .buttonStyle(.themeBorderless)
                        .fixedSize()
                        .help("Remove all filters (⌥⌘L)")
                }
                Button("Save as Smart Album…") { library.saveFilterAsSmartAlbum() }
                    .buttonStyle(.themeBorderless)
                    .fixedSize()
                    .disabled(library.composedRule.isEmpty || library.diagnostic != nil)
                    .help(library.composedRule.isEmpty ? "Set a filter (or open an album) first"
                          : "Save “\(library.composedRule)” as a smart album")
            }
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: Theme.Space.xs) {
                    facetMenu("Decision", \.decisions, values: ["keep", "undecided", "reject"].map { v in
                        (v, v.capitalized, count(facets?.decisions, v))
                    })
                    facetMenu("Grade", \.grades, values: ["1", "2", "3"].map { g in
                        (g, "\(g) · \(CullState.gradeNames[Int(g)!])", count(facets?.grades, g))
                    })
                    facetMenu("Mark", \.marks, values: markValues(facets))
                    facetMenu("Camera", \.cameras, values: listed(facets?.cameras, selected: library.filter.cameras))
                    facetMenu("Lens", \.lenses, values: listed(facets?.lenses, selected: library.filter.lenses))
                    facetMenu("Keyword", \.keywords, values: listed(facets?.keywords, selected: library.filter.keywords))
                    dateButton
                    albumMenu(facets)
                }
            }
        }
        .font(Theme.Fonts.caption)
        .padding(.horizontal, Theme.Space.gutter)
        .padding(.vertical, Theme.Space.s)
        .background(Theme.panel)
        .overlay(alignment: .bottom) { Hairline() }
        .disabled(!library.isAvailable)
    }

    private func count(_ list: [FacetCount]?, _ value: String) -> Int {
        Int(list?.first { $0.value == value }?.count ?? 0)
    }

    /// Facet values by count; selected values stay listed even at zero.
    private func listed(_ list: [FacetCount]?, selected: Set<String>) -> [(String, String, Int)] {
        var rows = (list ?? []).filter { !$0.value.isEmpty }.map { ($0.value, $0.value, Int($0.count)) }
        for v in selected where !rows.contains(where: { $0.0 == v }) { rows.append((v, v, 0)) }
        return rows
    }

    private func markValues(_ facets: SearchFacets?) -> [(String, String, Int)] {
        var names = CullController.markNames.sorted { $0.key < $1.key }.map(\.value)
        for c in facets?.marks ?? [] where !names.contains(c.value) { names.append(c.value) }
        return names.map { ($0, $0, count(facets?.marks, $0)) }
    }

    private func facetMenu(_ title: String, _ keyPath: WritableKeyPath<LibraryFilter, Set<String>>,
                           values: [(value: String, label: String, count: Int)]) -> some View {
        let selected = library.filter[keyPath: keyPath]
        return Menu {
            if values.isEmpty {
                Text("No values in this folder")
            }
            ForEach(values, id: \.value) { row in
                Toggle(isOn: Binding(get: { selected.contains(row.value) },
                                     set: { _ in library.toggle(keyPath, row.value) })) {
                    Text("\(row.label)    \(row.count.formatted())")
                }
            }
            if !selected.isEmpty {
                Divider()
                Button("Any \(title)") { library.filter[keyPath: keyPath] = [] }
            }
        } label: {
            Text(selected.isEmpty ? title : "\(title) · \(selected.count == 1 ? selected.first! : "\(selected.count)")")
                .lineLimit(1)
        }
        .menuStyle(ThemeMenuStyle(height: Theme.Height.small, active: !selected.isEmpty))
        .accessibilityIdentifier("facet\(title)")
    }

    private var dateButton: some View {
        let active = library.filter.dateValue != nil
        return Button(active ? "Date · \(library.filter.dateValue!.replacingOccurrences(of: "0001..", with: "…").replacingOccurrences(of: "..9998", with: "…"))" : "Date") {
            showDates.toggle()
        }
        .buttonStyle(ThemeButtonStyle(kind: .bordered, height: Theme.Height.small))
        .overlay(RoundedRectangle(cornerRadius: Theme.Radius.control)
            .strokeBorder(active ? Theme.accent.opacity(0.5) : Theme.clear, lineWidth: Theme.Space.hairline))
        .fixedSize()
        .popover(isPresented: $showDates, arrowEdge: .bottom) {
            VStack(alignment: .leading, spacing: Theme.Space.s) {
                Text("Capture date").font(Theme.Fonts.labelSemibold)
                HStack(spacing: Theme.Space.s) {
                    TextField("From  YYYY[-MM[-DD]]", text: $library.filter.dateFrom).frame(width: 150)
                    Text("to").foregroundStyle(Theme.textSecondary)
                    TextField("To", text: $library.filter.dateTo).frame(width: 150)
                }
                .textFieldStyle(.roundedBorder)
                Hint("Inclusive. 2024 is the whole year; 2024-06 the whole month.")
                HStack(spacing: Theme.Space.s) {
                    Button("Clear") { library.filter.dateFrom = ""; library.filter.dateTo = "" }.buttonStyle(.themeBordered)
                    Spacer()
                    Button("Done") { showDates = false }.keyboardShortcut(.defaultAction).buttonStyle(.themePrimary)
                }
            }
            .font(Theme.Fonts.label)
            .padding(Theme.Space.m)
            .tint(Theme.accent)
        }
    }

    private func albumMenu(_ facets: SearchFacets?) -> some View {
        let status = library.filter.albumStatus
        return Menu {
            Toggle(isOn: Binding(get: { status == "none" }, set: { library.filter.albumStatus = $0 ? "none" : nil })) {
                Text("Not in any album    \((facets?.inNoAlbum ?? 0).formatted())")
            }
            Toggle(isOn: Binding(get: { status == "any" }, set: { library.filter.albumStatus = $0 ? "any" : nil })) {
                Text("In an album    \((facets?.inAnyAlbum ?? 0).formatted())")
            }
        } label: {
            Text(status == "none" ? "Not in Album" : status == "any" ? "In Album" : "Album")
        }
        .menuStyle(ThemeMenuStyle(height: Theme.Height.small, active: status != nil))
        .accessibilityIdentifier("facetAlbum")
    }
}
