import SwiftUI
import TesseraCore

/// Uses the existing controls and controller. Tabs change inspector composition only.
struct PhotoEditInspectorView: View {
    @Bindable var model: AppModel
    private var tools: DevelopTools { .shared }
    private var masks: MaskTools { .shared }

    var body: some View {
        VStack(spacing: 0) {
            SegmentedPicker(selection: $model.photoInspectorTab, segments: [
                .init(value: PhotoInspectorTab.develop, title: "Develop"),
                .init(value: PhotoInspectorTab.masks, title: "Masks"),
            ])
            .padding(Theme.Space.gutter)
            .accessibilityIdentifier("photo-edit-inspector-tabs")
            Hairline()
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    PanelSection("Editing target") {
                        Text(model.editTarget?.name ?? "No photo selected")
                            .font(Theme.Fonts.labelMedium).foregroundStyle(Theme.textPrimary)
                            .lineLimit(1).help(model.editTarget?.name ?? "No photo selected")
                        Text(model.photoInspectorTab == .develop ? "Whole photo" : masks.selected.map { "Mask: \($0.name)" } ?? "No mask selected")
                            .font(Theme.Fonts.caption).foregroundStyle(Theme.textSecondary)
                            .accessibilityIdentifier("photo-edit-target")
                        if case .unavailable(let reason) = model.developStatus { Hint(reason) }
                        if model.developStatus == .loading { Hint("Opening photo…") }
                    }
                    if model.photoInspectorTab == .masks {
                        PanelSection("Masks") { MasksPanel(model: model, masks: masks) }
                            .developContext(model, tools)
                    } else {
                        PhotoDevelopPanels(model: model)
                    }
                }
            }
            .scrollIndicators(.never)
        }
        .background(Theme.panel)
        .tint(Theme.accent)
        .onChange(of: model.photoInspectorTab) { _, tab in
            if tab == .develop { masks.setActive(false) }
        }
        .onChange(of: masks.active) { _, active in
            if active { model.photoInspectorTab = .masks }
        }
    }
}

private struct PhotoDevelopPanels: View {
    let model: AppModel
    private var tools: DevelopTools { .shared }
    var body: some View {
        Group {
            PanelSection("Histogram") { HistogramPanel(model: model).frame(height: HistogramView.height) }
            PanelSection("Basic") { BasicPanel(model: model) }
            Group {
                PanelSection("Tone Curve", expanded: false) { ToneCurvePanel(model: model, tools: tools) }
                PanelSection("HSL / Color", expanded: false) { HSLPanel(model: model, tools: tools) }
                PanelSection("Color Grading", expanded: false) { ColorGradingPanel(model: model, tools: tools) }
                PanelSection("Detail", expanded: false) { DetailPanel(model: model, tools: tools) }
                PanelSection("Transform", expanded: false) { TransformPanel(model: model, tools: tools, guideTool: .shared) }
                PanelSection("Effects", expanded: false) { EffectsPanel(model: model, tools: tools) }
                PanelSection("Lens Blur", expanded: false) { LensBlurPanel(model: model, tools: tools) }
                PanelSection("Crop & Straighten", expanded: false) { CropPanel(model: model, tools: tools) }
                PanelSection("HDR", expanded: false) { HDRPanel(model: model, tools: tools) }
                PanelSection("Soft Proofing", expanded: false) { SoftProofPanel(proof: .shared) }
                PanelSection("Presets", expanded: false) { PresetsPanel(model: model, tools: tools) }
                PanelSection("Snapshots", expanded: false) { SnapshotsPanel(model: model) }
                PanelSection("History", expanded: false) { HistoryPanel(model: model, tools: tools) }
            }
            .developContext(model, tools)
        }
    }
}
