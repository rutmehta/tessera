//! Renderer-local damage using ordered geometry and finite style support.
use super::{Compositor, DocRef, NodeKey, Part, exec::Region};
use crate::{Document, Layer, LayerKind, Rect};
use engine_api::{
    EngineResult,
    tile::{Tile, TileCoord},
};
use std::sync::atomic::Ordering;

#[derive(Clone, Copy)]
struct Footprint {
    rect: Rect,
    halo: i64,
}

// Match styles::render's finite kernels. Round each kernel separately, include
// bilinear sampling and bevel derivatives. Nested supports add, not max.
fn style_halo(styles: &super::styles::LayerStyles) -> EngineResult<i64> {
    use super::styles::StyleEffect::*;
    styles.validate()?;
    let radius = |v: f32| (v * styles.scale).ceil() as i64;
    Ok(styles
        .effects
        .iter()
        .map(|e| match e {
            DropShadow(s) | InnerShadow(s) if s.enabled => {
                radius(s.size) + radius(s.spread) + radius(s.distance.abs()) + 2
            }
            OuterGlow(s) | InnerGlow(s) if s.enabled => radius(s.size) + radius(s.spread) + 2,
            Bevel(s) if s.enabled => radius(s.size) + radius(s.soften) + 2,
            Satin(s) if s.enabled => radius(s.size) + radius(s.distance.abs()) + 2,
            Stroke(s) if s.enabled => radius(s.size) + 1,
            _ => 0,
        })
        .max()
        .unwrap_or(0))
}

fn expanded(rect: Rect, halo: i64) -> Rect {
    if rect.is_empty() {
        return rect;
    }
    Rect::new(
        rect.x0.saturating_sub(halo),
        rect.y0.saturating_sub(halo),
        rect.x1.saturating_add(halo),
        rect.y1.saturating_add(halo),
    )
}

#[derive(Clone)]
pub(super) struct Snapshot {
    base: [u8; 32],
    items: Vec<([u8; 32], Rect)>,
    key: NodeKey,
}

pub(super) fn has_live(layers: &[std::sync::Arc<Layer>]) -> bool {
    layers.iter().any(|l| match &l.kind {
        LayerKind::Text { .. } | LayerKind::Shape { .. } => true,
        LayerKind::Group { children, .. } => has_live(children),
        _ => false,
    })
}

fn frame_key(doc: u64, coord: TileCoord) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(&doc.to_le_bytes());
    h.update(&[coord.level]);
    h.update(&coord.x.to_le_bytes());
    h.update(&coord.y.to_le_bytes());
    *h.finalize().as_bytes()
}

impl Compositor {
    pub(super) fn warm_live_viewport(&self, doc: &Document, coord: TileCoord) -> bool {
        !super::effects::has_styles(doc.state())
            && !super::has_local_adjustments(doc.state())
            && has_live(&doc.state().root)
            && self
                .live
                .frames
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&frame_key(doc.key(), coord))
                .is_some()
    }

    fn live_snapshot(&self, doc: DocRef<'_>, coord: TileCoord) -> EngineResult<Snapshot> {
        let rect =
            Rect::of_tile(coord, doc.state.canvas.at_level(coord.level)).to_level0(coord.level);
        let mut base = blake3::Hasher::new();
        base.update(&doc.state.root_rev.to_le_bytes());
        base.update(&doc.state.canvas.width.to_le_bytes());
        base.update(&doc.state.canvas.height.to_le_bytes());
        base.update(&[doc.state.depth.bytes() as u8]);
        base.update(&serde_json::to_vec(&doc.state.global_light).map_err(super::live::invalid)?);
        let mut items = Vec::new();
        self.live_list_identity(
            &doc.state.root,
            coord,
            Footprint { rect, halo: 0 },
            &mut base,
            &mut items,
        )?;
        let base = *base.finalize().as_bytes();
        let mut h = blake3::Hasher::new();
        h.update(&base);
        for (identity, _) in &items {
            h.update(identity);
        }
        let digest = h.finalize();
        let bytes = digest.as_bytes();
        Ok(Snapshot {
            base,
            items,
            key: NodeKey {
                doc: doc.key,
                // Separate namespace from revision-keyed root composites.
                node: u64::MAX,
                part: Part::Root,
                stamp: u64::from_le_bytes(bytes[..8].try_into().unwrap()),
                coord,
            },
        })
    }

    fn live_list_identity(
        &self,
        layers: &[std::sync::Arc<Layer>],
        coord: TileCoord,
        footprint: Footprint,
        base: &mut blake3::Hasher,
        items: &mut Vec<([u8; 32], Rect)>,
    ) -> EngineResult<()> {
        // Mirror TileJob::compile_list: a hidden clipping base suppresses
        // its whole chain, and a hidden group suppresses all descendants.
        let mut i = 0;
        while i < layers.len() {
            let first = i;
            i += 1;
            while i < layers.len() && layers[i].props.clipped {
                i += 1;
            }
            if !layers[first].props.visible {
                continue;
            }
            for layer in &layers[first..i] {
                if layer.props.visible {
                    self.live_layer_identity(layer, coord, footprint, base, items)?;
                }
            }
        }
        Ok(())
    }

    fn live_layer_identity(
        &self,
        layer: &Layer,
        coord: TileCoord,
        footprint: Footprint,
        base: &mut blake3::Hasher,
        items: &mut Vec<([u8; 32], Rect)>,
    ) -> EngineResult<()> {
        base.update(&layer.id.0.to_le_bytes());
        base.update(&layer.props_rev.to_le_bytes());
        let footprint = Footprint {
            halo: footprint
                .halo
                .saturating_add(style_halo(&layer.props.styles)?),
            ..footprint
        };
        let rect = footprint.rect;
        match &layer.kind {
            LayerKind::Text { .. } | LayerKind::Shape { .. } => {
                base.update(b"live");
                // Content revision includes text edits AND mask edits. Replace
                // only the source portion; explicitly retain every mask input.
                base.update(
                    &serde_json::to_vec(&(
                        &layer.vector_mask,
                        layer.mask.as_ref().map(|m| {
                            (
                                m.enabled,
                                m.density,
                                m.raster.default_value(),
                                if footprint.halo > 0 {
                                    m.raster.max_rev()
                                } else {
                                    m.raster.footprint_rev(coord.level, coord.x, coord.y)
                                },
                            )
                        }),
                    ))
                    .map_err(super::live::invalid)?,
                );
                for p in &self.prepare_live(layer)?.items {
                    let bounds = expanded(p.bounds, footprint.halo);
                    if bounds.intersects(&rect) {
                        let mut h = blake3::Hasher::new();
                        h.update(&layer.id.0.to_le_bytes());
                        h.update(&p.identity);
                        items.push((*h.finalize().as_bytes(), bounds.intersect(&rect)));
                    }
                }
            }
            LayerKind::Group { children, .. } => {
                base.update(b"group");
                base.update(&layer.content_rev.to_le_bytes());
                if let Some(m) = &layer.mask {
                    base.update(&m.raster.max_rev().to_le_bytes());
                }
                self.live_list_identity(children, coord, footprint, base, items)?;
                base.update(b"end-group");
            }
            _ => {
                base.update(b"ordinary");
                base.update(&layer.stamp(coord.level, coord.x, coord.y).to_le_bytes());
                // Neighbour pixels/masks can feed a styled ancestor. Retain
                // their global revisions while live geometry remains spatial.
                if footprint.halo > 0 {
                    if let Some(r) = layer.raster() {
                        base.update(&r.max_rev().to_le_bytes());
                    }
                    if let Some(m) = &layer.mask {
                        base.update(&m.raster.max_rev().to_le_bytes());
                    }
                }
            }
        }
        Ok(())
    }

    pub(super) fn render_live_scene(
        &self,
        doc: &Document,
        coord: TileCoord,
        pass: Option<&super::smart_filters::FilterPass>,
        style_pass: Option<&super::style_pass::StylePass>,
        cancel: Option<&engine_api::jobs::CancellationToken>,
    ) -> EngineResult<Tile> {
        let dref = DocRef {
            state: doc.state(),
            key: doc.key(),
            pass,
            style_pass,
            cancel,
        };
        // Validate before level scaling or tile-origin arithmetic.
        let mut job = self.job(dref, coord)?;
        let snapshot = self.live_snapshot(dref, coord)?;
        let frame_key = frame_key(doc.key(), coord);
        let previous = self
            .live
            .frames
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&frame_key);
        let remember = || {
            let mut frames = self.live.frames.lock().unwrap_or_else(|e| e.into_inner());
            // Evicting byte-bounded metadata merely loses partial updates.
            frames.insert(
                frame_key,
                std::sync::Arc::new(snapshot.clone()),
                std::mem::size_of::<Snapshot>()
                    + snapshot.items.len() * std::mem::size_of::<([u8; 32], Rect)>(),
            );
        };
        if let Some(tile) = self.cache_get(&snapshot.key) {
            remember();
            return Ok(tile);
        }
        let tr = Rect::of_tile(coord, doc.state().canvas.at_level(coord.level));
        let previous = previous.filter(|p| self.partial_updates && p.base == snapshot.base);
        if let Some(previous) = previous
            && let Some(mut tile) = self.cache.get(&previous.key)
        {
            // Preserve common prefix/suffix (including stacking order). The
            // union of the remaining old/new glyph or fill/stroke bounds is
            // conservative even for bidi reorder, reflow and overlapping glyphs.
            let a = &previous.items;
            let b = &snapshot.items;
            let prefix = a.iter().zip(b).take_while(|(a, b)| a.0 == b.0).count();
            let suffix = a[prefix..]
                .iter()
                .rev()
                .zip(b[prefix..].iter().rev())
                .take_while(|(a, b)| a.0 == b.0)
                .count();
            let damage = a[prefix..a.len() - suffix]
                .iter()
                .chain(&b[prefix..b.len() - suffix])
                .fold(Rect::default(), |r, (_, b)| r.union(b))
                .to_level(coord.level)
                .intersect(&tr);
            if damage.is_empty() {
                self.stats.root_reused.fetch_add(1, Ordering::Relaxed);
                self.cache_put(snapshot.key, tile.clone());
                remember();
                return Ok(tile);
            }
            if damage.area() * 2 <= tr.area() {
                job.full = false;
                job.region = Region {
                    x0: (damage.x0 - tr.x0) as usize,
                    y0: (damage.y0 - tr.y0) as usize,
                    x1: (damage.x1 - tr.x0) as usize,
                    y1: (damage.y1 - tr.y0) as usize,
                };
                let acc = job.run(&job.compile()?)?;
                let dst = tile.samples_mut::<f32>()?;
                for c in 0..4 {
                    for y in job.region.y0..job.region.y1 {
                        let start = c * job.n + y * job.w;
                        let range = start + job.region.x0..start + job.region.x1;
                        dst[range.clone()].copy_from_slice(&acc[range]);
                    }
                }
                self.stats.root_partial.fetch_add(1, Ordering::Relaxed);
                self.cache_put(snapshot.key, tile.clone());
                remember();
                return Ok(tile);
            }
        }
        let acc = job.run(&job.compile()?)?;
        let tile = Tile::from_samples(coord, job.layout(), acc)?.with_premultiplied(true)?;
        self.stats.root_full.fetch_add(1, Ordering::Relaxed);
        self.cache_put(snapshot.key, tile.clone());
        remember();
        Ok(tile)
    }
}
