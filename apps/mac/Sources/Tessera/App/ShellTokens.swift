import SwiftUI

// UX-11 shell tokens (redesign SPEC §1–2). Kept out of Theme.swift while UX-10 replaces its values;
// fold them into the Width / Height / Radius enums there once that pass has landed.

extension Theme.Space {
    /// UX-16: the photo area's padding and the panel's inner padding (the prototype's 20 px).
    static let panelInset: CGFloat = 20
}

extension Theme.Height {
    /// UX-16: the search bar ⌘F drops under the toolbar.
    static let searchBar: CGFloat = 40
    /// UX-16: what the bottom row (Try cards and the agent row) reserves at the bottom of the
    /// Edit panel, so no control hides under it (the prototype's 150 px).
    static let bottomRow: CGFloat = 150
}

extension Theme.Width {
    /// UX-16: the Edit panel, a plain column beside the photo (the prototype's 320 px).
    static let editPanel: CGFloat = 320
}

extension Theme.Width {
    /// The agent field, bottom centre of every mode; it shrinks to the minimum only when the row
    /// cannot hold it (UX-16).
    static let agentField: CGFloat = 560
    static let agentFieldMin: CGFloat = 360
    /// The search field, collapsed and while it has the keyboard.
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

// UX-13: the dial, the Try cards and the on-photo HUDs (SPEC §3).

extension Theme.Width {
    /// The dial HUD beside the cursor.
    static let dial: CGFloat = 236
    /// One Try card (its thumbnail is the full width).
    static let tryCard: CGFloat = 132
}

extension Theme.Height {
    /// The dial's knob and its inner disc.
    static let dialKnob: CGFloat = 120
    static let dialKnobInner: CGFloat = 96
    /// A Try card: the thumbnail, then the caption line.
    static let tryThumbnail: CGFloat = 64
    static let tryCaption: CGFloat = 28
}

// UX-15: Deliver (SPEC §5).

extension Theme.Width {
    /// The What · How · Where column of Deliver (Tab hides it).
    static let deliverColumn: CGFloat = 320
    /// One cell of the preview grid (the files to be written).
    static let deliverCell: CGFloat = 172
}

extension Theme.Height {
    /// One row of the What / How lists.
    static let deliverRow: CGFloat = 36
    /// The thumbnail of a preview cell; the caption line sits under it.
    static let deliverCell: CGFloat = 116
}
