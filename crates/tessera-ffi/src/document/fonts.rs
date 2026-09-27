//! The one font snapshot every document compositor renders live text with
//! (WP B5-10b).
//!
//! Every `Compositor` the document bridge constructs — the session renderer
//! and its styled-document CPU fallback, thumbnails, merge down / flatten,
//! export_flat, filter previews and smart-filter bakes, the eyedropper — comes
//! from [`compositor`], which installs a renderer over the shared snapshot of
//! document/text.rs (system fonts discovered once, plus any test fixtures).
//! The Type tool lays text out over the same snapshot, so layout, viewport
//! and every output agree. A family missing from the snapshot is the
//! documented `font unavailable: <family>` error on every path: there is no
//! substitution.
//!
//! Not covered (engine-owned, NEEDS.md): the PSD writer's composite and text
//! layer pixels, `ConvertToPixels` / `rasterize_layer`, the resident
//! renderer's smart-filter CPU stack and engine merge helpers each build a
//! system-font compositor of their own.

use compositor::{Compositor, resident::ResidentRenderer};

/// A CPU compositor with a `budget`-byte cache over the shared font snapshot.
pub(crate) fn compositor(budget: usize) -> Compositor {
    let c = Compositor::new(budget);
    c.set_text_renderer(super::text::shared_text_renderer());
    c
}

/// Installs the shared font snapshot into the resident (GPU) renderer.
pub(crate) fn install(resident: &mut ResidentRenderer) {
    resident.set_text_renderer(super::text::shared_text_renderer());
}
