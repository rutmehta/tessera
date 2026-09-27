//! Output-resolution rasterization of live geometry, shared by CPU and resident paths.
use super::{
    Compositor, DocRef,
    cache::{NodeKey, Part, RenderCache},
};
use crate::{Affine, Depth, Layer, LayerKind, Raster};
use crate::{Rect, text_vector::memo::Memo};
use engine_api::tile::{Extent, TILE_SIZE, Tile, TileCoord, TileLayout};
use engine_api::{EngineError, EngineResult};
use std::sync::{Arc, Mutex};

pub(super) struct LiveRuntime {
    fonts: Mutex<Option<typography::TextRenderer>>,
    cache: RenderCache,
    coverage: RenderCache,
    prepared: Mutex<Memo<Prepared>>,
    pub(super) frames: Mutex<Memo<super::live_damage::Snapshot>>,
}
impl LiveRuntime {
    pub fn new(budget: usize) -> Self {
        Self {
            fonts: Mutex::new(None),
            frames: Mutex::new(Memo::new(budget / 8)),
            cache: RenderCache::new(budget),
            coverage: RenderCache::new(budget / 2),
            prepared: Mutex::new(Memo::new((budget / 4).max(1 << 20))),
        }
    }
    pub fn clear(&self) {
        self.frames
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        self.cache.retain(|_| false);
        self.coverage.retain(|_| false);
        self.prepared
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }
}
pub(super) fn invalid(e: impl std::fmt::Display) -> EngineError {
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
pub(super) struct Primitive {
    path: vector::Path,
    fill: vector::Fill,
    pub bounds: Rect,
    pub identity: [u8; 32],
    coverage_key: [u8; 32],
}
pub(super) struct Prepared {
    pub items: Vec<Primitive>,
}
impl Primitive {
    fn new(path: vector::Path, fill: vector::Fill) -> EngineResult<Self> {
        path.validate().map_err(invalid)?;
        // Runtime-only identity: hash the exact coordinates rather than format
        // thousands of stroke vertices as decimal JSON on every pointer edit.
        let mut geometry = blake3::Hasher::new();
        geometry.update(&[match path.fill_rule {
            vector::FillRule::EvenOdd => 0,
            vector::FillRule::NonZero => 1,
        }]);
        for subpath in &path.subpaths {
            geometry.update(&(subpath.anchors.len() as u64).to_le_bytes());
            geometry.update(&[u8::from(subpath.closed)]);
            for a in &subpath.anchors {
                geometry.update(bytemuck::cast_slice(&[
                    a.point.x,
                    a.point.y,
                    a.incoming.x,
                    a.incoming.y,
                    a.outgoing.x,
                    a.outgoing.y,
                ]));
            }
        }
        let coverage_key = *geometry.finalize().as_bytes();
        let mut h = blake3::Hasher::new();
        h.update(&coverage_key);
        h.update(&serde_json::to_vec(&fill).map_err(invalid)?);
        let b = path.bounds();
        // Outward rounding and a document-pixel guard cover normalization's
        // finite precision. Empty contours never contribute.
        let bounds = if path.subpaths.is_empty() {
            Rect::default()
        } else {
            Rect::new(
                (b.x0.floor() as i64).saturating_sub(1),
                (b.y0.floor() as i64).saturating_sub(1),
                (b.x1.ceil() as i64).saturating_add(1),
                (b.y1.ceil() as i64).saturating_add(1),
            )
        };
        Ok(Self {
            path,
            fill,
            bounds,
            identity: *h.finalize().as_bytes(),
            coverage_key,
        })
    }
}
impl Prepared {
    pub fn identity(&self, rect: Rect) -> [u8; 32] {
        let mut h = blake3::Hasher::new();
        for p in &self.items {
            if p.bounds.intersects(&rect) {
                h.update(&p.identity);
            }
        }
        *h.finalize().as_bytes()
    }
}
fn key_for_coverage(p: &Primitive, canvas: Extent, coord: TileCoord) -> NodeKey {
    key(&p.coverage_key, canvas, Depth::F32, coord, false)
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
    pub(super) fn prepare_live(&self, layer: &Layer) -> EngineResult<Arc<Prepared>> {
        let bytes = match &layer.kind {
            LayerKind::Text { model, transform } => serde_json::to_vec(&("text", model, transform)),
            LayerKind::Shape { model, transform } => {
                serde_json::to_vec(&("shape", model, transform))
            }
            _ => return Err(invalid("expected text or shape")),
        }
        .map_err(invalid)?;
        let id = *blake3::hash(&bytes).as_bytes();
        let mut memo = self.live.prepared.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(p) = memo.get(&id) {
            return Ok(p);
        }
        self.stats
            .live_preparations
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut items = Vec::new();
        match &layer.kind {
            LayerKind::Shape { model, transform } => {
                crate::text_vector::validate_shape(model, *transform)?;
                if let Some(fill) = &model.fill {
                    items.push(Primitive::new(
                        model.path.affine(affine(*transform)),
                        fill.clone(),
                    )?);
                }
                if let Some((stroke, fill)) = &model.stroke {
                    items.push(Primitive::new(
                        stroke
                            .outline(&model.path, 0.02)
                            .map_err(invalid)?
                            .affine(affine(*transform)),
                        fill.clone(),
                    )?);
                }
            }
            LayerKind::Text { model, transform } => {
                crate::text_vector::validate_text(model, *transform)?;
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
                    items.push(Primitive::new(
                        glyph_path(&outline.path)?.affine(affine(*transform)),
                        vector::Fill::Solid(outline.color.map(|v| f32::from(v) / 255.)),
                    )?);
                }
            }
            _ => unreachable!(),
        }
        let size = items
            .iter()
            .map(|p| {
                p.path
                    .subpaths
                    .iter()
                    .map(|s| s.anchors.len() * std::mem::size_of::<vector::Anchor>())
                    .sum::<usize>()
                    + serde_json::to_vec(&p.fill).map_or(0, |b| b.len())
                    + std::mem::size_of::<Primitive>()
            })
            .sum::<usize>()
            + bytes.len();
        let prepared = Arc::new(Prepared { items });
        memo.insert(id, prepared.clone(), size);
        Ok(prepared)
    }
    pub(crate) fn live_intersects(
        &self,
        layer: &Layer,
        canvas: Extent,
        coord: TileCoord,
    ) -> EngineResult<bool> {
        let rect = Rect::of_tile(coord, canvas.at_level(coord.level)).to_level0(coord.level);
        Ok(self
            .prepare_live(layer)?
            .items
            .iter()
            .any(|p| p.bounds.intersects(&rect)))
    }
    pub(crate) fn live_tile(
        &self,
        layer: &Layer,
        canvas: Extent,
        depth: Depth,
        coord: TileCoord,
    ) -> EngineResult<Tile> {
        let prepared = self.prepare_live(layer)?;
        let tr = Rect::of_tile(coord, canvas.at_level(coord.level));
        let identity = prepared.identity(tr.to_level0(coord.level));
        let key = key(&identity, canvas, depth, coord, false);
        if let Some(t) = self.live.cache.get(&key) {
            return Ok(t);
        }
        let view = viewport(canvas, coord);
        let n = (view.width * view.height) as usize;
        let mut pixels = vec![[0.; 4]; n];
        let mut occupied = false;
        for p in &prepared.items {
            let rect = p
                .bounds
                .intersect(&tr.to_level0(coord.level))
                .to_level(coord.level);
            if rect.is_empty() {
                continue;
            }
            occupied = true;
            let ck = key_for_coverage(p, canvas, coord);
            let coverage = if let Some(t) = self.live.coverage.get(&ck) {
                t
            } else {
                self.stats
                    .live_coverages
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                // Retain the original tile origin for boolean normalization and
                // area arithmetic: changing origins can change rounding at seams.
                let full = vector::VectorRenderer { tolerance: 0.02 }
                    .coverage(&p.path, view)
                    .map_err(invalid)?;
                let mut data = Vec::with_capacity(rect.area() as usize);
                for y in rect.y0..rect.y1 {
                    let a = ((y - tr.y0) * i64::from(view.width) + rect.x0 - tr.x0) as usize;
                    data.extend_from_slice(&full.data[a..a + rect.width() as usize]);
                }
                let t = Tile::from_samples(
                    coord,
                    TileLayout {
                        extent: Extent::new(rect.width() as u32, rect.height() as u32),
                        halo: 0,
                        channels: 1,
                    },
                    data,
                )?;
                self.live.coverage.insert(ck, t.clone());
                t
            };
            let coverage = coverage.samples::<f32>()?;
            for y in rect.y0..rect.y1 {
                for x in rect.x0..rect.x1 {
                    let i = ((y - rect.y0) * rect.width() + x - rect.x0) as usize;
                    let color = p.fill.sample(vector::Point::new(
                        (x as f64 + 0.5) * view.pixel_size(),
                        (y as f64 + 0.5) * view.pixel_size(),
                    ));
                    let alpha = color[3] * coverage[i];
                    let src = [color[0] * alpha, color[1] * alpha, color[2] * alpha, alpha];
                    let dst =
                        &mut pixels[((y - tr.y0) * i64::from(view.width) + x - tr.x0) as usize];
                    for c in 0..4 {
                        dst[c] = src[c] + dst[c] * (1. - src[3]);
                    }
                }
            }
        }
        if occupied {
            self.stats
                .live
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
        if occupied {
            self.live.cache.insert(key, tile.clone());
        }
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

#[cfg(test)]
mod sparse_parity_tests {
    use super::*;
    fn fonts() -> typography::TextRenderer {
        let mut f = typography::TextRenderer::new();
        f.fonts_mut().load_font_data(
            include_bytes!("../../../typography/tests/fonts/NotoSans-Regular.ttf").to_vec(),
        );
        f
    }
    #[test]
    fn sparse_source_matches_legacy_full_tile_arithmetic() {
        let comp = Compositor::new(16 << 20);
        comp.set_text_renderer(fonts());
        let mut model = typography::TextModel::point("ffi AV e\u{301}", "Noto Sans", 83.);
        model.runs[0].color = [172, 23, 130, 177];
        model.warp.amount = 0.12;
        let text = Layer::new(
            "text",
            LayerKind::Text {
                model,
                transform: Affine {
                    m: [1.1, -0.21, 211.75, 0.12, 0.97, 19.25],
                },
            },
        );
        let shape = Layer::new(
            "shape",
            LayerKind::Shape {
                model: vector::ShapeModel {
                    path: vector::Shape::Ellipse {
                        center: vector::Point::new(250., 100.),
                        radii: vector::Vec2::new(130., 83.),
                    }
                    .path()
                    .unwrap(),
                    fill: Some(vector::Fill::Solid([0.8, 0.2, 0.1, 0.7])),
                    stroke: Some((
                        vector::Stroke {
                            width: 13.,
                            dashes: vec![21., 7.],
                            ..Default::default()
                        },
                        vector::Fill::Solid([0.1, 0.4, 0.9, 0.6]),
                    )),
                    ..Default::default()
                },
                transform: Affine {
                    m: [0.9, 0.2, 5.5, -0.1, 1.0, 20.75],
                },
            },
        );
        let canvas = Extent::new(531, 259);
        for layer in [text, shape] {
            let prepared = comp.prepare_live(&layer).unwrap();
            for level in [0, 1, 2, 3] {
                let (cols, rows) = canvas.at_level(level).tile_grid(TILE_SIZE);
                for y in 0..rows {
                    for x in 0..cols {
                        let coord = TileCoord::new(level, x, y);
                        let view = viewport(canvas, coord);
                        let n = (view.width * view.height) as usize;
                        let mut pixels = vec![[0.; 4]; n];
                        // Legacy algorithm: every outline across the complete tile,
                        // without culling, sparse coverage, or source memoization.
                        for p in &prepared.items {
                            let src = vector::VectorRenderer { tolerance: 0.02 }
                                .rgba(&p.path, &p.fill, view)
                                .unwrap();
                            for (d, s) in pixels.iter_mut().zip(src.data) {
                                for c in 0..4 {
                                    d[c] = s[c] + d[c] * (1. - s[3]);
                                }
                            }
                        }
                        let mut planar = vec![0.; n * 4];
                        for (i, p) in pixels.iter().enumerate() {
                            for c in 0..3 {
                                planar[c * n + i] = if p[3] > 0. { p[c] / p[3] } else { 0. };
                            }
                            planar[3 * n + i] = p[3];
                        }
                        for depth in [Depth::F32, Depth::U8, Depth::U16] {
                            let actual = comp.live_tile(&layer, canvas, depth, coord).unwrap();
                            let expected = crate::raster::tile_from_normalized(
                                coord,
                                actual.layout(),
                                depth,
                                &planar,
                            )
                            .unwrap();
                            let mut a = vec![0.; n * 4];
                            let mut b = a.clone();
                            crate::raster::load_normalized(&actual, &mut a).unwrap();
                            crate::raster::load_normalized(&expected, &mut b).unwrap();
                            assert_eq!(
                                a.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                                b.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                                "{coord:?} {depth:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}
