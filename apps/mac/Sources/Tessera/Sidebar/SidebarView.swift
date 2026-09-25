import TesseraCore
import SwiftUI

/// Left sidebar: library sources, folders, albums. Plain text rows with counts; no icon column.
struct SidebarView: View {
    let model: AppModel

    var body: some View {
        let selection = Binding<LibrarySource?>(
            get: { model.source },
            set: { if let s = $0 { model.setSource(s) } })
        List(selection: selection) {
            Section("Library") {
                row(.all, count: model.library.items.count)
            }
            Section("Folders") {
                if let folder = model.library.folder {
                    Text(folder.lastPathComponent).font(.system(size: 12, weight: .medium)).lineLimit(1)
                        .help(folder.path)
                    ForEach(model.library.subfolders, id: \.self) { sub in
                        Button { model.openFolder(sub) } label: {
                            Text(sub.lastPathComponent).font(.system(size: 12)).foregroundStyle(.secondary)
                                .padding(.leading, 10).lineLimit(1)
                        }
                        .buttonStyle(.plain)
                    }
                } else if model.library.items.isEmpty {
                    Text("No folder open").font(.system(size: 12)).foregroundStyle(.tertiary)
                } else {
                    Text(model.library.title).font(.system(size: 12, weight: .medium))
                }
                ForEach(model.recentFolders.filter { $0 != model.library.folder }.prefix(5), id: \.self) { url in
                    Button { model.openFolder(url) } label: {
                        Text(url.lastPathComponent).font(.system(size: 12)).foregroundStyle(.secondary).lineLimit(1)
                    }
                    .buttonStyle(.plain)
                    .help(url.path)
                }
            }
            Section("Albums") {
                row(.basket, count: model.counts.basket, swatch: Theme.basket)
            }
            Section("Smart Albums") {
                row(.decision(.keep), count: model.counts.keep, swatch: Theme.keep)
                row(.decision(.undecided), count: model.counts.undecided)
                row(.decision(.reject), count: model.counts.reject, swatch: Theme.reject)
                ForEach([UInt8(6), 7, 8, 9], id: \.self) { m in
                    row(.mark(m), count: nil, swatch: MarkStyle.color(m))
                }
            }
        }
        .listStyle(.sidebar)
        .scrollContentBackground(.hidden)
        .background(Color(nsColor: Theme.windowBackground))
    }

    private func row(_ source: LibrarySource, count: Int?, swatch: NSColor? = nil) -> some View {
        HStack(spacing: 8) {
            RoundedRectangle(cornerRadius: 2)
                .fill(swatch.map { Color(nsColor: $0) } ?? Color.clear)
                .frame(width: 8, height: 8)
            Text(source.title).font(.system(size: 12)).lineLimit(1)
            Spacer()
            if let count {
                Text(count.formatted()).font(.system(size: 11).monospacedDigit()).foregroundStyle(.secondary)
            }
        }
        .tag(source)
    }
}
