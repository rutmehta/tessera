//! Output-resolution rasterization of live geometry, shared by CPU and resident paths.
use super::{
    Compositor, DocRef,
    cache::{NodeKey, Part, RenderCache},
};
use crate::{Affine, Depth, Layer, LayerKind, Raster};
use engine_api::tile::{Extent, TILE_SIZE, Tile, TileCoord, TileLayout};
use engine_api::{EngineError, EngineResult};
use std::sync::Mutex;

pub(super) struct LiveRuntime {
    fonts: Mutex<Option<typography::TextRenderer>>,
    cache: RenderCache,
}
impl LiveRuntime {
    pub fn new(budget: usize) -> Self {
        Self {
            fonts: Mutex::new(None),
            cache: RenderCache::new(budget),
        }
    }
    pub fn clear(&self) {
        self.cache.retain(|_| false);
    }
}
fn invalid(e: impl std::fmt::Display) -> EngineError {
    EngineError::invalid("live layer", e.to_string())
}
fn affine(a: Affine) -> vector::Affine {
    let [a, b, c, d, e, f] = a.m;
    vector::Affine::new([a, d, b, e, c, f])
}
fn viewport(canvas: Extent, coord: TileCoord) -> vector::Viewport {
    let e = canvas.at_level(coord.level);
    vector::Viewport {
        width: (e.width - coord.x * TILE_SIZE).min(TILE_SIZE),
        height: (e.height - coord.y * TILE_SIZE).min(TILE_SIZE),
        origin: vector::Point::new(
            f64::from(coord.x * TILE_SIZE) * 2f64.powi(coord.level.into()),
            f64::from(coord.y * TILE_SIZE) * 2f64.powi(coord.level.into()),
        ),
        level: coord.level,
    }
}
fn key(bytes: &[u8], canvas: Extent, depth: Depth, coord: TileCoord, mask: bool) -> NodeKey {
    let mut h = blake3::Hasher::new();
    h.update(bytes);
    h.update(&canvas.width.to_le_bytes());
    h.update(&canvas.height.to_le_bytes());
    h.update(&[depth.bytes() as u8]);
    let digest = h.finalize();
    let b = digest.as_bytes();
    NodeKey {
        doc: u64::from_le_bytes(b[0..8].try_into().unwrap()),
        node: u64::from_le_bytes(b[8..16].try_into().unwrap()),
        stamp: u64::from_le_bytes(b[16..24].try_into().unwrap()),
        part: if mask { Part::Mask } else { Part::Content },
        coord,
    }
}
fn over(dst: &mut [[f32; 4]], src: &[[f32; 4]]) {
    for (d, s) in dst.iter_mut().zip(src) {
        for c in 0..4 {
            d[c] = s[c] + d[c] * (1. - s[3]);
        }
    }
}
fn glyph_path(path: &lyon_path::Path) -> EngineResult<vector::Path> {
    use std::fmt::Write;
    let mut svg = String::new();
    for event in path.iter() {
        use lyon_path::Event::*;
        match event {
            Begin { at } => {
                write!(svg, "M{} {}", at.x, at.y).unwrap();
            }
            Line { to, .. } => {
                write!(svg, "L{} {}", to.x, to.y).unwrap();
            }
            Quadratic { ctrl, to, .. } => {
                write!(svg, "Q{} {} {} {}", ctrl.x, ctrl.y, to.x, to.y).unwrap();
            }
            Cubic {
                ctrl1, ctrl2, to, ..
            } => {
                write!(
                    svg,
                    "C{} {} {} {} {} {}",
                    ctrl1.x, ctrl1.y, ctrl2.x, ctrl2.y, to.x, to.y
                )
                .unwrap();
            }
            End { close: true, .. } => svg.push('Z'),
            End { .. } => {}
        }
    }
    if svg.is_empty() {
        Ok(vector::Path::default())
    } else {
        vector::Path::from_svg_data(&svg).map_err(invalid)
    }
}
impl Compositor {
    /// Install a caller-owned font database. Clears all source/composite caches.
    /// Hosts should load explicit font bytes for deterministic cross-machine output.
    pub fn set_text_renderer(&self, renderer: typography::TextRenderer) {
        *self.live.fonts.lock().unwrap_or_else(|e| e.into_inner()) = Some(renderer);
        self.clear();
    }
    pub(crate) fn live_tile(
        &self,
        layer: &Layer,
        canvas: Extent,
        depth: Depth,
        coord: TileCoord,
    ) -> EngineResult<Tile> {
        match &layer.kind {
            LayerKind::Text { model, transform } => {
                crate::text_vector::validate_text(model, *transform)?
            }
            LayerKind::Shape { model, transform } => {
                crate::text_vector::validate_shape(model, *transform)?
            }
            _ => {}
        }
        let bytes = match &layer.kind {
            LayerKind::Text { model, transform } => serde_json::to_vec(&("text", model, transform)),
            LayerKind::Shape { model, transform } => {
                serde_json::to_vec(&("shape", model, transform))
            }
            _ => return Err(invalid("expected text or shape")),
        }
        .map_err(invalid)?;
        let key = key(&bytes, canvas, depth, coord, false);
        if let Some(t) = self.live.cache.get(&key) {
            return Ok(t);
        }
        self.stats
            .live
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let view = viewport(canvas, coord);
        let n = (view.width * view.height) as usize;
        let renderer = vector::VectorRenderer { tolerance: 0.02 };
        let mut pixels = vec![[0.; 4]; n];
        match &layer.kind {
            LayerKind::Shape { model, transform } => {
                let path = model.path.affine(affine(*transform));
                if let Some(fill) = &model.fill {
                    pixels = renderer.rgba(&path, fill, view).map_err(invalid)?.data;
                }
                if let Some((stroke, fill)) = &model.stroke {
                    let outline = stroke
                        .outline(&model.path, 0.02)
                        .map_err(invalid)?
                        .affine(affine(*transform));
                    over(
                        &mut pixels,
                        &renderer.rgba(&outline, fill, view).map_err(invalid)?.data,
                    );
                }
            }
            LayerKind::Text { model, transform } => {
                let mut guard = self.live.fonts.lock().unwrap_or_else(|e| e.into_inner());
                let fonts = guard.get_or_insert_with(|| {
                    let mut f = typography::TextRenderer::new();
                    f.discover_system_fonts();
                    f
                });
                let layout = if let Some(path) = &model.path {
                    let contour =
                        typography::TextPath::new(&path.to_lyon().map_err(invalid)?, 0.01)
                            .map_err(invalid)?;
                    fonts
                        .layout_on_path(model, &contour, path.offset)
                        .map_err(invalid)?
                } else {
                    fonts.layout(model).map_err(invalid)?
                };
                for outline in fonts.outlines(model, &layout).map_err(invalid)? {
                    let path = glyph_path(&outline.path)?.affine(affine(*transform));
                    let color = outline.color.map(|v| f32::from(v) / 255.);
                    over(
                        &mut pixels,
                        &renderer
                            .rgba(&path, &vector::Fill::Solid(color), view)
                            .map_err(invalid)?
                            .data,
                    );
                }
            }
            _ => unreachable!(),
        }
        let mut planar = vec![0.; n * 4];
        for (i, p) in pixels.iter().enumerate() {
            for c in 0..3 {
                planar[c * n + i] = if p[3] > 0. { p[c] / p[3] } else { 0. };
            }
            planar[3 * n + i] = p[3];
        }
        let tile = crate::raster::tile_from_normalized(
            coord,
            TileLayout {
                extent: Extent::new(view.width, view.height),
                halo: 0,
                channels: 4,
            },
            depth,
            &planar,
        )?;
        self.live.cache.insert(key, tile.clone());
        Ok(tile)
    }
    pub(crate) fn vector_mask_tile(
        &self,
        layer: &Layer,
        canvas: Extent,
        depth: Depth,
        coord: TileCoord,
    ) -> EngineResult<Tile> {
        let mask = layer
            .vector_mask
            .as_ref()
            .ok_or_else(|| invalid("missing vector mask"))?;
        crate::text_vector::validate_mask(mask)?;
        let bytes = serde_json::to_vec(mask).map_err(invalid)?;
        let key = key(&bytes, canvas, depth, coord, true);
        if let Some(t) = self.live.cache.get(&key) {
            return Ok(t);
        }
        let view = viewport(canvas, coord);
        let sigma = f64::from(mask.feather) / view.pixel_size();
        let radius = (sigma * 3.).ceil() as u32;
        if radius > 1024 {
            return Err(EngineError::ResourceExhausted {
                resource: "vector-mask feather halo exceeds 1024 output pixels".into(),
            });
        }
        let halo = vector::Viewport {
            width: view.width + 2 * radius,
            height: view.height + 2 * radius,
            origin: view.origin
                - vector::Vec2::new(f64::from(radius), f64::from(radius)) * view.pixel_size(),
            ..view
        };
        let coverage = vector::VectorRenderer { tolerance: 0.02 }
            .coverage(&mask.path, halo)
            .map_err(invalid)?
            .data;
        let w = halo.width as usize;
        let h = halo.height as usize;
        let r = radius as usize;
        let mut filtered = coverage.clone();
        if radius > 0 {
            let mut kernel: Vec<f32> = (0..=2 * r)
                .map(|i| (-0.5 * ((i as f64 - r as f64) / sigma).powi(2)).exp() as f32)
                .collect();
            let sum: f32 = kernel.iter().sum();
            for k in &mut kernel {
                *k /= sum;
            }
            let mut temp = vec![0.; coverage.len()];
            for y in 0..h {
                for x in r..w - r {
                    temp[y * w + x] = (0..kernel.len())
                        .map(|k| coverage[y * w + x + k - r] * kernel[k])
                        .sum();
                }
            }
            for y in r..h - r {
                for x in r..w - r {
                    filtered[y * w + x] = (0..kernel.len())
                        .map(|k| temp[(y + k - r) * w + x] * kernel[k])
                        .sum();
                }
            }
        }
        let mut data = Vec::with_capacity((view.width * view.height) as usize);
        for y in r..h - r {
            for x in r..w - r {
                data.push(1. - mask.density * (1. - filtered[y * w + x]));
            }
        }
        // Keep mask evaluation in f32 until it is combined with an optional raster mask.
        let tile = Tile::from_samples(
            coord,
            TileLayout {
                extent: Extent::new(view.width, view.height),
                halo: 0,
                channels: 1,
            },
            data,
        )?;
        self.live.cache.insert(key, tile.clone());
        Ok(tile)
    }
    pub(crate) fn effective_vector_mask(
        &self,
        doc: DocRef<'_>,
        layer: &Layer,
        coord: TileCoord,
    ) -> EngineResult<Tile> {
        let bytes = serde_json::to_vec(&(
            "effective",
            &layer.vector_mask,
            layer.mask.as_ref().map(|m| {
                (
                    m.enabled,
                    m.density,
                    m.raster.default_value(),
                    m.raster.footprint_rev(coord.level, coord.x, coord.y),
                )
            }),
            doc.key,
            layer.id.0,
            layer.content_rev,
        ))
        .map_err(invalid)?;
        let key = key(&bytes, doc.state.canvas, doc.state.depth, coord, true);
        if let Some(tile) = self.live.cache.get(&key) {
            return Ok(tile);
        }
        let tile = self.vector_mask_tile(layer, doc.state.canvas, doc.state.depth, coord)?;
        let mut data = tile.samples::<f32>()?.to_vec();
        if let Some(mask) = layer.mask.as_ref().filter(|m| m.enabled) {
            let mut raster = vec![mask.raster.default_value(); data.len()];
            if let Some(t) =
                self.raster_level(doc.key, layer.id.0, Part::Mask, &mask.raster, coord)?
            {
                crate::raster::load_normalized(&t, &mut raster)?;
            }
            for (v, m) in data.iter_mut().zip(raster) {
                *v *= 1. - mask.density.clamp(0., 1.) * (1. - m);
            }
        }
        let tile =
            crate::raster::tile_from_normalized(coord, tile.layout(), doc.state.depth, &data)?;
        self.live.cache.insert(key, tile.clone());
        Ok(tile)
    }
}
/// Rasterize live source pixels at an output level without baking layer masks,
/// opacity or blending. The returned raster is straight RGBA F32.
pub fn rasterize_layer(layer: &Layer, canvas: Extent, level: u8) -> EngineResult<Raster> {
    if level > super::MAX_LEVEL {
        return Err(invalid("pyramid level exceeds limit"));
    }
    let comp = Compositor::new(0);
    let extent = canvas.at_level(level);
    let mut raster = Raster::new(extent, 4, Depth::F32, 0.);
    let (cols, rows) = extent.tile_grid(TILE_SIZE);
    for y in 0..rows {
        for x in 0..cols {
            let t = comp.live_tile(layer, canvas, Depth::F32, TileCoord::new(level, x, y))?;
            let t = Tile::from_samples(
                TileCoord::new(0, x, y),
                t.layout(),
                t.samples::<f32>()?.to_vec(),
            )?;
            raster.set_slot(x, y, Some(t), layer.content_rev)?;
        }
    }
    Ok(raster)
}
