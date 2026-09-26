//! Previews of open documents through the compositor's GPU-resident
//! renderer (CPU reference compositor when no GPU is available), and
//! `describe_document`.
use std::sync::{Arc, OnceLock};

use compositor::gpu::GpuCompositor;
use compositor::resident::ResidentRenderer;
use compositor::{Compositor, Depth, DocOp, DocState, Document, Layer, LayerKind, LayerProps};
use engine_api::id::{DocumentId, LayerId};
use engine_api::tile::Extent;
use engine_api::{EngineError, EngineResult};
use serde_json::{Value, json};

use super::{Documents, io::encode_srgb, list_layers, summary};

/// Page budget of one document's resident renderer.
const RESIDENT_BUDGET: u64 = 512 << 20;
/// Most thumbnails `describe_document` renders.
const MAX_THUMBNAILS: usize = 32;

fn gpu() -> Option<&'static GpuCompositor> {
    static GPU: OnceLock<Option<GpuCompositor>> = OnceLock::new();
    GPU.get_or_init(|| {
        if std::env::var_os("TESSERA_NO_GPU").is_some() {
            return None;
        }
        GpuCompositor::new().ok()
    })
    .as_ref()
}

/// The finest pyramid level whose long edge fits `max_px` (never upscales).
fn level_for(canvas: Extent, max_px: u32) -> u8 {
    let mut level = 0u8;
    while level + 1 < canvas.full_level_count().min(compositor::render::MAX_LEVEL) {
        let e = canvas.at_level(level);
        if e.width.max(e.height) <= max_px {
            break;
        }
        level += 1;
    }
    level
}

fn to_rgba8(e: Extent, rgba: &[f32], depth: Depth) -> EngineResult<image::RgbaImage> {
    let float = depth == Depth::F32;
    let px = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| {
            let enc = |v: f32| {
                if float {
                    encode_srgb(v)
                } else {
                    v.clamp(0.0, 1.0)
                }
            };
            [
                (enc(p[0]) * 255.0).round() as u8,
                (enc(p[1]) * 255.0).round() as u8,
                (enc(p[2]) * 255.0).round() as u8,
                (p[3].clamp(0.0, 1.0) * 255.0).round() as u8,
            ]
        })
        .collect();
    image::RgbaImage::from_raw(e.width, e.height, px)
        .ok_or_else(|| EngineError::internal("preview size"))
}

/// A layer thumbnail.
#[derive(Debug, Clone)]
pub struct LayerThumbnail {
    /// Layer.
    pub layer: LayerId,
    /// The layer alone (normal blending, full opacity), straight RGBA.
    pub image: image::RgbaImage,
}

/// `describe_document`: a JSON summary plus images.
#[derive(Debug, Clone)]
pub struct DocumentDescription {
    /// Canvas, depth, profile, history, selection and every layer.
    pub summary: Value,
    /// The composite at a preview level.
    pub composite: image::RgbaImage,
    /// Thumbnails of pixel, fill, text, smart-object and group layers.
    pub thumbnails: Vec<LayerThumbnail>,
}

impl Documents {
    /// True if the document's last preview came from the GPU-resident
    /// renderer (false: CPU reference compositor).
    pub fn preview_is_resident(&self, id: DocumentId) -> EngineResult<bool> {
        Ok(self.session(id)?.renderer.is_some())
    }

    /// The composite at the finest pyramid level whose long edge fits
    /// `max_px`, rendered by the document's resident GPU renderer. Returns
    /// the level.
    pub fn render_preview(
        &mut self,
        id: DocumentId,
        max_px: u32,
    ) -> EngineResult<(u8, image::RgbaImage)> {
        if !(1..=8192).contains(&max_px) {
            return Err(EngineError::invalid("max_px", "must be within 1..=8192"));
        }
        let session = self.session_mut(id)?;
        let state = session.doc.state().clone();
        let level = level_for(state.canvas, max_px);
        let resident = (|| -> EngineResult<Option<(Extent, Vec<f32>)>> {
            let Some(gpu) = gpu() else {
                return Ok(None);
            };
            if session.renderer.is_none() {
                session.renderer = Some(ResidentRenderer::with_budget(gpu, RESIDENT_BUDGET)?);
            }
            let r = session.renderer.as_mut().expect("just created");
            r.render(&session.doc, level)?;
            r.wait()?;
            Ok(Some(r.read_level(level, false)?))
        })();
        let (e, rgba) = match resident {
            Ok(Some(v)) => v,
            // No GPU, or a document the resident path cannot mirror: the
            // CPU reference compositor renders the same level.
            Ok(None) | Err(_) => {
                if let Err(e) = &resident {
                    eprintln!(
                        "tessera-mcp: resident preview failed, using the CPU compositor: {e}"
                    );
                }
                session.renderer = None;
                Compositor::new(64 << 20).render_level_rgba(&session.doc, level)?
            }
        };
        Ok((level, to_rgba8(e, &rgba, state.depth)?))
    }

    /// Layers, sizes, blend modes, history and selection, with a composite
    /// preview (`max_px` long edge) and layer thumbnails (`thumb_px`).
    pub fn describe(
        &mut self,
        id: DocumentId,
        max_px: u32,
        thumb_px: Option<u32>,
    ) -> EngineResult<DocumentDescription> {
        let (level, composite) = self.render_preview(id, max_px)?;
        let (brush, selection) = self.engines();
        let (brush, selection) = (brush.to_owned(), selection.to_owned());
        let session = self.session(id)?;
        let state = session.doc.state();
        let layers = list_layers(state)?;
        let mut thumbnails = Vec::new();
        if let Some(px) = thumb_px {
            if !(1..=1024).contains(&px) {
                return Err(EngineError::invalid(
                    "thumbnail_px",
                    "must be within 1..=1024",
                ));
            }
            for info in layers.iter().take(MAX_THUMBNAILS) {
                let layer = state.find(info.id).expect("listed layer exists");
                if let Some(image) = thumbnail(state, layer, px)? {
                    thumbnails.push(LayerThumbnail {
                        layer: info.id,
                        image,
                    });
                }
            }
        }
        let depth_of = |id: LayerId| {
            let mut d = 0;
            let mut cur = layers.iter().find(|l| l.id == id).and_then(|l| l.parent);
            while let Some(p) = cur {
                d += 1;
                cur = layers.iter().find(|l| l.id == p).and_then(|l| l.parent);
            }
            d
        };
        let rows: Vec<Value> = layers
            .iter()
            .map(|l| {
                let mut v = serde_json::to_value(l).expect("serializable");
                v["nesting"] = json!(depth_of(l.id));
                v["thumbnail"] = json!(thumbnails.iter().position(|t| t.layer == l.id));
                v
            })
            .collect();
        let history = &session.history;
        let recent: Vec<Value> = history
            .lineage(history.head)?
            .iter()
            .rev()
            .take(20)
            .map(|e| {
                json!({
                    "entry": e.id, "label": e.meta.label, "author": e.meta.author,
                    "rationale": e.meta.rationale, "group": e.meta.group,
                })
            })
            .collect();
        let selection_bounds = match &state.selection {
            None => Value::Null,
            Some(sel) => json!(super::dense::content_bounds(sel, 0)?.map(super::canvas_rect)),
        };
        let summary = json!({
            "document": id,
            "path": session.path,
            "summary": summary(state)?,
            "ppi": state.ppi,
            "profile": state.profile.as_ref().map(|p| json!({"name": p.name, "handle": p.handle})),
            "layers": rows,
            "history": {
                "head": history.head,
                "entries": history.entries.len(),
                "recent": recent,
            },
            "selection": {"active": state.selection.is_some(), "bounds": selection_bounds},
            "saved_selections": session.saved_selections().iter().map(|s| json!({"id": s.id, "name": s.name})).collect::<Vec<_>>(),
            "preview": {"level": level, "width": composite.width(), "height": composite.height()},
            "engines": {"brush": brush, "selection": selection},
            "renderer": if session.renderer.is_some() { "resident-gpu" } else { "cpu" },
            "warnings": session.warnings,
        });
        Ok(DocumentDescription {
            summary,
            composite,
            thumbnails,
        })
    }
}

/// The layer alone: its content (and mask) at full opacity and Normal
/// blending over transparency. Adjustment layers have no thumbnail.
fn thumbnail(state: &DocState, layer: &Layer, px: u32) -> EngineResult<Option<image::RgbaImage>> {
    if matches!(layer.kind, LayerKind::Adjustment(_)) {
        return Ok(None);
    }
    let mut alone = layer.clone();
    alone.props = LayerProps {
        name: alone.props.name.clone(),
        ..LayerProps::default()
    };
    if let LayerKind::Group { children, .. } = &mut alone.kind {
        for c in children.iter_mut() {
            Arc::make_mut(c).props.clipped = false;
        }
    }
    let mut s = DocState::new(state.canvas, state.depth);
    s.profile = state.profile.clone();
    let mut doc = Document::new(s);
    doc.apply(DocOp::AddLayer {
        parent: None,
        index: 0,
        layer: alone,
    })?;
    let level = level_for(state.canvas, px);
    let (e, rgba) = Compositor::new(16 << 20).render_level_rgba(&doc, level)?;
    Ok(Some(to_rgba8(e, &rgba, state.depth)?))
}
