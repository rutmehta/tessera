//! Edit ▸ Auto-Align Layers, Edit ▸ Auto-Blend Layers and File ▸ Automate ▸
//! Photomerge in document mode (WP B5-19) over the compositor's
//! `DocOp::AutoAlignLayers`, `DocOp::AutoBlendLayers` and `DocOp::Photomerge`.
//!
//! Every call is one history node. Selections are validated here first, in
//! words the sheets can show (two or more top-level, unlocked, unclipped
//! layers; pixel layers for alignment, pixel or aligned layers for blending),
//! so nothing changes on error; the engine validates again atomically.
//!
//! Alignment and blending are not cancellable yet (`merge::layers` takes no
//! token): the app shows an indeterminate busy state, never a Cancel button.
//! Lens corrections (vignette removal, geometric distortion) need one
//! explicit calibration per layer; library lens profiles are not mapped.

use super::{DocumentSession, DocumentUpdate, Opened, io};
use crate::{Engine, Result, failure};
use compositor::{DocOp, DocState, Document, LayerId, LayerKind, Raster};
use engine_api::{EngineResult, tile::Extent};
use merge::{
    LinearImage,
    layers::{AlignMode, AlignOptions, BlendMode, BlendOptions, LensCorrection},
};
use std::{path::Path, sync::Arc, sync::atomic::AtomicBool};

/// Most layers or photos one stack takes (`merge::layers`).
const MAX_STACK: usize = 128;

/// Auto-Align / Photomerge projection ("Layout" in Photoshop).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StackAlignMode {
    Auto,
    Perspective,
    Cylindrical,
    Spherical,
    Collage,
    Reposition,
}

/// Auto-Blend method.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StackBlendMode {
    /// Seams between overlapping images.
    Panorama,
    /// Focus stacking: each region from its sharpest layer.
    StackImages,
}

/// One layer's radial lens calibration (`merge::layers::LensCorrection`):
/// distortion maps ideal to observed radius by `r·(1 + k1·r² + k2·r⁴ +
/// k3·r⁶)`; vignette coefficients describe observed illumination.
#[derive(Clone, Copy, Debug, Default, PartialEq, uniffi::Record)]
pub struct StackLensCorrection {
    pub k1: f64,
    pub k2: f64,
    pub k3: f64,
    pub v1: f64,
    pub v2: f64,
    pub v3: f64,
}

/// Auto-Align Layers options.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct StackAlignOptions {
    pub mode: StackAlignMode,
    /// Index into the ids (or sources) of the layer that stays put.
    pub reference_index: u32,
    pub vignette_removal: bool,
    pub geometric_distortion: bool,
    /// One per layer, in order; required by either lens correction.
    pub lens_corrections: Vec<StackLensCorrection>,
    pub seed: u64,
}

/// Auto-Blend Layers options.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct StackBlendOptions {
    pub mode: StackBlendMode,
    pub seamless_tones: bool,
    /// Content-Aware Fill Transparent Areas: one new layer filling the
    /// pixels no layer covers.
    pub content_aware_fill: bool,
    pub seed: u64,
}

/// Whether the given layers can be aligned / blended, and why not.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct StackEligibility {
    pub can_align: bool,
    pub can_blend: bool,
    /// The alignment problem, else the blend problem.
    pub reason: Option<String>,
}

/// Photoshop's defaults: Auto layout, no lens corrections.
#[uniffi::export]
pub fn default_stack_align_options() -> StackAlignOptions {
    StackAlignOptions {
        mode: StackAlignMode::Auto,
        reference_index: 0,
        vignette_removal: false,
        geometric_distortion: false,
        lens_corrections: Vec::new(),
        seed: AlignOptions::default().seed,
    }
}

/// Photoshop's defaults: Panorama with seamless tones, no fill.
#[uniffi::export]
pub fn default_stack_blend_options() -> StackBlendOptions {
    let d = BlendOptions::default();
    StackBlendOptions {
        mode: StackBlendMode::Panorama,
        seamless_tones: d.seamless_tones,
        content_aware_fill: d.fill_transparent,
        seed: d.seed,
    }
}

impl From<StackAlignMode> for AlignMode {
    fn from(m: StackAlignMode) -> Self {
        match m {
            StackAlignMode::Auto => AlignMode::Auto,
            StackAlignMode::Perspective => AlignMode::Perspective,
            StackAlignMode::Cylindrical => AlignMode::Cylindrical,
            StackAlignMode::Spherical => AlignMode::Spherical,
            StackAlignMode::Collage => AlignMode::Collage,
            StackAlignMode::Reposition => AlignMode::Reposition,
        }
    }
}

impl From<StackBlendMode> for BlendMode {
    fn from(m: StackBlendMode) -> Self {
        match m {
            StackBlendMode::Panorama => BlendMode::Panorama,
            StackBlendMode::StackImages => BlendMode::StackImages,
        }
    }
}

/// The engine's options for `count` layers, after the checks the engine
/// would report less clearly.
fn align_options(o: &StackAlignOptions, count: usize) -> Result<AlignOptions> {
    if o.reference_index as usize >= count {
        return Err(failure(format!(
            "the reference layer must be one of the {count} layers"
        )));
    }
    if (o.vignette_removal || o.geometric_distortion) && o.lens_corrections.len() != count {
        let what = if o.vignette_removal {
            "Vignette removal"
        } else {
            "Geometric distortion correction"
        };
        return Err(failure(format!(
            "{what} needs a lens calibration for every layer ({} given for {count})",
            o.lens_corrections.len()
        )));
    }
    Ok(AlignOptions {
        mode: o.mode.into(),
        reference: o.reference_index as usize,
        seed: o.seed,
        vignette_removal: o.vignette_removal,
        geometric_distortion: o.geometric_distortion,
        lens_corrections: o
            .lens_corrections
            .iter()
            .map(|c| LensCorrection {
                distortion: [c.k1, c.k2, c.k3],
                vignette: [c.v1, c.v2, c.v3],
            })
            .collect(),
    })
}

fn blend_options(o: &StackBlendOptions) -> BlendOptions {
    BlendOptions {
        mode: o.mode.into(),
        seamless_tones: o.seamless_tones,
        fill_transparent: o.content_aware_fill,
        seed: o.seed,
        ..BlendOptions::default()
    }
}

/// `DocOp`'s Content-Aware Fill hook: `filters::caf::fill` (non-capturing,
/// so it coerces to `compositor::edit::ContentAwareFill`).
fn content_aware_fill(input: &Raster, holes: &[f32], seed: u64) -> EngineResult<Raster> {
    let params = filters::caf::FillParams {
        seed,
        ..Default::default()
    };
    Ok(filters::caf::fill(input, holes, &params, &AtomicBool::new(false))?.composite)
}

fn fill_for(o: &StackBlendOptions) -> Option<compositor::edit::ContentAwareFill> {
    o.content_aware_fill
        .then_some(content_aware_fill as compositor::edit::ContentAwareFill)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Align,
    Blend,
}

/// Why `ids` cannot be aligned (`Stage::Align`) or blended, in the sheets'
/// words; `None` when they can.
fn problem(s: &DocState, ids: &[u64], stage: Stage) -> Option<String> {
    let (verb, title) = match stage {
        Stage::Align => ("align", "Auto-Align"),
        Stage::Blend => ("blend", "Auto-Blend"),
    };
    if ids.len() < 2 {
        return Some(format!("Select two or more layers to {verb}"));
    }
    if ids.len() > MAX_STACK {
        return Some(format!("{title} takes at most {MAX_STACK} layers"));
    }
    for (i, id) in ids.iter().enumerate() {
        if ids[..i].contains(id) {
            return Some(format!("layer {id} is selected twice"));
        }
        let Some(l) = s.root.iter().find(|l| l.id.0 == *id) else {
            return Some(match s.find(LayerId(*id)) {
                Some(l) => format!(
                    "{title} works on top-level layers; “{}” is inside a group",
                    l.props.name
                ),
                None => format!("layer {id} not found"),
            });
        };
        let name = &l.props.name;
        let locks = l.props.locks;
        if locks.all || locks.pixels || locks.position {
            return Some(format!("“{name}” is locked"));
        }
        if l.props.clipped {
            return Some(format!("“{name}” is part of a clipping mask"));
        }
        let ok = match (&l.kind, stage) {
            (LayerKind::Pixel(r), _) => r.channels() == 4,
            (LayerKind::SmartObject(_), Stage::Blend) => true,
            _ => false,
        };
        if !ok {
            return Some(format!(
                "{title} needs pixel layers; “{name}” is not a pixel layer"
            ));
        }
    }
    None
}

fn is_image_id(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// One Photomerge source (a library image id, rendered developed, or a
/// `.tessera-doc` / PSD / JPEG / PNG / TIFF path) as its display name and the
/// composite's RGB, in the document encoding (as Auto-Align reads layers).
fn load_source(engine: &Arc<Engine>, source: &str) -> Result<(String, LinearImage)> {
    let (opened, name) = if is_image_id(source) {
        let o = io::open_image(engine, source, true)?;
        let name = o.title.clone();
        (o, name)
    } else {
        let path = Path::new(source);
        let name = path
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Layer".into());
        (io::open_path(path)?, name)
    };
    let (extent, rgba) = compositor::Compositor::new(64 << 20).render_level_rgba(&opened.doc, 0)?;
    Ok((
        name,
        LinearImage {
            width: extent.width as usize,
            height: extent.height as usize,
            pixels: rgba
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| [p[0], p[1], p[2]])
                .collect(),
            color_matrix: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            as_shot_neutral: [1.; 3],
        },
    ))
}

/// Decodes every source (blocking) after checking the count.
fn load_sources(engine: &Arc<Engine>, sources: &[String]) -> Result<Vec<(String, LinearImage)>> {
    if sources.len() < 2 {
        return Err(failure("Photomerge needs two or more photos"));
    }
    if sources.len() > MAX_STACK {
        return Err(failure(format!(
            "Photomerge takes at most {MAX_STACK} photos"
        )));
    }
    sources
        .iter()
        .map(|s| load_source(engine, s).map_err(|e| failure(format!("{s}: {e}"))))
        .collect()
}

fn photomerge_op(
    images: Vec<(String, LinearImage)>,
    align: &StackAlignOptions,
    blend: &StackBlendOptions,
) -> Result<DocOp> {
    Ok(DocOp::Photomerge {
        align: align_options(align, images.len())?,
        images,
        blend: blend_options(blend),
        fill: fill_for(blend),
    })
}

#[uniffi::export]
impl DocumentSession {
    /// Whether `ids` (Layers panel selection) can be aligned and blended.
    pub fn stack_eligibility(&self, ids: Vec<u64>) -> Result<StackEligibility> {
        let st = self.shared.lock()?;
        let s = st.live().state();
        let align = problem(s, &ids, Stage::Align);
        let blend = problem(s, &ids, Stage::Blend);
        Ok(StackEligibility {
            can_align: align.is_none(),
            can_blend: blend.is_none(),
            reason: align.or(blend),
        })
    }

    /// Edit ▸ Auto-Align Layers: registers the root pixel layers `ids` (the
    /// reference stays put), extends the canvas to their union and keeps each
    /// source with its transform editable. One history node. Blocking.
    pub fn auto_align_layers(
        &self,
        ids: Vec<u64>,
        options: StackAlignOptions,
    ) -> Result<DocumentUpdate> {
        {
            let st = self.shared.lock()?;
            if let Some(p) = problem(st.live().state(), &ids, Stage::Align) {
                return Err(failure(p));
            }
        }
        let options = align_options(&options, ids.len())?;
        self.edit(
            DocOp::AutoAlignLayers {
                ids: ids.into_iter().map(LayerId).collect(),
                options,
            },
            Some("Auto-Align Layers"),
        )
    }

    /// Edit ▸ Auto-Blend Layers: editable masks on each layer (panorama
    /// seams or focus stacking), optional seamless-tone correction layers
    /// and, when asked, one Content-Aware Fill layer for uncovered pixels.
    /// One history node. Blocking.
    pub fn auto_blend_layers(
        &self,
        ids: Vec<u64>,
        options: StackBlendOptions,
    ) -> Result<DocumentUpdate> {
        {
            let st = self.shared.lock()?;
            if let Some(p) = problem(st.live().state(), &ids, Stage::Blend) {
                return Err(failure(p));
            }
        }
        self.edit(
            DocOp::AutoBlendLayers {
                ids: ids.into_iter().map(LayerId).collect(),
                options: blend_options(&options),
                fill: fill_for(&options),
            },
            Some("Auto-Blend Layers"),
        )
    }

    /// Photomerge into this document: each source (library image id or
    /// file path) becomes a named top-level layer, aligned and blended, as
    /// one history node. Blocking (decodes every source first).
    pub fn photomerge_into_layers(
        &self,
        sources: Vec<String>,
        align: StackAlignOptions,
        blend: StackBlendOptions,
    ) -> Result<DocumentUpdate> {
        let engine = self
            .shared
            .engine
            .upgrade()
            .ok_or_else(|| failure("engine is closed"))?;
        align_options(&align, sources.len().max(1))?;
        let images = load_sources(&engine, &sources)?;
        self.edit(photomerge_op(images, &align, &blend)?, Some("Photomerge"))
    }
}

#[uniffi::export]
impl Engine {
    /// File ▸ Automate ▸ Photomerge: a new Untitled document whose one
    /// history node after "New Document" merges `sources` (library image ids
    /// or file paths) into named, aligned and blended layers. Blocking.
    pub fn photomerge_document(
        self: Arc<Self>,
        sources: Vec<String>,
        align: StackAlignOptions,
        blend: StackBlendOptions,
    ) -> Result<Arc<DocumentSession>> {
        align_options(&align, sources.len().max(1))?;
        let images = load_sources(&self, &sources)?;
        let op = photomerge_op(images, &align, &blend)?;
        let depth = compositor::Depth::F32;
        let mut state = DocState::new(Extent::new(1, 1), depth);
        state.profile = io::profile(None)?;
        let session = self.register_document(
            None,
            Opened {
                doc: Document::new(state),
                title: "Untitled".into(),
                path: None,
                source_image_id: None,
                origin: "New Document",
                unsaved: true,
            },
        );
        if let Err(e) = session.edit(op, Some("Photomerge")) {
            session.close();
            return Err(e);
        }
        Ok(session)
    }
}
