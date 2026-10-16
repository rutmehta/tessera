import SwiftUI
import TesseraCore

/// The agent's triage of the shoot (redesign SPEC §4, WP UX-14): the left column of Cull. Four
/// rows, each a filter on the view, filling in while the analysis pass runs; the person's own
/// decisions; and progress through the bursts. Tab hides it, like every mode's panel.
struct TriageColumn: View {
    let model: AppModel
    private var assist: AssistController { model.assist }
    private var triage: CullTriage { assist.triage }

    var body: some View {
        let _ = model.decisionRevision
        VStack(alignment: .leading, spacing: Theme.Space.l) {
            HStack(alignment: .firstTextBaseline) {
                Text("Shoot").font(Theme.Fonts.title).foregroundStyle(Theme.textPrimary)
                Spacer()
                Text("\(model.library.items.count.formatted()) · \(triage.burstCount.formatted()) burst\(triage.burstCount == 1 ? "" : "s")")
                    .font(Theme.Fonts.labelNumeric).foregroundStyle(Theme.textTertiary)
            }
            VStack(alignment: .leading, spacing: Theme.Space.xxs) {
                row(.keepers, color: Theme.keep)
                row(.bursts, color: Theme.accent)
                row(.rejects, color: Theme.reject)
                Text("blurry \(triage.count(.blurry)) · eyes closed \(triage.count(.eyesClosed)) · near-duplicates \(triage.count(.nearDuplicate))")
                    .font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textQuaternary)
                    .lineLimit(2).fixedSize(horizontal: false, vertical: true)
                    .padding(.leading, Theme.Space.xl + Theme.Space.xs)
                    .padding(.bottom, Theme.Space.xs)
                    .accessibilityIdentifier("library.cull.triage.reasons")
                row(.rest, color: Theme.track)
            }
            // UX-15 (item 21): the one place the analysis pass reports: its count here, with the
            // Stop control; there is no bar at the bottom of the window.
            HStack(alignment: .firstTextBaseline, spacing: Theme.Space.s) {
                Text(summary)
                    .font(Theme.Fonts.label).foregroundStyle(Theme.textTertiary)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("library.cull.triage.summary")
                if assist.progress != nil {
                    Spacer(minLength: 0)
                    Button("Stop") { assist.cancel() }
                        .buttonStyle(.theme(.bordered, height: Theme.Height.small))
                        .help("Pause measuring; it picks up where it left off the next time this folder opens")
                        .accessibilityIdentifier("library.assist.stop")
                }
            }
            Hairline()
            VStack(alignment: .leading, spacing: Theme.Space.xs) {
                Text("Your decisions").font(Theme.Fonts.bodySemibold).foregroundStyle(Theme.textPrimary)
                count("Keeps", model.counts.keep, Theme.keep)
                count("Rejects", model.counts.reject, Theme.reject)
                count("Undecided", model.counts.undecided, Theme.track)
            }
            Spacer(minLength: 0)
            VStack(alignment: .leading, spacing: Theme.Space.s) {
                HStack {
                    Text(progressTitle).font(Theme.Fonts.labelMedium).foregroundStyle(Theme.textSecondary)
                    Spacer()
                    Text("\(model.burstsDecided.formatted()) decided").font(Theme.Fonts.captionNumeric).foregroundStyle(Theme.textTertiary)
                }
                GeometryReader { geo in
                    ZStack(alignment: .leading) {
                        RoundedRectangle(cornerRadius: Theme.Space.xxs).fill(Theme.track)
                        RoundedRectangle(cornerRadius: Theme.Space.xxs).fill(Theme.accent)
                            .frame(width: geo.size.width * progressFraction)
                            .animation(Theme.Motion.appear, value: progressFraction)
                    }
                }
                .frame(height: Theme.Space.xs)
                .accessibilityElement()
                .accessibilityLabel("\(model.burstsDecided) of \(triage.burstCount) bursts decided")
                .accessibilityIdentifier("library.cull.triage.progress")
            }
        }
        .padding(.horizontal, Theme.Space.l)
        .padding(.top, Theme.Space.l)
        .padding(.bottom, Theme.Space.m)
        .frame(width: Theme.Width.triageColumn)
        .frame(maxHeight: .infinity, alignment: .top)
        // UX-16: the prototype's column has no fill, only the hairline on its right.
        .background(Theme.canvas)
        .overlay(alignment: .trailing) { Hairline(vertical: true) }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Triage")
        .accessibilityIdentifier("library.cull.triage")
    }

    private var summary: String {
        if let p = assist.progress {
            // The culling pass; a face pass (Cull ▸ Analyze Faces) names itself.
            let what = p.title.hasPrefix("Measuring") ? "Sorting the shoot while it loads" : p.title
            return p.total > 0 ? "\(what): \(p.done.formatted()) of \(p.total.formatted()) measured" : what + "…"
        }
        if !model.isEngineBacked { return "Signals need a folder opened on the engine; bursts still group by time." }
        if triage.measured == 0 { return "Nothing measured yet. Cull ▸ Analyze Shoot Again sorts the shoot." }
        return "The agent sorted the shoot while it loaded. Decide the bursts; the rest is one click."
    }

    private var progressTitle: String {
        if let n = model.focusedBurstOrdinal { return "Burst \(n) of \(triage.burstCount)" }
        return triage.burstCount == 0 ? "No bursts" : "\(triage.burstCount) bursts"
    }

    private var progressFraction: CGFloat {
        guard triage.burstCount > 0 else { return 0 }
        return CGFloat(model.burstsDecided) / CGFloat(triage.burstCount)
    }

    private func row(_ bucket: CullTriage.Bucket, color: Color) -> some View {
        let on = model.cullFilter == .bucket(bucket)
        return Button { model.setCullFilter(.bucket(bucket)) } label: {
            HStack(spacing: Theme.Space.s + Theme.Space.xxs) {
                Circle().fill(color).frame(width: Theme.Space.s, height: Theme.Space.s)
                Text(bucket.title).font(Theme.Fonts.body).foregroundStyle(Theme.textPrimary)
                Spacer()
                Text(triage.count(bucket).formatted()).font(Theme.Fonts.labelNumeric).foregroundStyle(Theme.textTertiary)
            }
            .padding(.horizontal, Theme.Space.s + Theme.Space.xxs)
            .frame(height: Theme.Height.sectionHeader)
            .background(RoundedRectangle(cornerRadius: Theme.Radius.control).fill(on ? Theme.raised : Theme.clear))
            .overlay(RoundedRectangle(cornerRadius: Theme.Radius.control)
                .strokeBorder(on ? Theme.hairlineStrong : Theme.clear, lineWidth: Theme.Space.hairline))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .help(on ? "Show the whole shoot again" : "Show only these frames")
        .accessibilityAddTraits(on ? .isSelected : [])
        .accessibilityIdentifier("library.cull.triage.\(bucket.rawValue)")
    }

    /// One "step" row of Your decisions: a 12 pt square swatch (the prototype's), the label, the count.
    private func count(_ title: String, _ n: Int, _ color: Color) -> some View {
        HStack(spacing: Theme.Space.s) {
            RoundedRectangle(cornerRadius: Theme.Space.xxs).fill(color).frame(width: Theme.Space.m, height: Theme.Space.m)
            Text(title).font(Theme.Fonts.label).foregroundStyle(Theme.textSecondary)
            Spacer()
            Text(n.formatted()).font(Theme.Fonts.labelNumeric).foregroundStyle(Theme.textTertiary)
        }
        .padding(.horizontal, Theme.Space.s + Theme.Space.xxs)
        .frame(height: Theme.Height.regular)
    }
}
