import SwiftUI

// UX-11 shell tokens (redesign SPEC §1–2). Kept out of Theme.swift while UX-10 replaces its values;
// fold them into the Width / Height / Radius enums there once that pass has landed.

extension Theme.Width {
    /// The agent field, bottom centre of every mode.
    static let agentField: CGFloat = 560
    /// The toolbar search field, collapsed and while it has the keyboard.
    static let search: CGFloat = 180
    static let searchExpanded: CGFloat = 360
    /// The filter chips popover under the search field.
    static let filterPopover: CGFloat = 520
    /// The folder name in the toolbar truncates beyond this.
    static let folderButton: CGFloat = 240
    /// UX-14: the Cull triage column, and one cell of the burst strip (SPEC §4).
    static let triageColumn: CGFloat = 260
    static let burstCell: CGFloat = 172
    /// The big photo of the burst view is at most this wide.
    static let burstPhotoMax: CGFloat = 1100
    /// The sharpness bar inside a HUD pill.
    static let signalBar: CGFloat = 48
}

extension Theme.Height {
    static let agentField: CGFloat = 48
    /// One button of the tools strip beside the agent field (Masks, Crop, Heal).
    static let toolStrip: CGFloat = 36
    /// The folder popover (the source sidebar's content).
    static let folderPopover: CGFloat = 480
    /// UX-14: one cell of the burst strip, and the burst view's action buttons.
    static let burstCell: CGFloat = 116
    static let burstAction: CGFloat = 36
}

extension Theme.Radius {
    /// The agent field.
    static let agentField: CGFloat = 12
}
