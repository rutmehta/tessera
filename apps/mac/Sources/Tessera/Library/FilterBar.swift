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
        VStack(alignment: .leading, spacing: 5) {
            HStack(spacing: 8) {
                RuleTextField(text: $library.filter.text, diagnostic: library.diagnostic,
                              placeholder: "Search or rule, e.g.  beach rating>=2 NOT decision:reject",
                              onSubmit: {})
                    .frame(minWidth: 160, maxWidth: 420)
                    .help("Words search names, captions and keywords. Fields: keyword: camera: lens: rating>= "
                          + "decision: mark: date: album: (none / any / name), combined with AND, OR, NOT and ( )")
                if let d = library.diagnostic {
                    Text(d.message)
                        .foregroundStyle(Color(nsColor: Theme.reject))
                        .lineLimit(1)
                        .truncationMode(.tail)
                        .help(d.message)
                        .accessibilityIdentifier("filterDiagnostic")
                }
                Spacer(minLength: 4)
                if let n = library.matchCount, !library.filter.isEmpty {
                    Text("\(n.formatted()) match\(n == 1 ? "" : "es")")
                        .monospacedDigit()
                        .foregroundStyle(.secondary)
                        .fixedSize()
                        .accessibilityIdentifier("filterMatchCount")
                }
                if !library.filter.isEmpty {
                    Button("Clear") { library.clearFilter() }
                        .fixedSize()
                        .help("Remove all filters (⌥⌘L)")
                }
                Button("Save as Smart Album…") { library.saveFilterAsSmartAlbum() }
                    .fixedSize()
                    .disabled(library.composedRule.isEmpty || library.diagnostic != nil)
                    .help(library.composedRule.isEmpty ? "Set a filter (or open an album) first"
                          : "Save “\(library.composedRule)” as a smart album")
            }
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 6) {
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
                .padding(.vertical, 1)
            }
        }
        .controlSize(.small)
        .font(.system(size: 11))
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background(Color(nsColor: Theme.windowBackground))
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
        .menuStyle(.button)
        .fixedSize()
        .tint(selected.isEmpty ? nil : Color(nsColor: Theme.accent))
        .accessibilityIdentifier("facet\(title)")
    }

    private var dateButton: some View {
        let active = library.filter.dateValue != nil
        return Button(active ? "Date · \(library.filter.dateValue!.replacingOccurrences(of: "0001..", with: "…").replacingOccurrences(of: "..9998", with: "…"))" : "Date") {
            showDates.toggle()
        }
        .fixedSize()
        .popover(isPresented: $showDates, arrowEdge: .bottom) {
            VStack(alignment: .leading, spacing: 8) {
                Text("Capture date").font(.system(size: 12, weight: .semibold))
                HStack {
                    TextField("From  YYYY[-MM[-DD]]", text: $library.filter.dateFrom).frame(width: 150)
                    Text("to")
                    TextField("To", text: $library.filter.dateTo).frame(width: 150)
                }
                Text("Inclusive. 2024 is the whole year; 2024-06 the whole month.")
                    .font(.system(size: 11)).foregroundStyle(.secondary)
                HStack {
                    Button("Clear") { library.filter.dateFrom = ""; library.filter.dateTo = "" }
                    Spacer()
                    Button("Done") { showDates = false }.keyboardShortcut(.defaultAction)
                }
            }
            .padding(12)
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
        .menuStyle(.button)
        .fixedSize()
        .tint(status == nil ? nil : Color(nsColor: Theme.accent))
        .accessibilityIdentifier("facetAlbum")
    }
}
