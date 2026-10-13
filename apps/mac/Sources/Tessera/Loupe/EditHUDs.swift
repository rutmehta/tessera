import SwiftUI
import TesseraCore

/// The HUDs over the photo in Edit (SPEC §3, WP UX-13): the Masks chips and the crop aspect chips
/// top centre, the dial beside the cursor. Everything else is click-through to the loupe.
struct EditPhotoHUDs: View {
    let model: AppModel
    private var dial: PhotoDial { .shared }
    private var tools: DevelopTools { .shared }
    private var masks: MaskTools { .shared }

    var body: some View {
        GeometryReader { area in
            ZStack(alignment: .top) {
                if tools.cropActive {
                    AspectChipsHUD(tools: tools)
                        .padding(.top, Theme.Space.l)
                        .transition(Theme.Motion.transition(from: .top))
                } else if dial.masksToolShown {
                    MaskChipsHUD(dial: dial)
                        // Under the mask toolbar when M's brush and gradient tools are up.
                        .padding(.top, masks.active ? Theme.Height.sectionHeader + Theme.Height.large + Theme.Space.m : Theme.Space.l)
                        .transition(Theme.Motion.transition(from: .top))
                }
                if dial.isOpen {
                    DialHUD(dial: dial)
                        .position(Self.dialCenter(anchor: dial.anchor, in: area.size))
                        .transition(.opacity)
                }
            }
            .frame(width: area.size.width, height: area.size.height, alignment: .top)
            .animation(Theme.Motion.appear, value: dial.masksToolShown)
            .animation(Theme.Motion.appear, value: tools.cropActive)
            .animation(Theme.Motion.appear, value: dial.isOpen)
        }
        .allowsHitTesting(dial.isOpen || dial.masksToolShown || tools.cropActive)
    }

    /// The dial's centre: beside the click, kept inside the canvas (the demo's placement).
    static func dialCenter(anchor: CGPoint, in size: CGSize) -> CGPoint {
        let w = Theme.Width.dial, h = DialHUD.estimatedHeight
        let x = min(anchor.x + Theme.Space.xxl + Theme.Space.s, size.width - w - Theme.Space.m)
        let y = min(max(anchor.y - Theme.Height.dialKnob, Theme.Space.m), size.height - h - Theme.Space.m)
        return CGPoint(x: max(x, Theme.Space.m) + w / 2, y: max(y, Theme.Space.m) + h / 2)
    }
}

/// A pill of chips over the photo: the HUD ground at 86 %, hairlineStrong, radius 999.
private struct HUDPill<Content: View>: View {
    @ViewBuilder var content: Content
    var body: some View {
        HStack(spacing: Theme.Space.xs) { content }
            .padding(Theme.Space.xs)
            .background(Capsule().fill(Theme.hud))
            .overlay(Capsule().strokeBorder(Theme.hairlineStrong, lineWidth: Theme.Space.hairline))
            .shadow(color: Theme.shadow.opacity(0.5), radius: Theme.Space.m, y: Theme.Space.xs)
    }
}

/// One chip of a HUD pill: selected in the accent (masks) or in ink (dial, aspects).
private struct HUDChip: View {
    let title: String
    let on: Bool
    var accent = false
    let action: () -> Void
    @State private var hovering = false

    var body: some View {
        Button(action: action) {
            Text(title)
                .font(on ? Theme.Fonts.labelSemibold : Theme.Fonts.label)
                .monospacedDigit()
                .foregroundStyle(on ? (accent ? Theme.textOnAccent : Theme.canvas) : Theme.textSecondary)
                .padding(.horizontal, Theme.Space.m)
                .frame(height: Theme.Height.regular)
                .background(Capsule().fill(on ? (accent ? Theme.accent : Theme.textPrimary) : hovering ? Theme.hover : Theme.clear))
                .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .onHover { hovering = $0 }
        .accessibilityAddTraits(on ? .isSelected : [])
    }
}

/// Subject · Sky · Background · Whole photo (the strip's Masks tool).
struct MaskChipsHUD: View {
    let dial: PhotoDial

    var body: some View {
        HUDPill {
            ForEach(DialTarget.allCases) { t in
                HUDChip(title: t.title, on: dial.target == t, accent: true) { dial.select(t) }
                    .help(t == .whole ? "Adjust the whole photo" : "Target the \(t.title.lowercased()) mask")
                    .accessibilityIdentifier("develop.maskchips.\(t.rawValue)")
            }
            if dial.segmenting {
                ProgressView().controlSize(.small).padding(.horizontal, Theme.Space.xs)
                    .accessibilityLabel("Finding the subject, sky and background")
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Masks")
    }
}

/// 3:2 · 4:5 · 1:1 · 16:9 · Free over the crop overlay.
struct AspectChipsHUD: View {
    let tools: DevelopTools
    private static let aspects: [(String, CropAspect)] = [
        ("3:2", .ratio(3, 2)), ("4:5", .ratio(4, 5)), ("1:1", .ratio(1, 1)), ("16:9", .ratio(16, 9)), ("Free", .free),
    ]

    var body: some View {
        HUDPill {
            ForEach(Self.aspects, id: \.0) { title, aspect in
                HUDChip(title: title, on: tools.cropAspect.id == aspect.id) { tools.setCropAspect(aspect) }
                    .help(aspect.id == CropAspect.free.id ? "Any proportions" : "Crop to \(title)")
                    .accessibilityIdentifier("develop.crop.aspect.\(title)")
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Crop aspect")
    }
}

/// The dial: title and value, a 120 pt knob (drag up or down; the conic fill is the value), chips to
/// switch the control, and the line that says what it applies to.
struct DialHUD: View {
    let dial: PhotoDial
    @State private var pressed = false

    /// The HUD's height for placement (title row, knob, two chip rows, the note, the paddings).
    static let estimatedHeight: CGFloat = Theme.Space.m * 2 + Theme.Height.large + Theme.Height.dialKnob
        + Theme.Height.regular * 2 + Theme.Height.small + Theme.Space.m * 4

    private var controls: [DialControl] {
        DialControl.allCases.filter { $0 != .skin || dial.faceUnderCursor }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.m) {
            HStack(alignment: .firstTextBaseline) {
                Text(dial.control.title).font(Theme.Fonts.bodyMedium).foregroundStyle(Theme.textPrimary)
                Spacer()
                Text(dial.formatted)
                    .font(Theme.Fonts.headline).monospacedDigit()
                    .foregroundStyle(Theme.accent)
                    .accessibilityIdentifier("develop.dial.value")
            }
            knob.frame(maxWidth: .infinity)
            FlowRow(spacing: Theme.Space.xs + Theme.Space.hairline) {
                ForEach(controls) { c in
                    HUDChip(title: c.title, on: dial.control == c) { dial.choose(c) }
                        .accessibilityIdentifier("develop.dial.\(c.rawValue)")
                }
            }
            Text(dial.segmenting ? "Finding the subject, sky and background…" : dial.target.note)
                .font(Theme.Fonts.caption).foregroundStyle(Theme.textTertiary)
                .accessibilityIdentifier("develop.dial.note")
        }
        .padding(Theme.Space.m + Theme.Space.xxs)
        .frame(width: Theme.Width.dial)
        .background(RoundedRectangle(cornerRadius: Theme.Radius.hud).fill(Theme.hud))
        .overlay(RoundedRectangle(cornerRadius: Theme.Radius.hud).strokeBorder(Theme.hairlineStrong, lineWidth: Theme.Space.hairline))
        .shadow(color: Theme.shadow.opacity(0.6), radius: Theme.Space.xl, y: Theme.Space.l)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Dial: \(dial.control.title) on \(dial.target.title)")
        .accessibilityIdentifier("develop.dial")
    }

    private var knob: some View {
        let range = dial.range
        let fraction = (dial.value - range.lowerBound) / max(range.upperBound - range.lowerBound, 1e-9)
        return ZStack {
            Circle()
                .fill(AngularGradient(stops: [
                    .init(color: Theme.accent, location: 0),
                    .init(color: Theme.accent, location: fraction),
                    .init(color: Theme.hairlineStrong, location: fraction),
                    .init(color: Theme.hairlineStrong, location: 1),
                ], center: .center, startAngle: .degrees(-90), endAngle: .degrees(270)))
            Circle().fill(Theme.panel).frame(width: Theme.Height.dialKnobInner, height: Theme.Height.dialKnobInner)
            Text("drag up\nor down")
                .font(Theme.Fonts.caption).multilineTextAlignment(.center)
                .foregroundStyle(pressed ? Theme.textSecondary : Theme.textTertiary)
        }
        .frame(width: Theme.Height.dialKnob, height: Theme.Height.dialKnob)
        .contentShape(Circle())
        .gesture(DragGesture(minimumDistance: 0)
            .onChanged { g in
                pressed = true
                dial.drag(delta: g.startLocation.y - g.location.y, final: false)
            }
            .onEnded { g in
                pressed = false
                dial.drag(delta: g.startLocation.y - g.location.y, final: true)
            })
        .onHover { NSCursor.resizeUpDown.set(); if !$0 { NSCursor.arrow.set() } }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("\(dial.control.title) knob")
        .accessibilityValue(dial.formatted)
        .accessibilityIdentifier("develop.dial.knob")
    }
}
