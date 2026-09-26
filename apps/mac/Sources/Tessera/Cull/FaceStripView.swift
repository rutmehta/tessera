import AppKit
import SwiftUI
import TesseraCore

/// Face strip under the loupe (docs/06 §3): every detected face as a close-up with a focus badge
/// (green / yellow / red) and an eyes badge; click a face to zoom in, and to filter the shoot by
/// that person. Faces come from `Engine.analyzeImage` (Cull ▸ Analyze Faces).
struct FaceStrip: View {
    let model: AppModel
    private var assist: AssistController { model.assist }

    var body: some View {
        VStack(spacing: 0) {
            Hairline()
            HStack(spacing: Theme.Space.s) {
                Text("Faces").font(Theme.Fonts.captionMedium).foregroundStyle(Theme.textSecondary)
                ScrollView(.horizontal) {
                    HStack(spacing: Theme.Space.s) {
                        ForEach(assist.faces) { face in FaceChipButton(model: model, face: face) }
                    }
                }
                .scrollIndicators(.never)
                Spacer(minLength: Theme.Space.s)
                FaceLegend()
            }
            .padding(.horizontal, Theme.Space.gutter)
            .frame(height: Theme.Height.filmstrip)
        }
        .background(Theme.panel)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("face-strip")
    }
}

/// The legend keeps colour from being the only signal.
private struct FaceLegend: View {
    var body: some View {
        HStack(spacing: Theme.Space.s) {
            legend(Theme.keep, "Sharp")
            legend(Theme.warning, "Soft")
            legend(Theme.reject, "Missed")
            Label("Eyes", systemImage: "eye").labelStyle(.titleAndIcon)
        }
        .font(Theme.Fonts.caption)
        .foregroundStyle(Theme.textTertiary)
    }

    private func legend(_ color: Color, _ title: String) -> some View {
        HStack(spacing: Theme.Space.xs) {
            Circle().fill(color).frame(width: Theme.Space.s - Theme.Space.xxs, height: Theme.Space.s - Theme.Space.xxs)
            Text(title)
        }
    }
}

extension FaceChip.Level {
    var color: Color {
        switch self {
        case .good: Theme.keep
        case .fair: Theme.warning
        case .poor: Theme.reject
        case .unknown: Theme.textTertiary
        }
    }
    var focusWord: String {
        switch self {
        case .good: "sharp"
        case .fair: "soft"
        case .poor: "missed focus"
        case .unknown: "unknown"
        }
    }
    var eyesWord: String {
        switch self {
        case .good: "eyes open"
        case .fair: "eyes unsure"
        case .poor: "eyes closed"
        case .unknown: "eyes unknown"
        }
    }
    var eyesSymbol: String {
        switch self {
        case .good: "eye"
        case .fair: "eye.trianglebadge.exclamationmark"
        case .poor: "eye.slash"
        case .unknown: "questionmark"
        }
    }
}

/// Crops a face out of the displayed preview (normalized rect, origin top-left), with margin.
@MainActor
func faceCrop(_ image: CGImage?, _ rect: CGRect, margin: Double = 0.25) -> CGImage? {
    guard let image else { return nil }
    let w = Double(image.width), h = Double(image.height)
    var r = CGRect(x: rect.minX * w, y: rect.minY * h, width: rect.width * w, height: rect.height * h)
    let side = max(r.width, r.height) * (1 + margin)
    r = CGRect(x: r.midX - side / 2, y: r.midY - side / 2, width: side, height: side)
        .intersection(CGRect(x: 0, y: 0, width: w, height: h)).integral
    guard r.width >= 2, r.height >= 2 else { return nil }
    return image.cropping(to: r)
}

private struct FaceChipButton: View {
    let model: AppModel
    let face: FaceChip
    @State private var zoomed = false
    @State private var hovering = false
    private let size: CGFloat = Theme.Height.filmstrip - Theme.Space.l - Theme.Space.xs

    /// The person's name when known (People view), otherwise "Unnamed person" / "Face n".
    private var label: String {
        if let tile = model.people.person(face.personID) { return tile.isNamed ? tile.displayName : "Unnamed person" }
        return "Face \(face.ordinal + 1)"
    }

    var body: some View {
        let person = model.assist.person(face.personID)
        Button { zoomed = true } label: {
            ZStack(alignment: .bottom) {
                FaceImage(model: model, face: face)
                    .frame(width: size, height: size)
                    .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.chip))
                HStack(spacing: 0) {
                    Circle().fill(face.focusLevel.color)
                        .frame(width: Theme.Space.s, height: Theme.Space.s)
                        .overlay(Circle().strokeBorder(Theme.shadow, lineWidth: Theme.Space.hairline))
                    Spacer(minLength: 0)
                    Image(systemName: face.eyesLevel.eyesSymbol)
                        .font(Theme.Fonts.iconSmall)
                        .foregroundStyle(face.eyesLevel.color)
                }
                .padding(.horizontal, Theme.Space.xs)
                .padding(.vertical, Theme.Space.xxs)
                .background(Theme.hud)
            }
            .frame(width: size, height: size)
            .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.chip))
            .overlay(RoundedRectangle(cornerRadius: Theme.Radius.chip)
                .strokeBorder(hovering ? Theme.accent : Theme.hairlineStrong, lineWidth: Theme.Space.hairline))
        }
        .buttonStyle(.plain)
        .onHover { hovering = $0 }
        .help("\(label): \(face.focusLevel.focusWord), \(face.eyesLevel.eyesWord). Click to zoom"
              + (model.people.person(face.personID) != nil ? "; right-click to name." : "."))
        .accessibilityLabel("\(label), \(face.focusLevel.focusWord), \(face.eyesLevel.eyesWord)")
        .accessibilityIdentifier("face-chip-\(face.ordinal)")
        .contextMenu {
            if let tile = model.people.person(face.personID) {
                Button(tile.isNamed ? "Rename \(tile.displayName)…" : "Name…") { promptPersonName(model: model, person: tile) }
                    .accessibilityIdentifier("face-name-person")
                Button("Show in People") {
                    model.setSource(.people)
                    model.people.openDetail(tile.id)
                }
            } else {
                Text("Not grouped into a person yet (Cull ▸ Analyze Faces)")
            }
        }
        .popover(isPresented: $zoomed, arrowEdge: .top) {
            FaceZoom(model: model, face: face, person: person) { zoomed = false }
        }
    }
}

private struct FaceImage: View {
    let model: AppModel
    let face: FaceChip
    var margin = 0.25
    var body: some View {
        if let item = model.focusedItem,
           let crop = faceCrop(model.loader.cached(item, tier: .preview) ?? model.loader.cached(item, tier: .thumbnail),
                               face.rect, margin: margin) {
            Image(decorative: crop, scale: 1).resizable().aspectRatio(contentMode: .fill)
        } else {
            Theme.well
        }
    }
}

/// Click-to-zoom: the face large, its measurements, and the per-person filters.
private struct FaceZoom: View {
    let model: AppModel
    let face: FaceChip
    let person: PersonSummary?
    let close: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            FaceImage(model: model, face: face, margin: 0.6)
                .frame(width: 240, height: 240)
                .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.card))
            HStack(spacing: Theme.Space.xs) {
                Text(model.people.person(face.personID).map { $0.isNamed ? $0.displayName : "Unnamed person" } ?? person?.name ?? "Face \(face.ordinal + 1)")
                    .font(Theme.Fonts.labelSemibold).foregroundStyle(Theme.textPrimary)
                if let person { Text("in \(person.items.count) frame\(person.items.count == 1 ? "" : "s")").font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary) }
            }
            HStack(spacing: Theme.Space.xs) {
                Chip(text: String(format: "Focus %.2f · %@", face.focus, face.focusLevel.focusWord),
                     color: face.focusLevel.color, style: .outlined)
                Chip(text: face.eyesOpen.map { String(format: "Eyes %.2f · %@", $0, face.eyesLevel.eyesWord) } ?? "Eyes unknown",
                     color: face.eyesLevel.color, style: .outlined)
            }
            Hint("Eyes is a landmark-geometry proxy, not a verified blink detector.")
            if let person {
                HStack(spacing: Theme.Space.xs) {
                    Button("Frames with \(person.name)") { model.assist.filter(person: person, eyesClosed: false); close() }
                        .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                        .accessibilityIdentifier("face-filter-person")
                    Button("…with eyes closed") { model.assist.filter(person: person, eyesClosed: true); close() }
                        .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                        .accessibilityIdentifier("face-filter-eyes-closed")
                }
            }
        }
        .padding(Theme.Space.m)
        .background(Theme.panel)
        .accessibilityIdentifier("face-zoom")
    }
}
