import AppKit
import SwiftUI
import TesseraCore

/// Sidebar ▸ People (docs/01 §1.4, WP M2-40, M2-44): a grid of person tiles (the medoid face, the
/// name or an inline name field, counts and a confirmed badge), named people first. Click
/// selects (⌘ / ⇧ extend) for the toolbar's Merge; double-click opens the person's faces, where
/// faces are confirmed, split off, or dragged onto another person. Every edit goes through the
/// engine (`CullSession` people calls) and the tiles reload from it; Edit ▸ Undo / Redo replay the
/// engine's people history while this view is frontmost.
struct PeopleView: View {
    let model: AppModel
    private var people: PeopleModel { model.people }

    var body: some View {
        Group {
            if let person = people.detail {
                PersonDetailView(model: model, person: person)
            } else {
                PeopleGrid(model: model)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(Theme.canvas)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("people-view")
    }
}

// MARK: - Grid

private struct PeopleGrid: View {
    let model: AppModel
    private var people: PeopleModel { model.people }
    private let columns = [GridItem(.adaptive(minimum: PeopleMetrics.tile, maximum: PeopleMetrics.tile),
                                    spacing: Theme.Space.l, alignment: .top)]

    var body: some View {
        VStack(spacing: 0) {
            PeopleHeader(model: model)
            if people.tiles.isEmpty {
                EmptyStateContent(symbol: "person.2",
                                  title: people.isRefreshing ? "Finding people…" : "No people yet",
                                  message: "Cull ▸ Analyze Faces finds faces and groups them into people.\nName a group to find that person everywhere.") {
                    Button("Analyze Faces") { model.assist.analyze(faces: true, force: false, title: "Finding faces") }
                        .buttonStyle(.theme(.primary, height: Theme.Height.large))
                        .disabled(!model.isEngineBacked || model.assist.isRunning)
                        .accessibilityIdentifier("people-analyze-faces")
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ScrollView {
                    LazyVGrid(columns: columns, alignment: .leading, spacing: Theme.Space.l) {
                        ForEach(people.tiles) { tile in PersonTileView(model: model, tile: tile) }
                    }
                    .padding(Theme.Space.gutter)
                }
                .contentShape(Rectangle())
                .onTapGesture { people.selection = [] }
                .accessibilityIdentifier("people-grid")
            }
            if let note = people.approximateNote {
                Hairline()
                HStack(spacing: Theme.Space.xs) {
                    Image(systemName: "info.circle").font(Theme.Fonts.iconSmall)
                    Text(note)
                    Spacer(minLength: 0)
                }
                .font(Theme.Fonts.caption)
                .foregroundStyle(Theme.textTertiary)
                .padding(.horizontal, Theme.Space.gutter)
                .frame(height: Theme.Height.regular)
                .background(Theme.panel)
                .help("Too many faces to cluster at once: clusters were fitted on a sample and the rest matched to them. Rare people can stay unnamed singletons; Refit re-runs it.")
                .accessibilityIdentifier("people-approximate-note")
            }
        }
    }
}

enum PeopleMetrics {
    /// Tile width: the face plus its caption.
    static let tile: CGFloat = 176
    /// Face chips in the detail view.
    static let chip: CGFloat = 96
    /// "Move to" column in the detail view.
    static let column: CGFloat = Theme.Width.sidebarIdeal
    /// Face crop margin around the detector box.
    static let margin = 0.35
}

/// 32 pt bar above the grid: title and counts, selection, refresh.
private struct PeopleHeader: View {
    let model: AppModel
    private var people: PeopleModel { model.people }

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: Theme.Space.s) {
                Text("People").font(Theme.Fonts.labelSemibold).foregroundStyle(Theme.textPrimary)
                let named = people.named.count
                Text("\(people.tiles.count.formatted()) · \(named.formatted()) named")
                    .font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textTertiary)
                    .accessibilityIdentifier("people-count")
                if people.isRefreshing {
                    ProgressView().controlSize(.small).accessibilityIdentifier("people-refreshing")
                }
                Spacer(minLength: Theme.Space.s)
                if people.selection.count > 1 {
                    Text("\(people.selection.count) selected").font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.accent)
                }
                Button("Refit") { Task { await people.refresh(force: true); model.peopleDidChange() } }
                    .buttonStyle(.theme(.borderless, height: Theme.Height.small))
                    .disabled(people.isRefreshing || !model.isEngineBacked)
                    .help("Re-cluster every face now. Named people and confirmed faces are kept.")
                    .accessibilityIdentifier("people-refit")
            }
            .padding(.horizontal, Theme.Space.gutter)
            .frame(height: Theme.Height.sectionHeader)
            Hairline()
        }
        .background(Theme.panel)
    }
}

/// One person: the representative face, the name (or a name field), counts, confirmed badge.
private struct PersonTileView: View {
    let model: AppModel
    let tile: PersonTile
    @State private var draft = ""
    @State private var targeted = false
    @State private var hovering = false
    private var people: PeopleModel { model.people }

    var body: some View {
        let selected = people.selection.contains(tile.id)
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            FaceCropView(model: model, face: tile.cover)
                .frame(width: PeopleMetrics.tile - Theme.Space.s * 2, height: PeopleMetrics.tile - Theme.Space.s * 2)
                .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.chip))
                .overlay(RoundedRectangle(cornerRadius: Theme.Radius.chip)
                    .strokeBorder(targeted || hovering ? Theme.accent : Theme.hairline, lineWidth: Theme.Space.hairline))
            nameRow
            HStack(spacing: Theme.Space.xs) {
                Text("\(tile.items.count.formatted()) photo\(tile.items.count == 1 ? "" : "s")")
                    .font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textTertiary)
                Spacer(minLength: 0)
                if tile.isConfirmed {
                    Chip(text: "Confirmed", color: Theme.keep, style: .outlined, height: Theme.Height.chip)
                        .help("Every face is confirmed: automatic re-clustering will not move them")
                        .accessibilityIdentifier("person-confirmed-\(tile.id)")
                } else if tile.confirmedCount > 0 {
                    Text("\(tile.confirmedCount)/\(tile.faces) confirmed")
                        .font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textTertiary)
                }
            }
        }
        .padding(Theme.Space.s)
        .frame(width: PeopleMetrics.tile, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: Theme.Radius.control)
            .fill(selected ? Theme.accentSubtle : targeted ? Theme.hover : Theme.clear))
        .overlay(RoundedRectangle(cornerRadius: Theme.Radius.control)
            .strokeBorder(targeted ? Theme.accent : Theme.clear, lineWidth: Theme.Space.xxs))
        .contentShape(RoundedRectangle(cornerRadius: Theme.Radius.control))
        .onHover { hovering = $0 }
        .onTapGesture(count: 2) { people.openDetail(tile.id) }
        .onTapGesture {
            let mods = NSEvent.modifierFlags
            people.click(tile.id, command: mods.contains(.command), shift: mods.contains(.shift))
        }
        .dropDestination(for: String.self) { tokens, _ in
            let faces = tokens.compactMap(PersonFaceRef.init(dragToken:))
            var moved = false
            for face in faces where people.reassign(face, to: tile.id) { moved = true }
            model.peopleDidChange()
            return moved
        } isTargeted: { targeted = $0 }
        .contextMenu { PersonMenu(model: model, tile: tile) }
        .help(tile.isNamed ? "\(tile.displayName): \(tile.faces) faces. Double-click to see them." :
              "Unnamed: \(tile.faces) faces. Type a name and press Return, or double-click to see them.")
        .accessibilityElement(children: .contain)
        .accessibilityLabel("\(tile.displayName), \(tile.items.count) photos\(tile.isConfirmed ? ", confirmed" : "")")
        .accessibilityAddTraits(selected ? .isSelected : [])
        .accessibilityIdentifier("person-tile-\(tile.id)")
    }

    @ViewBuilder private var nameRow: some View {
        if let name = tile.name {
            Text(name).font(Theme.Fonts.labelMedium).foregroundStyle(Theme.textPrimary).lineLimit(1)
                .frame(height: Theme.Height.small)
                .accessibilityIdentifier("person-name-\(tile.id)")
        } else {
            HStack(spacing: Theme.Space.xxs) {
                TextField("Unnamed", text: $draft)
                    .textFieldStyle(.roundedBorder)
                    .controlSize(.small)
                    .font(Theme.Fonts.caption)
                    .onSubmit {
                        if people.name(tile.id, as: draft) { draft = "" }
                        model.peopleDidChange()
                    }
                    .help("Type a name and press Return")
                    .accessibilityIdentifier("person-name-field-\(tile.id)")
                let suggestions = people.suggestions[tile.id] ?? []
                if !suggestions.isEmpty {
                    Menu {
                        Section("Looks like") {
                            ForEach(suggestions, id: \.self) { s in
                                Button("\(s.name)    \(Int((s.similarity * 100).rounded())) % similar") {
                                    people.accept(s)
                                    model.peopleDidChange()
                                }
                            }
                        }
                    } label: {
                        Image(systemName: "person.crop.circle.badge.questionmark").font(Theme.Fonts.iconSmall)
                    }
                    .menuStyle(IconMenuStyle())
                    .help("Name suggestions: choosing one merges these faces into that person")
                    .accessibilityIdentifier("person-suggestions-\(tile.id)")
                }
            }
            .frame(height: Theme.Height.small)
        }
    }
}

/// Tile context menu.
private struct PersonMenu: View {
    let model: AppModel
    let tile: PersonTile
    private var people: PeopleModel { model.people }

    var body: some View {
        Button("Open") { people.openDetail(tile.id) }
        Button("Show Photos") { model.showPhotos(of: tile.id) }
        Divider()
        Button(tile.isNamed ? "Rename…" : "Name…") { promptPersonName(model: model, person: tile) }
        if tile.isNamed {
            Button("Clear Name") { people.name(tile.id, as: ""); model.peopleDidChange() }
        }
        if people.selection.count > 1, people.selection.contains(tile.id) {
            Divider()
            Button("Merge \(people.selection.count) People") { people.mergeSelection(); model.peopleDidChange() }
        }
    }
}

/// Name… / Rename… (tile menu and the loupe face strip): a small sheet with a field.
@MainActor
func promptPersonName(model: AppModel, person: PersonTile) {
    model.collections.promptName(title: person.isNamed ? "Rename \(person.displayName)" : "Name This Person",
                                 message: model.people.naming.writeFaceRegions
                                     ? "The name is saved in the library and written to the photos' XMP face regions."
                                     : "The name is saved in the library (XMP face regions are off in Settings ▸ Library).",
                                 initial: person.name ?? "", confirm: "Name") { name in
        model.people.name(person.id, as: name)
        model.peopleDidChange()
    }
}

// MARK: - Detail

/// A person's faces: confirm / unconfirm, select and Split, drag onto another person.
private struct PersonDetailView: View {
    let model: AppModel
    let person: PersonTile
    @State private var draft = ""
    private var people: PeopleModel { model.people }
    private let columns = [GridItem(.adaptive(minimum: PeopleMetrics.chip, maximum: PeopleMetrics.chip + Theme.Space.l),
                                    spacing: Theme.Space.s, alignment: .top)]

    var body: some View {
        VStack(spacing: 0) {
            header
            Hairline()
            HStack(spacing: 0) {
                VStack(alignment: .leading, spacing: 0) {
                    ScrollView {
                        LazyVGrid(columns: columns, alignment: .leading, spacing: Theme.Space.s) {
                            ForEach(person.members) { member in FaceMemberChip(model: model, person: person, member: member) }
                        }
                        .padding(Theme.Space.gutter)
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                    .accessibilityIdentifier("person-faces")
                    Hairline()
                    Hint("Click faces to select them, then Split. Drag a face onto a person on the right to move it. "
                         + "Confirmed faces stay with this person when faces are re-clustered.")
                        .padding(.horizontal, Theme.Space.gutter)
                        .padding(.vertical, Theme.Space.s)
                }
                Hairline(vertical: true)
                MoveTargets(model: model, person: person)
                    .frame(width: PeopleMetrics.column)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .onAppear { draft = person.name ?? "" }
        .onChange(of: person.name) { draft = person.name ?? "" }
        .onExitCommand { people.closeDetail() }
    }

    private var header: some View {
        HStack(spacing: Theme.Space.s) {
            IconButton(symbol: "chevron.left", help: "All people (Esc)", size: Theme.Height.small) { people.closeDetail() }
                .accessibilityIdentifier("person-detail-back")
            TextField("Unnamed", text: $draft)
                .textFieldStyle(.roundedBorder)
                .controlSize(.small)
                .font(Theme.Fonts.label)
                .frame(width: Theme.Width.sidebarIdeal)
                .onSubmit { people.name(person.id, as: draft); model.peopleDidChange() }
                .help("Type a name and press Return; clear it to remove the name")
                .accessibilityIdentifier("person-detail-name")
            Text("\(person.items.count.formatted()) photos · \(person.faces.formatted()) faces · \(person.confirmedCount.formatted()) confirmed")
                .font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textTertiary)
                .accessibilityIdentifier("person-detail-counts")
            Spacer(minLength: Theme.Space.s)
            if !people.faceSelection.isEmpty {
                Text("\(people.faceSelection.count) selected").font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.accent)
            }
            Button("Show Photos") { model.showPhotos(of: person.id) }
                .buttonStyle(.theme(.borderless, height: Theme.Height.small))
                .accessibilityIdentifier("person-show-photos")
            Button("Confirm All") { people.confirmAll(); model.peopleDidChange() }
                .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                .disabled(person.isConfirmed)
                .help("Confirm every face of this person (protects them from automatic re-clustering)")
                .accessibilityIdentifier("person-confirm-all")
            Button("Split") { people.splitSelection(); model.peopleDidChange() }
                .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                .disabled(people.faceSelection.isEmpty || people.faceSelection.count >= person.members.count)
                .help("Move the selected faces into a new, unnamed person")
                .accessibilityIdentifier("person-split")
        }
        .padding(.horizontal, Theme.Space.gutter)
        .frame(height: Theme.Height.sectionHeader)
        .background(Theme.panel)
    }
}

/// One face of the detail person: select (click), confirm toggle, drag source.
private struct FaceMemberChip: View {
    let model: AppModel
    let person: PersonTile
    let member: PersonTile.Member
    private var people: PeopleModel { model.people }

    var body: some View {
        let selected = people.faceSelection.contains(member.face)
        let name = model.library.items.indices.contains(member.face.item) ? model.library.items[member.face.item].name : ""
        VStack(alignment: .leading, spacing: Theme.Space.xxs) {
            ZStack(alignment: .bottomTrailing) {
                FaceCropView(model: model, face: member.face)
                    .frame(width: PeopleMetrics.chip, height: PeopleMetrics.chip)
                    .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.chip))
                    .overlay(RoundedRectangle(cornerRadius: Theme.Radius.chip)
                        .strokeBorder(selected ? Theme.accent : Theme.hairline,
                                      lineWidth: selected ? Theme.Space.xxs : Theme.Space.hairline))
                Button {
                    people.setConfirmed(member.face, !member.confirmed)
                    model.peopleDidChange()
                } label: {
                    Image(systemName: member.confirmed ? "checkmark.seal.fill" : "checkmark.seal")
                        .font(Theme.Fonts.iconSmall)
                        .foregroundStyle(member.confirmed ? Theme.keep : Theme.textSecondary)
                        .frame(width: Theme.Height.small, height: Theme.Height.small)
                        .background(RoundedRectangle(cornerRadius: Theme.Radius.chip).fill(Theme.hud))
                }
                .buttonStyle(.plain)
                .padding(Theme.Space.xs)
                .help(member.confirmed ? "Confirmed: click to unconfirm" : "Unconfirmed: click to confirm this is \(person.displayName)")
                .accessibilityLabel(member.confirmed ? "Unconfirm" : "Confirm")
                .accessibilityValue(member.confirmed ? "confirmed" : "unconfirmed")
                .accessibilityIdentifier("face-confirm-\(member.face.item)-\(member.face.ordinal)")
            }
            Text(name).font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                .lineLimit(1).truncationMode(.middle)
                .frame(width: PeopleMetrics.chip, alignment: .leading)
        }
        .padding(Theme.Space.xxs)
        .background(RoundedRectangle(cornerRadius: Theme.Radius.control).fill(selected ? Theme.accentSubtle : Theme.clear))
        .contentShape(Rectangle())
        .onTapGesture { people.toggleFace(member.face) }
        .draggable(member.face.dragToken) {
            FaceCropView(model: model, face: member.face)
                .frame(width: Theme.Height.filmstrip, height: Theme.Height.filmstrip)
                .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.chip))
        }
        .contextMenu {
            Button(member.confirmed ? "Unconfirm" : "Confirm") {
                people.setConfirmed(member.face, !member.confirmed); model.peopleDidChange()
            }
            Menu("Move To") {
                ForEach(people.tiles.filter { $0.id != person.id }) { other in
                    Button(other.displayName + (other.isNamed ? "" : " · \(other.faces) faces")) {
                        people.reassign(member.face, to: other.id); model.peopleDidChange()
                    }
                }
            }
            Button("Split Off") {
                people.faceSelection = [member.face]
                people.splitSelection()
                model.peopleDidChange()
            }
            .disabled(person.members.count < 2)
            Divider()
            Button("Show in Loupe") { model.setSource(.all); model.showInLoupe(member.face.item) }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("\(name), face \(member.face.ordinal + 1)\(member.confirmed ? ", confirmed" : "")")
        .accessibilityAddTraits(selected ? .isSelected : [])
        .accessibilityIdentifier("face-member-\(member.face.item)-\(member.face.ordinal)")
    }
}

/// Other people as drop targets (drag a face chip onto one to reassign it).
private struct MoveTargets: View {
    let model: AppModel
    let person: PersonTile

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text("Move to").font(Theme.Fonts.captionMedium).foregroundStyle(Theme.textSecondary)
                .padding(.horizontal, Theme.Space.gutter)
                .frame(height: Theme.Height.regular)
            ScrollView {
                VStack(alignment: .leading, spacing: Theme.Space.xxs) {
                    ForEach(model.people.tiles.filter { $0.id != person.id }) { other in
                        MoveTargetRow(model: model, tile: other)
                    }
                }
                .padding(.horizontal, Theme.Space.s)
            }
        }
        .background(Theme.panel)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("person-move-targets")
    }
}

private struct MoveTargetRow: View {
    let model: AppModel
    let tile: PersonTile
    @State private var targeted = false

    var body: some View {
        HStack(spacing: Theme.Space.s) {
            FaceCropView(model: model, face: tile.cover)
                .frame(width: Theme.Height.regular, height: Theme.Height.regular)
                .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.chip))
            Text(tile.displayName).font(Theme.Fonts.label)
                .foregroundStyle(tile.isNamed ? Theme.textPrimary : Theme.textSecondary).lineLimit(1)
            Spacer(minLength: 0)
            Text(tile.faces.formatted()).font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textTertiary)
        }
        .padding(.horizontal, Theme.Space.xs)
        .frame(height: Theme.Height.large)
        .background(RoundedRectangle(cornerRadius: Theme.Radius.control).fill(targeted ? Theme.accentSubtle : Theme.clear))
        .overlay(RoundedRectangle(cornerRadius: Theme.Radius.control)
            .strokeBorder(targeted ? Theme.accent : Theme.clear, lineWidth: Theme.Space.hairline))
        .contentShape(Rectangle())
        .dropDestination(for: String.self) { tokens, _ in
            var moved = false
            for face in tokens.compactMap(PersonFaceRef.init(dragToken:)) where model.people.reassign(face, to: tile.id) {
                moved = true
            }
            model.peopleDidChange()
            return moved
        } isTargeted: { targeted = $0 }
        .onTapGesture(count: 2) { model.people.openDetail(tile.id) }
        .help("Drop a face here to move it to \(tile.displayName). Double-click to open.")
        .accessibilityIdentifier("person-move-target-\(tile.id)")
    }
}

// MARK: - Face crops

/// A face cut out of its photo's thumbnail (loaded on demand, cached per image).
struct FaceCropView: View {
    let model: AppModel
    let face: PersonFaceRef?

    var body: some View {
        let item = face.flatMap { f in model.library.items.indices.contains(f.item) ? model.library.items[f.item] : nil }
        let image = item.flatMap { model.faceThumbnails.image(for: $0) }
        Group {
            if let face, let image, let rect = model.people.faceRect(face),
               let crop = faceCrop(image, rect, margin: PeopleMetrics.margin) {
                Image(decorative: crop, scale: 1).resizable().aspectRatio(contentMode: .fill)
            } else {
                ZStack {
                    Theme.well
                    Image(systemName: "person.fill").font(Theme.Fonts.icon).foregroundStyle(Theme.textTertiary)
                }
            }
        }
        .task(id: item.map(FaceThumbnails.key)) {
            if let item { model.faceThumbnails.load(item, loader: model.loader) }
        }
    }
}

/// Thumbnails for face crops, keyed by engine image id (item ids move on in-place updates).
@MainActor @Observable
final class FaceThumbnails {
    private var images: [String: CGImage] = [:]
    @ObservationIgnored private var pending: Set<String> = []

    nonisolated static func key(_ item: PhotoItem) -> String { item.engineImage?.imageID ?? "item:\(item.id)" }

    func image(for item: PhotoItem) -> CGImage? { images[Self.key(item)] }

    func load(_ item: PhotoItem, loader: ThumbnailLoader) {
        let key = Self.key(item)
        guard images[key] == nil, pending.insert(key).inserted else { return }
        _ = loader.request(item, tier: .thumbnail) { [weak self] image in
            self?.images[key] = image
            self?.pending.remove(key)
        }
    }

    func removeAll() {
        images = [:]
        pending = []
    }
}
