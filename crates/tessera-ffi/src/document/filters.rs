//! Filters, Image ▸ Adjustments and smart filters on a [`DocumentSession`]
//! (WP B5-05).
//!
//! # Calls
//!
//! - [`list_filters`]: the Filter menu, generated from `filters::registry`
//!   (id, group, name and a JSON parameter schema per filter).
//! - [`DocumentSession::preview_filter`]: renders the filter on the
//!   **viewport level** (and the visible region) on a worker thread and shows
//!   it in the presented surface. No history node; a newer preview cancels
//!   the running one (latest wins). [`DocumentSession::clear_preview`] ends it.
//! - [`DocumentSession::apply_filter`]: one history node. Destructive on
//!   pixel layers (inside the selection, if any); on smart objects the filter
//!   is appended to the layer's smart filter list (with the selection as its
//!   mask), and the pixels stay as they are.
//! - [`DocumentSession::set_smart_filter`] / `remove_smart_filter`: edit the
//!   list (enable, parameters, blend mode and opacity), one node each.
//! - [`DocumentSession::apply_adjustment`]: Image ▸ Adjustments on a pixel
//!   layer, with the same maths as the adjustment layer of that JSON.
//!
//! Filter JSON is `{"id":"gaussian_blur","params":{"radius":4}}`: `params`
//! uses the keys of the filter's schema, in its units; missing keys take the
//! schema defaults.
//!
//! # Presentation
//!
//! The compositor stores smart filters but does not render them, and a
//! preview is not part of the document. The session therefore renders a
//! *presented* document: the live one with (a) each smart object that has
//! enabled smart filters replaced by a baked pixel layer, (b) the previewed
//! layer replaced by a proxy pixel layer (the filtered viewport level, each
//! level pixel repeated `2^level` times, so the pyramid shows it exactly at
//! that level; outside the previewed region the layer's own tiles), and (c) for an
//! adjustment preview, the adjustment clipped directly above the layer. Bakes
//! run on the same worker at the viewport level and are refined at level 0
//! when the view is at 100 % or closer; export bakes at full resolution.

use super::{
    DocRect, DocumentSession, DocumentUpdate, Shared, blend_name, find, parse_blend,
    raster_from_rgba, tile_from_f32,
};
use crate::{Result, failure, surface::Surface};
use compositor::{
    Adjustment, Affine, BlendMode, Compositor, DocOp, DocState, Document, Layer, LayerId,
    LayerKind, PaintTarget, Raster, Rect, SmartFilter, SmartObject, TileDelta, blend::blend_pixel,
};
use engine_api::tile::{Extent, TILE_SIZE, TileCoord};
use filters::{
    Effect, Filter, FilterParams, Halo,
    registry::{self, ParamValue},
};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        Arc, Condvar, Mutex, MutexGuard, Weak,
        atomic::{AtomicBool, Ordering},
    },
};

// ─────────────────────────────── records ───────────────────────────────

/// One Filter menu entry (`filters::registry`).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct FilterInfo {
    /// Stable id used in filter JSON (`gaussian_blur`).
    pub id: String,
    /// `Blur`, `Sharpen`, `Noise`, `Distort`, `Stylize`, `Render` or `Other`.
    pub group: String,
    /// Menu title (`Gaussian Blur`).
    pub name: String,
    /// `{"params":[…]}`: controls with kind (`slider`, `angle`, `choice`,
    /// `point`, `toggle`), label, range, default, step, unit (see
    /// `filters::registry::FilterInfo::schema_json`).
    pub params_schema_json: String,
}

/// Every filter of the Filter menu, in menu order.
#[uniffi::export]
pub fn list_filters() -> Vec<FilterInfo> {
    registry::list()
        .into_iter()
        .map(|f| FilterInfo {
            id: f.id.into(),
            group: f.group.into(),
            name: f.name.into(),
            params_schema_json: f.schema_json(),
        })
        .collect()
}

/// One smart filter of a smart object, bottom (first applied) first.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SmartFilterRecord {
    pub index: u32,
    pub filter_id: String,
    /// Menu title of the filter.
    pub name: String,
    pub enabled: bool,
    /// `{"id":…,"params":{…}}` (the JSON `apply_filter` took).
    pub filter_json: String,
    /// Blending options: opacity 0…1 and a blend mode name.
    pub opacity: f32,
    pub blend_mode: String,
    /// A filter mask (from the selection when the filter was applied).
    pub has_mask: bool,
}

/// What [`DocumentSession::set_smart_filter`] changes.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum SmartFilterEdit {
    Enabled {
        enabled: bool,
    },
    /// New filter JSON (the same filter id).
    Params {
        filter_json: String,
    },
    /// Blending options (mode name as `LayerNode::blend_mode`, opacity 0…1).
    Blending {
        mode: String,
        opacity: f32,
    },
}

/// A 1:1 filter detail crop written into an RGBA8 IOSurface.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FilterDetail {
    pub surface_id: u32,
    pub width: u32,
    pub height: u32,
    /// Pyramid level the crop was filtered at: 0 (true 1:1) except for
    /// whole-image filters on large layers, which are filtered on a coarser
    /// level and enlarged.
    pub level: u8,
}

// ─────────────────────────────── filter specs ───────────────────────────────

/// A parsed, validated filter JSON.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Spec {
    id: String,
    values: BTreeMap<String, ParamValue>,
    /// Canonical `{"id","params"}` JSON.
    json: String,
}

impl Spec {
    fn parse(json: &str) -> Result<Self> {
        let v: serde_json::Value =
            serde_json::from_str(json).map_err(|e| failure(format!("filter JSON: {e}")))?;
        Self::from_value(&v)
    }

    fn from_value(v: &serde_json::Value) -> Result<Self> {
        let id = v
            .get("id")
            .and_then(|i| i.as_str())
            .ok_or_else(|| failure("filter JSON needs an \"id\""))?
            .to_owned();
        let mut values = BTreeMap::new();
        let params = v.get("params").cloned().unwrap_or(serde_json::json!({}));
        let obj = params
            .as_object()
            .ok_or_else(|| failure("filter \"params\" must be an object"))?;
        for (k, p) in obj {
            let value = match p {
                serde_json::Value::Number(n) => ParamValue::Number(n.as_f64().unwrap_or(f64::NAN)),
                serde_json::Value::Bool(b) => ParamValue::Bool(*b),
                serde_json::Value::String(s) => ParamValue::Text(s.clone()),
                serde_json::Value::Array(a) if a.len() == 2 => ParamValue::Point([
                    a[0].as_f64().unwrap_or(f64::NAN),
                    a[1].as_f64().unwrap_or(f64::NAN),
                ]),
                other => {
                    return Err(failure(format!(
                        "filter parameter {k}: unsupported {other}"
                    )));
                }
            };
            values.insert(k.clone(), value);
        }
        registry::build(&id, &values, 1.0)?;
        let json = serde_json::json!({ "id": id, "params": params }).to_string();
        Ok(Self { id, values, json })
    }

    fn name(&self) -> String {
        registry::find(&self.id).map_or_else(|| self.id.clone(), |f| f.name.to_owned())
    }

    /// Effect and parameters at pyramid `level`.
    fn at(&self, level: u8) -> Result<(Effect, FilterParams)> {
        Ok(registry::build(
            &self.id,
            &self.values,
            (1u32 << level) as f32,
        )?)
    }
}

/// One smart filter as evaluated (the compositor stores it as JSON).
#[derive(Clone, Debug, PartialEq)]
struct Node {
    spec: Spec,
    enabled: bool,
    opacity: f32,
    blend: BlendMode,
    /// Canvas-sized 8-bit grey PNG, base64.
    mask_png: Option<String>,
}

impl Node {
    fn new(spec: Spec) -> Self {
        Self {
            spec,
            enabled: true,
            opacity: 1.0,
            blend: BlendMode::Normal,
            mask_png: None,
        }
    }

    fn of(sf: &SmartFilter) -> Result<Self> {
        let p = &sf.params;
        let spec = Spec::from_value(
            p.get("filter")
                .unwrap_or(&serde_json::json!({ "id": sf.name })),
        )?;
        Ok(Self {
            spec,
            enabled: sf.enabled,
            opacity: p
                .get("opacity")
                .and_then(|v| v.as_f64())
                .map_or(1.0, |v| v.clamp(0.0, 1.0) as f32),
            blend: p
                .get("blend")
                .and_then(|v| v.as_str())
                .map(parse_blend)
                .transpose()?
                .unwrap_or(BlendMode::Normal),
            mask_png: p
                .get("mask_png")
                .and_then(|v| v.as_str())
                .map(str::to_owned),
        })
    }

    fn store(&self) -> SmartFilter {
        let filter: serde_json::Value =
            serde_json::from_str(&self.spec.json).unwrap_or(serde_json::Value::Null);
        let mut params = serde_json::json!({
            "filter": filter,
            "opacity": self.opacity,
            "blend": blend_name(self.blend),
        });
        if let Some(m) = &self.mask_png {
            params["mask_png"] = serde_json::Value::String(m.clone());
        }
        SmartFilter {
            blend: compositor::render::smart_filters::FilterBlend {
                mode: self.blend,
                opacity: self.opacity.clamp(0.0, 1.0),
            },
            name: self.spec.id.clone(),
            enabled: self.enabled,
            params,
        }
    }

    /// Everything that changes the result (the key of bakes).
    fn key(&self) -> String {
        format!(
            "{}|{}|{}|{:?}|{}",
            self.spec.json,
            self.enabled,
            self.opacity,
            self.blend,
            self.mask_png.as_ref().map_or(0, |m| {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                m.hash(&mut h);
                h.finish()
            })
        )
    }
}

fn nodes_of(so: &SmartObject) -> Result<Vec<Node>> {
    so.filters.iter().map(Node::of).collect()
}

// ─────────────────────────────── images ───────────────────────────────

/// Straight RGBA f32 pixels of `rect` (in the coordinates of one level).
#[derive(Clone, Debug)]
struct Img {
    rect: Rect,
    px: Vec<f32>,
}

impl Img {
    fn w(&self) -> usize {
        self.rect.width() as usize
    }
    fn h(&self) -> usize {
        self.rect.height() as usize
    }
    fn at(&self, x: usize, y: usize) -> &[f32] {
        let i = (y * self.w() + x) * 4;
        &self.px[i..i + 4]
    }
    /// The part of the image inside `r` (clamped to it).
    fn crop(&self, r: Rect) -> Img {
        let r = r.intersect(&self.rect);
        let (w, ox, oy) = (
            r.width() as usize,
            (r.x0 - self.rect.x0) as usize,
            (r.y0 - self.rect.y0) as usize,
        );
        let mut px = Vec::with_capacity(w * r.height() as usize * 4);
        for y in 0..r.height() as usize {
            let i = ((oy + y) * self.w() + ox) * 4;
            px.extend_from_slice(&self.px[i..i + w * 4]);
        }
        Img { rect: r, px }
    }
}

/// Normalized samples of `r` (level 0) of a raster, interleaved per pixel
/// (`channels` per pixel).
fn read_raster(raster: &Raster, r: Rect) -> Result<Vec<f32>> {
    let ch = raster.channels() as usize;
    let (w, h) = (r.width() as usize, r.height() as usize);
    let mut out = vec![raster.default_value(); w * h * ch];
    let ts = i64::from(TILE_SIZE);
    let full = Rect::of_extent(raster.extent());
    let r = r.intersect(&full);
    if r.is_empty() {
        return Ok(out);
    }
    let mut buf = Vec::new();
    for ty in r.y0 / ts..=(r.y1 - 1) / ts {
        for tx in r.x0 / ts..=(r.x1 - 1) / ts {
            let (tx32, ty32) = (tx as u32, ty as u32);
            raster.read_tile(tx32, ty32, &mut buf)?;
            let l = raster.layout(tx32, ty32);
            let (n, stride) = (l.plane_len(), l.stride());
            let tr = Rect::new(
                tx * ts,
                ty * ts,
                tx * ts + i64::from(l.extent.width),
                ty * ts + i64::from(l.extent.height),
            )
            .intersect(&r);
            for y in tr.y0..tr.y1 {
                for x in tr.x0..tr.x1 {
                    let src = (y - ty * ts) as usize * stride + (x - tx * ts) as usize;
                    let dst = ((y - r.y0) as usize * w + (x - r.x0) as usize) * ch;
                    for c in 0..ch {
                        out[dst + c] = buf[c * n + src];
                    }
                }
            }
        }
    }
    Ok(out)
}

/// Every tile of a raster as interleaved RGBA (raster channels ≥ 4).
fn raster_rgba(raster: &Raster) -> Result<Vec<f32>> {
    read_raster(raster, Rect::of_extent(raster.extent()))
}

/// A one-layer document with `layer`'s own content (Normal, opaque,
/// unmasked, unclipped), like the layer thumbnails.
/// `layer` with its smart filters removed. This session evaluates smart
/// filters itself (bakes); the compositor must only see the unfiltered child.
pub(crate) fn unfiltered(layer: &Layer) -> Layer {
    let mut l = layer.clone();
    if let LayerKind::SmartObject(so) = &mut l.kind {
        so.filters.clear();
        so.filter_mask = None;
    }
    l
}

fn solo(state: &DocState, layer: &Layer) -> Document {
    let mut l = unfiltered(layer);
    l.props.visible = true;
    l.props.opacity = 1.0;
    l.props.fill_opacity = 1.0;
    l.props.blend_mode = BlendMode::Normal;
    l.props.clipped = false;
    l.props.knockout = compositor::Knockout::None;
    l.props.background = false;
    l.props.blend_if = Default::default();
    l.mask = None;
    let mut s = DocState::new(state.canvas, state.depth);
    s.next_id = state.next_id;
    s.profile = state.profile.clone();
    s.root = vec![Arc::new(l)];
    Document::new(s)
}

fn threads() -> usize {
    std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .min(16)
}

/// The composite of `doc` inside `r` (coordinates of `level`), rendered
/// tile by tile in parallel through the CPU compositor.
fn render_region(comp: &Compositor, doc: &Document, level: u8, r: Rect) -> Result<Img> {
    let le = doc.state().canvas.at_level(level);
    let r = r.intersect(&Rect::of_extent(le));
    let (w, h) = (r.width().max(0) as usize, r.height().max(0) as usize);
    let mut px = vec![0.0f32; w * h * 4];
    if r.is_empty() {
        return Ok(Img { rect: r, px });
    }
    let ts = i64::from(TILE_SIZE);
    let coords: Vec<TileCoord> = (r.y0 / ts..=(r.y1 - 1) / ts)
        .flat_map(|ty| {
            (r.x0 / ts..=(r.x1 - 1) / ts).map(move |tx| TileCoord::new(level, tx as u32, ty as u32))
        })
        .collect();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let tiles = Mutex::new(Vec::with_capacity(coords.len()));
    let err = Mutex::new(None);
    std::thread::scope(|s| {
        for _ in 0..threads().min(coords.len()) {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(c) = coords.get(i) else { break };
                    match comp.render_tile(doc, *c) {
                        Ok(t) => tiles.lock().unwrap_or_else(|e| e.into_inner()).push(t),
                        Err(e) => {
                            *err.lock().unwrap_or_else(|e| e.into_inner()) = Some(e);
                            break;
                        }
                    }
                }
            });
        }
    });
    if let Some(e) = err.into_inner().unwrap_or_else(|e| e.into_inner()) {
        return Err(e.into());
    }
    for t in tiles.into_inner().unwrap_or_else(|e| e.into_inner()) {
        let (ox, oy) = t.coord().pixel_origin(TILE_SIZE);
        let l = t.layout();
        let n = l.plane_len();
        let s = t.samples::<f32>()?;
        for y in 0..l.extent.height as i64 {
            let gy = i64::from(oy) + y;
            if gy < r.y0 || gy >= r.y1 {
                continue;
            }
            for x in 0..l.extent.width as i64 {
                let gx = i64::from(ox) + x;
                if gx < r.x0 || gx >= r.x1 {
                    continue;
                }
                let i = y as usize * l.stride() + x as usize;
                let o = ((gy - r.y0) as usize * w + (gx - r.x0) as usize) * 4;
                for c in 0..4 {
                    px[o + c] = s[c * n + i];
                }
            }
        }
    }
    Ok(Img { rect: r, px })
}

// ─────────────────────────────── running filters ───────────────────────────────

/// Resampling filters average colours: they run on premultiplied pixels so
/// transparent neighbours do not darken edges.
fn premultiplied(e: Effect) -> bool {
    matches!(
        e,
        Effect::Gaussian
            | Effect::Box
            | Effect::Motion
            | Effect::RadialSpin
            | Effect::RadialZoom
            | Effect::Distort(_)
    )
}

/// `effect` over `img`. Neighbourhood filters run in parallel blocks with
/// real-neighbour halos (clamped at the image edge, as the crate clamps at
/// the canvas); whole-image filters run once over the image.
fn run_effect(effect: Effect, p: &FilterParams, img: &Img, cancel: &AtomicBool) -> Result<Img> {
    let (w, h) = (img.w(), img.h());
    if w == 0 || h == 0 {
        return Ok(img.clone());
    }
    let pre = premultiplied(effect);
    let prep = |px: &mut [f32]| {
        if pre {
            for q in px.as_chunks_mut::<4>().0 {
                for c in 0..3 {
                    q[c] *= q[3];
                }
            }
        }
    };
    let unprep = |px: &mut [f32]| {
        if pre {
            for q in px.as_chunks_mut::<4>().0 {
                if q[3] > 1e-6 {
                    for c in 0..3 {
                        q[c] /= q[3];
                    }
                } else {
                    q[..3].fill(0.0);
                }
            }
        }
    };
    let filter_block = |block: Rect| -> Result<Vec<f32>> {
        let mut src = img.crop(block);
        prep(&mut src.px);
        let e = Extent::new(src.w() as u32, src.h() as u32);
        let raster = raster_from_rgba(e, compositor::Depth::F32, &src.px, false)?;
        let out = effect.apply(&raster, p, cancel)?;
        let mut px = raster_rgba(&out)?;
        unprep(&mut px);
        Ok(px)
    };
    let halo = match effect.halo(p) {
        Halo::WholeImage => None,
        Halo::Radius(r) => Some(i64::from(r)),
    };
    let Some(halo) = halo else {
        let px = filter_block(img.rect)?;
        return Ok(Img { rect: img.rect, px });
    };
    let block = (halo * 2).clamp(256, 1024);
    let mut blocks = Vec::new();
    let mut y = img.rect.y0;
    while y < img.rect.y1 {
        let mut x = img.rect.x0;
        while x < img.rect.x1 {
            blocks.push(Rect::new(
                x,
                y,
                (x + block).min(img.rect.x1),
                (y + block).min(img.rect.y1),
            ));
            x += block;
        }
        y += block;
    }
    let mut out = img.clone();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let (tx, rx) = std::sync::mpsc::channel::<Result<(Rect, Rect, Vec<f32>)>>();
    std::thread::scope(|s| {
        for _ in 0..threads().min(blocks.len()) {
            let tx = tx.clone();
            let (next, blocks, filter_block) = (&next, &blocks, &filter_block);
            s.spawn(move || {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(b) = blocks.get(i) else { break };
                    let ext = b.inflate(halo).intersect(&img.rect);
                    let r = filter_block(ext).map(|px| (*b, ext, px));
                    let failed = r.is_err();
                    if tx.send(r).is_err() || failed {
                        break;
                    }
                }
            });
        }
        drop(tx);
        let w = out.w();
        for msg in rx.iter() {
            let (b, ext, px) = msg?;
            let ew = ext.width() as usize;
            for y in b.y0..b.y1 {
                let src = ((y - ext.y0) as usize * ew + (b.x0 - ext.x0) as usize) * 4;
                let dst = ((y - out.rect.y0) as usize * w + (b.x0 - out.rect.x0) as usize) * 4;
                let n = b.width() as usize * 4;
                out.px[dst..dst + n].copy_from_slice(&px[src..src + n]);
            }
        }
        Ok::<(), crate::BridgeError>(())
    })?;
    if cancel.load(Ordering::Relaxed) {
        return Err(failure("cancelled"));
    }
    Ok(out)
}

/// Halo (level pixels) the enabled nodes need around a region; `None` when
/// any is a whole-image operator.
fn stack_halo(nodes: &[Node], level: u8) -> Result<Option<i64>> {
    let mut sum = 0i64;
    for n in nodes.iter().filter(|n| n.enabled) {
        let (e, p) = n.spec.at(level)?;
        match e.halo(&p) {
            Halo::WholeImage => return Ok(None),
            Halo::Radius(r) => sum += i64::from(r),
        }
    }
    Ok(Some(sum))
}

/// The mask of a node over `r` at `level` (box-averaged), 0…1.
fn mask_region(png_b64: &str, canvas: Extent, level: u8, r: Rect) -> Result<Vec<f32>> {
    let bytes = base64_decode(png_b64).ok_or_else(|| failure("smart filter mask: bad base64"))?;
    let dec = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = dec.read_info().map_err(failure)?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(failure)?;
    if info.width != canvas.width
        || info.height != canvas.height
        || info.color_type != png::ColorType::Grayscale
        || info.bit_depth != png::BitDepth::Eight
    {
        return Err(failure("smart filter mask does not match the canvas"));
    }
    let (w, h) = (r.width() as usize, r.height() as usize);
    let d = 1i64 << level;
    let mut out = vec![1.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let (x0, y0) = ((r.x0 + x as i64) * d, (r.y0 + y as i64) * d);
            let (mut sum, mut n) = (0u32, 0u32);
            for yy in y0..(y0 + d).min(i64::from(canvas.height)) {
                for xx in x0..(x0 + d).min(i64::from(canvas.width)) {
                    sum += u32::from(buf[yy as usize * info.line_size + xx as usize]);
                    n += 1;
                }
            }
            if n > 0 {
                out[y * w + x] = sum as f32 / (n as f32 * 255.0);
            }
        }
    }
    Ok(out)
}

/// The enabled nodes over `src`, each blended onto its input with its
/// opacity, mode and mask.
fn eval_stack(
    src: Img,
    nodes: &[Node],
    level: u8,
    canvas: Extent,
    cancel: &AtomicBool,
) -> Result<Img> {
    let mut cur = src;
    for n in nodes.iter().filter(|n| n.enabled) {
        let (e, p) = n.spec.at(level)?;
        let f = run_effect(e, &p, &cur, cancel)?;
        let mask = n
            .mask_png
            .as_deref()
            .map(|m| mask_region(m, canvas, level, cur.rect))
            .transpose()?;
        if n.opacity >= 1.0 && n.blend == BlendMode::Normal && mask.is_none() {
            cur = f;
            continue;
        }
        for (i, (b, s)) in cur
            .px
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(f.px.as_chunks::<4>().0)
            .enumerate()
        {
            let k = n.opacity * mask.as_ref().map_or(1.0, |m| m[i]);
            let c = blend_pixel(n.blend, [b[0], b[1], b[2]], [s[0], s[1], s[2]]);
            for ch in 0..3 {
                b[ch] += k * (c[ch] - b[ch]);
            }
            b[3] += k * (s[3] - b[3]);
        }
    }
    Ok(cur)
}

/// A canvas-sized raster showing `img` (pixels of `level`), each level pixel
/// repeated `2^level` times; tiles outside `img` come from `under`. Tiles
/// are built in parallel.
fn upsampled(
    img: &Img,
    level: u8,
    canvas: Extent,
    depth: compositor::Depth,
    under: Option<&Raster>,
) -> Result<Raster> {
    let mut r = under
        .cloned()
        .unwrap_or_else(|| Raster::new(canvas, 4, depth, 0.0));
    let rev = r.max_rev() + 1;
    let area = img
        .rect
        .to_level0(level)
        .intersect(&Rect::of_extent(canvas));
    if area.is_empty() {
        return Ok(r);
    }
    let ts = i64::from(TILE_SIZE);
    let coords: Vec<(i64, i64)> = (area.y0 / ts..=(area.y1 - 1) / ts)
        .flat_map(|ty| (area.x0 / ts..=(area.x1 - 1) / ts).map(move |tx| (tx, ty)))
        .collect();
    let base = &r;
    let next = std::sync::atomic::AtomicUsize::new(0);
    let (tx_, rx) =
        std::sync::mpsc::channel::<Result<(u32, u32, Option<engine_api::tile::Tile>)>>();
    let results: Vec<_> = std::thread::scope(|s| {
        for _ in 0..threads().min(coords.len()) {
            let tx_ = tx_.clone();
            let (next, coords) = (&next, &coords);
            s.spawn(move || {
                let mut buf = Vec::new();
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(&(tx, ty)) = coords.get(i) else {
                        break;
                    };
                    let r = (|| -> Result<(u32, u32, Option<engine_api::tile::Tile>)> {
                        let (tx32, ty32) = (tx as u32, ty as u32);
                        let l = base.layout(tx32, ty32);
                        base.read_tile(tx32, ty32, &mut buf)?;
                        let n = l.plane_len();
                        let mut any = false;
                        for y in 0..l.extent.height as i64 {
                            let gy = ty * ts + y;
                            let inside_y = gy >= area.y0 && gy < area.y1;
                            let row = ((gy >> level) - img.rect.y0) as usize;
                            for x in 0..l.extent.width as i64 {
                                let gx = tx * ts + x;
                                let i = y as usize * l.stride() + x as usize;
                                if inside_y && gx >= area.x0 && gx < area.x1 {
                                    let p = img.at(((gx >> level) - img.rect.x0) as usize, row);
                                    for c in 0..4 {
                                        buf[c * n + i] = p[c];
                                    }
                                }
                                any |= buf[3 * n + i] != 0.0;
                            }
                        }
                        let tile = if any {
                            Some(tile_from_f32(
                                TileCoord::new(0, tx32, ty32),
                                l,
                                depth,
                                buf.clone(),
                            )?)
                        } else {
                            None
                        };
                        Ok((tx32, ty32, tile))
                    })();
                    if tx_.send(r).is_err() {
                        break;
                    }
                }
            });
        }
        drop(tx_);
        rx.iter().collect()
    });
    for res in results {
        let (tx, ty, tile) = res?;
        r.set_slot(tx, ty, tile, rev)?;
    }
    Ok(r)
}

// ─────────────────────────────── base64 ───────────────────────────────

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(data: &[u8]) -> String {
    let mut o = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= c.len() {
                o.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                o.push('=');
            }
        }
    }
    o
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut o = Vec::with_capacity(s.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for b in s.bytes() {
        if b == b'=' {
            break;
        }
        let v = B64.iter().position(|c| *c == b)? as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            o.push((acc >> bits & 0xff) as u8);
        }
    }
    Some(o)
}

/// The selection as a canvas-sized 8-bit grey PNG, base64.
fn mask_from_selection(sel: &Raster) -> Result<String> {
    let e = sel.extent();
    let v = read_raster(sel, Rect::of_extent(e))?;
    let bytes: Vec<u8> = v
        .iter()
        .map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8)
        .collect();
    let mut png_bytes = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut png_bytes, e.width, e.height);
        enc.set_color(png::ColorType::Grayscale);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        let mut w = enc.write_header().map_err(failure)?;
        w.write_image_data(&bytes).map_err(failure)?;
    }
    Ok(base64_encode(&png_bytes))
}

// ─────────────────────────────── worker state ───────────────────────────────

/// What a preview changes in the stack of the previewed layer.
#[derive(Clone, Debug)]
enum StackEdit {
    /// A new filter (on top of a smart object's list, or alone on pixels).
    Append(Spec),
    /// Re-editing smart filter `index`.
    Replace(usize, Spec),
}

struct PreviewJob {
    generation: u64,
    base: Arc<DocState>,
    layer: u64,
    edit: StackEdit,
    level: u8,
    /// Region of `level` to show.
    region: Rect,
}

#[derive(Clone)]
enum PreviewShown {
    /// The layer replaced by this proxy.
    Replace(u64, Arc<Raster>),
    /// An adjustment clipped directly above the layer.
    ClippedAdjustment(u64, Adjustment),
}

struct BakeJob {
    key: String,
    base: Arc<DocState>,
    layer: u64,
    level: u8,
    /// Level-0 region to refine (level 0 only).
    region: Option<Rect>,
}

/// A smart object's filtered pixels.
struct Baked {
    key: String,
    raster: Raster,
    /// Finest level computed over the whole canvas.
    level: u8,
    /// Level-0 regions computed at full resolution since.
    fine: Vec<Rect>,
}

#[derive(Default)]
struct Inner {
    stop: bool,
    busy: bool,
    generation: u64,
    preview_job: Option<PreviewJob>,
    running: Option<Arc<AtomicBool>>,
    preview: Option<(u64, PreviewShown)>,
    bake_jobs: BTreeMap<u64, BakeJob>,
    bakes: HashMap<u64, Baked>,
    /// Bake generation (part of the presented key).
    bake_serial: u64,
    /// Sources and stack prefixes, newest last.
    imgs: Vec<(String, Arc<Img>)>,
    presented: Option<(String, Arc<Document>)>,
    last_error: Option<String>,
}

struct Queue {
    m: Mutex<Inner>,
    cv: Condvar,
}

impl Queue {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.m.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// `(layer, index, max_px)` → `(mask key, surface)`.
type MaskThumbs = HashMap<(u64, usize, u32), (String, Arc<Surface>)>;

/// Per-session filter state (a field of [`Shared`]).
pub(crate) struct FilterState {
    q: Arc<Queue>,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
    comp: Arc<Compositor>,
    detail: Mutex<Option<Arc<Surface>>>,
    mask_thumbs: Mutex<MaskThumbs>,
    apply_cancel: Mutex<Arc<AtomicBool>>,
}

impl Default for FilterState {
    fn default() -> Self {
        Self {
            q: Arc::new(Queue {
                m: Mutex::new(Inner::default()),
                cv: Condvar::new(),
            }),
            worker: Mutex::new(None),
            comp: Arc::new(Compositor::new(256 << 20)),
            detail: Mutex::new(None),
            mask_thumbs: Mutex::new(HashMap::new()),
            apply_cancel: Mutex::new(Arc::new(AtomicBool::new(false))),
        }
    }
}

impl Drop for FilterState {
    fn drop(&mut self) {
        self.stop();
    }
}

impl FilterState {
    pub(crate) fn stop(&self) {
        {
            let mut i = self.q.lock();
            i.stop = true;
            if let Some(c) = &i.running {
                c.store(true, Ordering::Relaxed);
            }
        }
        self.q.cv.notify_all();
        if let Some(w) = self.worker.lock().unwrap_or_else(|e| e.into_inner()).take()
            && w.thread().id() != std::thread::current().id()
        {
            let _ = w.join();
        }
    }

    fn ensure_worker(&self, shared: &Arc<Shared>) {
        let mut w = self.worker.lock().unwrap_or_else(|e| e.into_inner());
        if w.is_some() || self.q.lock().stop {
            return;
        }
        let (q, comp, weak) = (self.q.clone(), self.comp.clone(), Arc::downgrade(shared));
        *w = std::thread::Builder::new()
            .name("document-filters".into())
            .spawn(move || worker_loop(q, comp, weak))
            .ok();
    }

    fn wait_idle(&self) {
        let mut i = self.q.lock();
        while !i.stop && (i.busy || i.preview_job.is_some() || !i.bake_jobs.is_empty()) {
            i = self.q.cv.wait(i).unwrap_or_else(|e| e.into_inner());
        }
    }
}

fn cached_img(q: &Queue, key: &str) -> Option<Arc<Img>> {
    q.lock()
        .imgs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, i)| i.clone())
}

fn store_img(q: &Queue, key: String, img: Arc<Img>) {
    let mut i = q.lock();
    i.imgs.retain(|(k, _)| *k != key);
    i.imgs.push((key, img));
    // Sources are up to a level's worth of f32 RGBA; keep a few.
    while i.imgs.len() > 4 {
        i.imgs.remove(0);
    }
}

/// What identifies a layer's own content for the source cache: pixels, text
/// proxy or the smart object's child and transform (not its filters).
fn content_key(l: &Layer) -> String {
    match &l.kind {
        LayerKind::SmartObject(so) => format!(
            "so{}:{:?}:{:?}",
            l.id.0,
            so.state
                .root
                .iter()
                .map(|c| (
                    Arc::as_ptr(c) as usize,
                    super::layer_revision(c),
                    c.props_rev
                ))
                .collect::<Vec<_>>(),
            so.transform.m
        ),
        _ => format!("l{}:{}", l.id.0, super::layer_revision(l)),
    }
}

/// The layer's own pixels (no filters) over `need` at `level`, cached.
fn source(
    q: &Queue,
    comp: &Compositor,
    base: &DocState,
    layer: &Layer,
    level: u8,
    need: Rect,
) -> Result<Arc<Img>> {
    let key = format!("src:{}:{level}:{need:?}", content_key(layer));
    if let Some(i) = cached_img(q, &key) {
        return Ok(i);
    }
    let img = Arc::new(render_region(comp, &solo(base, layer), level, need)?);
    store_img(q, key, img.clone());
    Ok(img)
}

/// Runs `nodes` over the layer's pixels for `region` (with the halo the
/// stack needs), cropped to `region`. Stack prefixes are cached so a
/// re-edited or appended filter does not recompute the filters below it.
#[allow(clippy::too_many_arguments)]
fn filtered(
    q: &Queue,
    comp: &Compositor,
    base: &DocState,
    layer: &Layer,
    nodes: &[Node],
    level: u8,
    region: Rect,
    cancel: &AtomicBool,
) -> Result<Img> {
    let full = Rect::of_extent(base.canvas.at_level(level));
    let region = region.intersect(&full);
    let need = match stack_halo(nodes, level)? {
        None => full,
        Some(h) => region.inflate(h).intersect(&full),
    };
    // The longest cached prefix of the stack.
    let prefix_key = |k: usize| {
        format!(
            "stack:{}:{level}:{need:?}:{}",
            content_key(layer),
            nodes[..k]
                .iter()
                .map(Node::key)
                .collect::<Vec<_>>()
                .join(";")
        )
    };
    let mut start = 0;
    let mut cur = None;
    for k in (1..=nodes.len()).rev() {
        if let Some(i) = cached_img(q, &prefix_key(k)) {
            start = k;
            cur = Some(i);
            break;
        }
    }
    let mut cur = match cur {
        Some(i) => (*i).clone(),
        None => (*source(q, comp, base, layer, level, need)?).clone(),
    };
    for k in start..nodes.len() {
        cur = eval_stack(cur, &nodes[k..=k], level, base.canvas, cancel)?;
        // Cache the prefix below the top filter (what previews re-run on).
        if k + 2 == nodes.len() {
            store_img(q, prefix_key(k + 1), Arc::new(cur.clone()));
        }
    }
    Ok(cur.crop(region))
}

/// The stack a preview evaluates.
fn edited_stack(layer: &Layer, edit: &StackEdit) -> Result<Vec<Node>> {
    let mut nodes = match &layer.kind {
        LayerKind::SmartObject(so) => nodes_of(so)?,
        LayerKind::Pixel(_) => Vec::new(),
        _ => return Err(failure("filters apply to pixel layers and smart objects")),
    };
    match edit {
        StackEdit::Append(s) => nodes.push(Node::new(s.clone())),
        StackEdit::Replace(i, s) => {
            let n = nodes
                .get_mut(*i)
                .ok_or_else(|| failure(format!("no smart filter {i}")))?;
            n.spec = s.clone();
            n.enabled = true;
        }
    }
    Ok(nodes)
}

fn worker_loop(q: Arc<Queue>, comp: Arc<Compositor>, shared: Weak<Shared>) {
    loop {
        enum Job {
            Preview(PreviewJob, Arc<AtomicBool>),
            Bake(BakeJob),
        }
        let job = {
            let mut i = q.lock();
            loop {
                if i.stop {
                    return;
                }
                if let Some(p) = i.preview_job.take() {
                    let cancel = Arc::new(AtomicBool::new(false));
                    i.running = Some(cancel.clone());
                    break Job::Preview(p, cancel);
                }
                if let Some(id) = i.bake_jobs.keys().next().copied() {
                    break Job::Bake(i.bake_jobs.remove(&id).expect("bake job"));
                }
                let (g, _) =
                    q.cv.wait_timeout(i, std::time::Duration::from_millis(500))
                        .unwrap_or_else(|e| e.into_inner());
                i = g;
                if shared.strong_count() == 0 {
                    return;
                }
            }
        };
        q.lock().busy = true;
        match job {
            Job::Preview(p, cancel) => {
                let result = find(&p.base, p.layer)
                    .map_err(|e| e.to_string())
                    .and_then(|layer| {
                        let nodes = edited_stack(layer, &p.edit).map_err(|e| e.to_string())?;
                        let img = filtered(
                            &q, &comp, &p.base, layer, &nodes, p.level, p.region, &cancel,
                        )
                        .map_err(|e| e.to_string())?;
                        // The layer's own pixels outside the region; the preview inside.
                        let under = match &layer.kind {
                            LayerKind::Pixel(r) => Some(r),
                            _ => None,
                        };
                        upsampled(&img, p.level, p.base.canvas, p.base.depth, under)
                            .map_err(|e| e.to_string())
                    });
                let mut i = q.lock();
                i.running = None;
                if i.generation == p.generation && !cancel.load(Ordering::Relaxed) {
                    match result {
                        Ok(raster) => {
                            i.preview = Some((
                                p.generation,
                                PreviewShown::Replace(p.layer, Arc::new(raster)),
                            ));
                            i.last_error = None;
                        }
                        Err(e) => i.last_error = Some(e),
                    }
                }
            }
            Job::Bake(b) => {
                let result = bake(&q, &comp, &b);
                let mut i = q.lock();
                match result {
                    Ok(Some(baked)) => {
                        i.bakes.insert(b.layer, baked);
                        i.bake_serial += 1;
                    }
                    Ok(None) => {}
                    Err(e) => i.last_error = Some(e.to_string()),
                }
            }
        }
        {
            let mut i = q.lock();
            i.busy = false;
        }
        q.cv.notify_all();
        if let Some(s) = shared.upgrade() {
            s.render.request(Vec::new(), false, 0);
        }
    }
}

fn bake(q: &Queue, comp: &Compositor, b: &BakeJob) -> Result<Option<Baked>> {
    let layer = find(&b.base, b.layer)?;
    let LayerKind::SmartObject(so) = &layer.kind else {
        return Ok(None);
    };
    let nodes = nodes_of(so)?;
    let cancel = AtomicBool::new(false);
    let canvas = b.base.canvas;
    let previous = {
        let i = q.lock();
        i.bakes
            .get(&b.layer)
            .filter(|p| p.key == b.key)
            .map(|p| (p.raster.clone(), p.level, p.fine.clone()))
    };
    match b.region {
        Some(r0) => {
            let img = filtered(q, comp, &b.base, layer, &nodes, 0, r0, &cancel)?;
            let (under, level, mut fine) =
                previous.map_or((None, 8, Vec::new()), |(r, l, f)| (Some(r), l, f));
            let raster = upsampled(&img, 0, canvas, b.base.depth, under.as_ref())?;
            fine.push(img.rect);
            Ok(Some(Baked {
                key: b.key.clone(),
                raster,
                level,
                fine,
            }))
        }
        None => {
            let full = Rect::of_extent(canvas.at_level(b.level));
            let img = filtered(q, comp, &b.base, layer, &nodes, b.level, full, &cancel)?;
            Ok(Some(Baked {
                key: b.key.clone(),
                raster: upsampled(&img, b.level, canvas, b.base.depth, None)?,
                level: b.level,
                fine: Vec::new(),
            }))
        }
    }
}

/// Smart objects with at least one enabled smart filter, anywhere in the tree.
fn filtered_smart_objects(state: &DocState) -> Vec<&Layer> {
    fn go<'a>(v: &'a [Arc<Layer>], out: &mut Vec<&'a Layer>) {
        for l in v {
            match &l.kind {
                LayerKind::SmartObject(so) if so.filters.iter().any(|f| f.enabled) => out.push(l),
                LayerKind::Group { children, .. } => go(children, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    go(&state.root, &mut out);
    out
}

fn bake_key(l: &Layer) -> String {
    match &l.kind {
        LayerKind::SmartObject(so) => format!(
            "{}|{}",
            content_key(l),
            so.filters
                .iter()
                .map(|f| Node::of(f).map(|n| n.key()).unwrap_or_default())
                .collect::<Vec<_>>()
                .join(";")
        ),
        _ => String::new(),
    }
}

/// The level-0 region a level-0 bake refines for a view of `view` (level 0):
/// the view plus a margin, snapped to 512-pixel blocks so small pans reuse it.
fn fine_region(view: Rect, canvas: Extent) -> Rect {
    let b = 512;
    let v = view.inflate(b);
    Rect::new(
        v.x0.div_euclid(b) * b,
        v.y0.div_euclid(b) * b,
        (v.x1 + b - 1).div_euclid(b) * b,
        (v.y1 + b - 1).div_euclid(b) * b,
    )
    .intersect(&Rect::of_extent(canvas))
}

/// A copy of `state` with layers replaced and an adjustment inserted.
fn substitute(
    state: &DocState,
    subs: &[(u64, Layer)],
    insert_above: Option<(u64, Layer)>,
) -> DocState {
    let mut s = state.clone();
    for (id, l) in subs {
        s.layer_mut(LayerId(*id), |x| *x = l.clone());
    }
    if let Some((above, adj)) = insert_above
        && let Some((parent, i)) = s.locate(LayerId(above))
    {
        let adj = Arc::new(adj);
        match parent {
            None => s.root.insert(i + 1, adj),
            Some(p) => {
                s.layer_mut(p, |g| {
                    if let LayerKind::Group { children, .. } = &mut g.kind {
                        children.insert(i + 1, adj);
                    }
                });
            }
        }
    }
    s
}

/// The document to present for `live` at `level` showing `view` (level
/// coordinates): smart filters baked, the preview shown. `None` when that is
/// the live document itself. Schedules missing bakes on the worker.
pub(crate) fn presented(
    shared: &Arc<Shared>,
    live: &Document,
    level: u8,
    view: Rect,
) -> Option<Arc<Document>> {
    let fs = &shared.filters;
    let state = live.state();
    let sos = filtered_smart_objects(state);
    let mut i = fs.q.lock();
    if sos.is_empty() && i.preview.is_none() {
        i.presented = None;
        return None;
    }
    let mut subs = Vec::new();
    let mut key = format!("{:p}|{}|", Arc::as_ptr(state), i.bake_serial);
    let mut spawn = false;
    for l in &sos {
        let id = l.id.0;
        let bk = bake_key(l);
        let wanted_region = (level == 0).then(|| fine_region(view.to_level0(level), state.canvas));
        let entry = i.bakes.get(&id);
        let fresh = entry.is_some_and(|b| b.key == bk);
        let good = entry.is_some_and(|b| {
            b.key == bk
                && (b.level <= level
                    || wanted_region.is_some_and(|w| b.fine.iter().any(|f| f.intersect(&w) == w)))
        });
        if !good {
            // A full bake at this level first; level 0 then refines the view.
            let job_level = if level == 0 && fresh { 0 } else { level.max(1) };
            let job = BakeJob {
                key: bk.clone(),
                base: state.clone(),
                layer: id,
                level: job_level,
                region: (job_level == 0).then_some(wanted_region).flatten(),
            };
            let queued = i.bake_jobs.get(&id).is_some_and(|j| {
                j.key == job.key && j.level == job.level && j.region == job.region
            });
            if !queued {
                i.bake_jobs.insert(id, job);
                spawn = true;
            }
        }
        if let Some(b) = i.bakes.get(&id) {
            let mut nl = (*l).clone();
            nl.kind = LayerKind::Pixel(b.raster.clone());
            key.push_str(&format!(
                "{id}:{:p}:{}:{};",
                Arc::as_ptr(&so_arc(l)),
                b.level,
                b.fine.len()
            ));
            subs.push((id, nl));
        } else {
            // Not baked yet: show the unfiltered contents until the bake lands.
            key.push_str(&format!("{id}:u;"));
            subs.push((id, unfiltered(l)));
        }
    }
    let mut insert = None;
    if let Some((generation, shown)) = &i.preview {
        match shown {
            PreviewShown::Replace(id, so) => {
                if let Some(l) = state.find(LayerId(*id)) {
                    let mut nl = l.clone();
                    nl.kind = LayerKind::Pixel((**so).clone());
                    subs.retain(|(s, _)| s != id);
                    subs.push((*id, nl));
                    key.push_str(&format!("p{generation}:{:p}", Arc::as_ptr(so)));
                }
            }
            PreviewShown::ClippedAdjustment(id, adj) => {
                if state.find(LayerId(*id)).is_some() {
                    let mut l = Layer::new("preview", LayerKind::Adjustment(adj.clone()));
                    l.id = LayerId(state.next_id);
                    l.props.clipped = true;
                    insert = Some((*id, l));
                    key.push_str(&format!("a{generation}"));
                }
            }
        }
    }
    if spawn {
        drop(i);
        fs.q.cv.notify_all();
        fs.ensure_worker(shared);
        i = fs.q.lock();
    }
    if subs.is_empty() && insert.is_none() {
        i.presented = None;
        return None;
    }
    if let Some((k, d)) = &i.presented
        && *k == key
    {
        return Some(d.clone());
    }
    let mut s = substitute(state, &subs, insert);
    s.rev = state.rev;
    let doc = Arc::new(Document::new(s));
    i.presented = Some((key, doc.clone()));
    Some(doc)
}

/// `state` with every smart filter removed (quick previews such as thumbnails).
pub(crate) fn unfiltered_state(state: &DocState) -> DocState {
    fn go(v: &[Arc<Layer>]) -> Vec<Arc<Layer>> {
        v.iter()
            .map(|l| match &l.kind {
                LayerKind::SmartObject(so) if !so.filters.is_empty() => Arc::new(unfiltered(l)),
                LayerKind::Group { children, mode } => {
                    let mut g = (**l).clone();
                    g.kind = LayerKind::Group {
                        mode: *mode,
                        children: go(children),
                    };
                    Arc::new(g)
                }
                _ => l.clone(),
            })
            .collect()
    }
    let mut s = state.clone();
    s.root = go(&state.root);
    s
}

/// Identity of a layer's smart object state (presented-cache key part).
fn so_arc(l: &Layer) -> Arc<DocState> {
    match &l.kind {
        LayerKind::SmartObject(so) => so.state.clone(),
        _ => Arc::new(DocState::new(Extent::new(1, 1), compositor::Depth::U8)),
    }
}

/// `doc` with every smart filter baked at full resolution (export, flatten).
pub(crate) fn for_output(doc: Document) -> Result<Document> {
    let state = doc.state().clone();
    let sos = filtered_smart_objects(&state);
    if sos.is_empty() {
        return Ok(doc);
    }
    let comp = Compositor::new(128 << 20);
    let q = Queue {
        m: Mutex::new(Inner::default()),
        cv: Condvar::new(),
    };
    let cancel = AtomicBool::new(false);
    let mut subs = Vec::new();
    for l in sos {
        let LayerKind::SmartObject(so) = &l.kind else {
            continue;
        };
        let nodes = nodes_of(so)?;
        let img = filtered(
            &q,
            &comp,
            &state,
            l,
            &nodes,
            0,
            Rect::of_extent(state.canvas),
            &cancel,
        )?;
        let mut nl = l.clone();
        nl.kind = LayerKind::Pixel(upsampled(&img, 0, state.canvas, state.depth, None)?);
        subs.push((l.id.0, nl));
    }
    let mut s = substitute(&state, &subs, None);
    s.rev = state.rev;
    Ok(Document::new(s))
}

// ─────────────────────────────── session calls ───────────────────────────────

/// The level and level region a preview renders for the current view.
fn preview_view(st: &super::State, region: Option<DocRect>) -> (u8, Rect) {
    let canvas = st.live().state().canvas;
    let level = match (st.view.viewport, st.view.surfaces.first()) {
        (Some(v), _) => v.level,
        (None, Some(s)) => (0..super::render::MAX_VIEW_LEVEL)
            .find(|&l| {
                let e = canvas.at_level(l);
                e.width <= s.width() && e.height <= s.height()
            })
            .unwrap_or(super::render::MAX_VIEW_LEVEL - 1),
        // No viewport: a level of at most ~2 MP.
        (None, None) => (0..super::render::MAX_VIEW_LEVEL)
            .find(|&l| canvas.at_level(l).area() <= 2_000_000)
            .unwrap_or(super::render::MAX_VIEW_LEVEL - 1),
    };
    let full = Rect::of_extent(canvas.at_level(level));
    let r = match region {
        Some(r) => Rect::new(r.x, r.y, r.x + r.width, r.y + r.height)
            .to_level(level)
            .intersect(&full),
        None => full,
    };
    (level, if r.is_empty() { full } else { r })
}

impl DocumentSession {
    fn filter_target(&self, layer: u64) -> Result<(Arc<DocState>, Option<Arc<Raster>>)> {
        let st = self.shared.lock()?;
        st.open()?;
        let s = st.live().state().clone();
        let l = find(&s, layer)?;
        if !matches!(l.kind, LayerKind::Pixel(_) | LayerKind::SmartObject(_)) {
            return Err(failure("filters apply to pixel layers and smart objects"));
        }
        let sel = s.selection.clone();
        Ok((s, sel))
    }

    fn submit_preview(&self, layer: u64, edit: StackEdit, region: Option<DocRect>) -> Result<()> {
        let (base, level, region) = {
            let st = self.shared.lock()?;
            st.open()?;
            let s = st.live().state().clone();
            let l = find(&s, layer)?;
            edited_stack(l, &edit)?;
            let (level, r) = preview_view(&st, region);
            (s, level, r)
        };
        let fs = &self.shared.filters;
        {
            let mut i = fs.q.lock();
            i.generation += 1;
            if let Some(c) = &i.running {
                c.store(true, Ordering::Relaxed);
            }
            let generation = i.generation;
            i.preview_job = Some(PreviewJob {
                generation,
                base,
                layer,
                edit,
                level,
                region,
            });
        }
        fs.q.cv.notify_all();
        fs.ensure_worker(&self.shared);
        Ok(())
    }

    /// Writes pixels of `img` (level 0) into the pixel layer as one paint
    /// node, blended by the selection and keeping alpha under a
    /// transparency lock. Fails when the layer changed since `base`.
    fn write_pixels(
        &self,
        base: &DocState,
        layer: u64,
        img: &Img,
        label: &str,
    ) -> Result<DocumentUpdate> {
        let l = find(base, layer)?;
        let LayerKind::Pixel(raster) = &l.kind else {
            return Err(failure("not a pixel layer"));
        };
        let depth = base.depth;
        let r = img.rect;
        let src = read_raster(raster, r)?;
        let sel = base
            .selection
            .as_ref()
            .map(|s| read_raster(s, r))
            .transpose()?;
        let keep_alpha = l.props.locks.transparency;
        let ts = i64::from(TILE_SIZE);
        let mut tiles = Vec::new();
        let mut buf = Vec::new();
        let w = r.width() as usize;
        for ty in r.y0 / ts..=(r.y1 - 1) / ts {
            for tx in r.x0 / ts..=(r.x1 - 1) / ts {
                let (tx32, ty32) = (tx as u32, ty as u32);
                let lay = raster.layout(tx32, ty32);
                raster.read_tile(tx32, ty32, &mut buf)?;
                let n = lay.plane_len();
                let mut any = false;
                for y in 0..lay.extent.height as i64 {
                    let gy = ty * ts + y;
                    for x in 0..lay.extent.width as i64 {
                        let gx = tx * ts + x;
                        let i = y as usize * lay.stride() + x as usize;
                        if gx >= r.x0 && gx < r.x1 && gy >= r.y0 && gy < r.y1 {
                            let j = (gy - r.y0) as usize * w + (gx - r.x0) as usize;
                            let k = sel.as_ref().map_or(1.0, |s| s[j].clamp(0.0, 1.0));
                            for c in 0..4 {
                                let (a, b) = (src[j * 4 + c], img.px[j * 4 + c]);
                                buf[c * n + i] = if c == 3 && keep_alpha {
                                    a
                                } else {
                                    a + k * (b - a)
                                };
                            }
                        }
                        any |= buf[3 * n + i] != 0.0;
                    }
                }
                let tile = if any || raster.tile(tx32, ty32).is_some() {
                    Some(tile_from_f32(
                        TileCoord::new(0, tx32, ty32),
                        lay,
                        depth,
                        buf.clone(),
                    )?)
                } else {
                    None
                };
                tiles.push(TileDelta {
                    tx: tx32,
                    ty: ty32,
                    tile,
                });
            }
        }
        let op = DocOp::PaintTiles {
            id: LayerId(layer),
            target: PaintTarget::Content,
            tiles,
            dirty: r,
        };
        self.edit_checked(layer, super::layer_revision(l), op, label)
    }

    /// One history node, provided the layer's content is still `revision`.
    fn edit_checked(
        &self,
        layer: u64,
        revision: u64,
        op: DocOp,
        label: &str,
    ) -> Result<DocumentUpdate> {
        self.clear_preview_state();
        let mut st = self.shared.lock()?;
        st.open()?;
        let before = st.live().state().clone();
        self.commit_pending(&mut st, None)?;
        let now = super::layer_revision(find(st.doc.state(), layer)?);
        if now != revision {
            return Err(failure(
                "the layer changed while the filter ran; apply it again",
            ));
        }
        let applied = st.doc.apply(op)?;
        st.labels.insert(applied.node, label.to_owned());
        Ok(self.update(&mut st, &before, Some(&applied), true))
    }

    fn clear_preview_state(&self) -> bool {
        let fs = &self.shared.filters;
        let mut i = fs.q.lock();
        i.generation += 1;
        i.preview_job = None;
        if let Some(c) = &i.running {
            c.store(true, Ordering::Relaxed);
        }
        i.preview.take().is_some()
    }

    /// Replaces a smart object's filter list (one node).
    fn set_nodes(
        &self,
        layer: u64,
        label: &str,
        f: impl FnOnce(&mut Vec<Node>) -> Result<()>,
    ) -> Result<DocumentUpdate> {
        self.clear_preview_state();
        let mut st = self.shared.lock()?;
        st.open()?;
        let before = st.live().state().clone();
        self.commit_pending(&mut st, None)?;
        let s = st.doc.state().clone();
        let l = find(&s, layer)?;
        let LayerKind::SmartObject(so) = &l.kind else {
            return Err(failure(format!("layer {layer} is not a smart object")));
        };
        let mut nodes = nodes_of(so)?;
        f(&mut nodes)?;
        let mut nl = l.clone();
        if let LayerKind::SmartObject(so) = &mut nl.kind {
            so.filters = nodes.iter().map(Node::store).collect();
        }
        let (parent, index) = s
            .locate(LayerId(layer))
            .ok_or_else(|| failure("layer not found"))?;
        let applied = st.doc.apply(DocOp::Batch(vec![
            DocOp::RemoveLayer { id: LayerId(layer) },
            DocOp::AddLayer {
                parent,
                index,
                layer: nl,
            },
        ]))?;
        st.labels.insert(applied.node, label.to_owned());
        let mut u = self.update(&mut st, &before, Some(&applied), true);
        // The layer was replaced in place, not created.
        u.created.clear();
        Ok(u)
    }
}

#[uniffi::export]
impl DocumentSession {
    /// Shows `filter_json` applied to `layer` in the viewport, rendered on
    /// the viewport's pyramid level over `region` (level-0 canvas pixels;
    /// `None`: the whole canvas) on a worker thread. No history node. A newer
    /// preview cancels this one; the frame arrives through the listener.
    /// On a smart object the filter is previewed on top of its smart filters.
    pub fn preview_filter(
        &self,
        layer: u64,
        filter_json: String,
        region: Option<DocRect>,
    ) -> Result<()> {
        let spec = Spec::parse(&filter_json)?;
        self.submit_preview(layer, StackEdit::Append(spec), region)
    }

    /// Like `preview_filter`, re-editing smart filter `index` of a smart object.
    pub fn preview_smart_filter(
        &self,
        layer: u64,
        index: u32,
        filter_json: String,
        region: Option<DocRect>,
    ) -> Result<()> {
        let spec = Spec::parse(&filter_json)?;
        self.submit_preview(layer, StackEdit::Replace(index as usize, spec), region)
    }

    /// Shows an Image ▸ Adjustments result (`compositor::Adjustment` JSON)
    /// on a pixel layer, live on the GPU (the adjustment clipped to the
    /// layer). No history node.
    pub fn preview_adjustment(&self, layer: u64, adjustment_json: String) -> Result<()> {
        let adj: Adjustment = serde_json::from_str(&adjustment_json)
            .map_err(|e| failure(format!("adjustment JSON: {e}")))?;
        {
            let st = self.shared.lock()?;
            st.open()?;
            if !matches!(find(st.live().state(), layer)?.kind, LayerKind::Pixel(_)) {
                return Err(failure("Image ▸ Adjustments apply to pixel layers"));
            }
        }
        let fs = &self.shared.filters;
        {
            let mut i = fs.q.lock();
            i.generation += 1;
            i.preview_job = None;
            if let Some(c) = &i.running {
                c.store(true, Ordering::Relaxed);
            }
            let g = i.generation;
            i.preview = Some((g, PreviewShown::ClippedAdjustment(layer, adj)));
        }
        let st = self.shared.lock()?;
        self.shared.render.request(Vec::new(), false, st.epoch);
        Ok(())
    }

    /// Ends a filter or adjustment preview (the viewport shows the document).
    pub fn clear_preview(&self) -> Result<()> {
        if self.clear_preview_state() {
            let st = self.shared.lock()?;
            self.shared.render.request(Vec::new(), false, st.epoch);
        }
        Ok(())
    }

    /// The last preview or smart filter render error (cleared by a good one).
    pub fn filter_error(&self) -> Option<String> {
        self.shared.filters.q.lock().last_error.clone()
    }

    /// Cancels a running `apply_filter` / `apply_adjustment` and the preview.
    pub fn cancel_filter(&self) {
        let fs = &self.shared.filters;
        fs.apply_cancel
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .store(true, Ordering::Relaxed);
        if let Some(c) = &fs.q.lock().running {
            c.store(true, Ordering::Relaxed);
        }
    }

    /// `filter_json` on the layer's own pixels (on a smart object: after its
    /// smart filters) over `width × height` level-0 pixels at `(x, y)`,
    /// written into an RGBA8 IOSurface (straight alpha) the session retains
    /// until the next call: the filter dialog's 1:1 detail pane. Blocking.
    pub fn filter_detail(
        &self,
        layer: u64,
        filter_json: String,
        x: i64,
        y: i64,
        width: u32,
        height: u32,
    ) -> Result<FilterDetail> {
        let spec = Spec::parse(&filter_json)?;
        if width == 0 || height == 0 || width > 4096 || height > 4096 {
            return Err(failure("detail size must be 1…4096 pixels"));
        }
        let (base, _) = self.filter_target(layer)?;
        let l = find(&base, layer)?;
        let nodes = edited_stack(l, &StackEdit::Append(spec))?;
        let r0 = Rect::new(x, y, x + i64::from(width), y + i64::from(height));
        // Whole-image filters are filtered on a level of at most ~4 MP.
        let level = if stack_halo(&nodes, 0)?.is_none() {
            (0..super::render::MAX_VIEW_LEVEL)
                .find(|&lv| base.canvas.at_level(lv).area() <= 4_000_000)
                .unwrap_or(0)
        } else {
            0
        };
        let fs = &self.shared.filters;
        let cancel = AtomicBool::new(false);
        let img = filtered(
            &fs.q,
            &fs.comp,
            &base,
            l,
            &nodes,
            level,
            r0.to_level(level),
            &cancel,
        )?;
        let surface = Surface::create_rgba8(width, height).map_err(failure)?;
        surface
            .with_pixels(|px, stride| {
                for yy in 0..height as i64 {
                    for xx in 0..width as i64 {
                        let o = yy as usize * stride + xx as usize * 4;
                        let (gx, gy) = ((x + xx) >> level, (y + yy) >> level);
                        let p = if img.rect.x0 <= gx
                            && gx < img.rect.x1
                            && img.rect.y0 <= gy
                            && gy < img.rect.y1
                        {
                            let q =
                                img.at((gx - img.rect.x0) as usize, (gy - img.rect.y0) as usize);
                            [q[0], q[1], q[2], q[3]]
                        } else {
                            [0.0; 4]
                        };
                        for c in 0..4 {
                            px[o + c] = (p[c].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                        }
                    }
                }
            })
            .map_err(failure)?;
        let id = surface.id();
        *fs.detail.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(surface));
        Ok(FilterDetail {
            surface_id: id,
            width,
            height,
            level,
        })
    }

    /// Applies `filter_json` to `layer` as one history node (labelled with
    /// the filter's name). Pixel layers are filtered at full resolution
    /// inside the selection (all of it without one); smart objects get the
    /// filter appended to their smart filters, masked by the selection.
    /// Blocking (seconds on large layers): call off the main thread;
    /// `cancel_filter` stops it.
    pub fn apply_filter(&self, layer: u64, filter_json: String) -> Result<DocumentUpdate> {
        let spec = Spec::parse(&filter_json)?;
        let (base, sel) = self.filter_target(layer)?;
        let l = find(&base, layer)?;
        if l.props.locks.pixels || l.props.locks.all {
            return Err(failure("the layer's pixels are locked"));
        }
        let name = spec.name();
        if let LayerKind::SmartObject(_) = l.kind {
            let mask = sel.as_deref().map(mask_from_selection).transpose()?;
            return self.set_nodes(layer, &name, move |nodes| {
                let mut n = Node::new(spec);
                n.mask_png = mask;
                nodes.push(n);
                Ok(())
            });
        }
        let LayerKind::Pixel(raster) = &l.kind else {
            return Err(failure("filters apply to pixel layers and smart objects"));
        };
        let cancel = {
            let c = Arc::new(AtomicBool::new(false));
            *self
                .shared
                .filters
                .apply_cancel
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = c.clone();
            c
        };
        let full = Rect::of_extent(base.canvas);
        let region = sel
            .as_deref()
            .and_then(super::io::selection_bounds)
            .unwrap_or(full)
            .intersect(&full);
        if region.is_empty() {
            return Err(failure("the selection is empty"));
        }
        let (effect, params) = spec.at(0)?;
        let need = match effect.halo(&params) {
            Halo::WholeImage => full,
            Halo::Radius(h) => region.inflate(i64::from(h)).intersect(&full),
        };
        let src = Img {
            rect: need,
            px: read_raster(raster, need)?,
        };
        let out = run_effect(effect, &params, &src, &cancel)?.crop(region);
        self.write_pixels(&base, layer, &out, &name)
    }

    /// Image ▸ Adjustments: `adjustment_json` (`compositor::Adjustment`, the
    /// JSON of the adjustment layer of the same kind) applied to a pixel
    /// layer's pixels inside the selection, as one history node.
    pub fn apply_adjustment(&self, layer: u64, adjustment_json: String) -> Result<DocumentUpdate> {
        let adj: Adjustment = serde_json::from_str(&adjustment_json)
            .map_err(|e| failure(format!("adjustment JSON: {e}")))?;
        let label = super::adjustment_title(&adj);
        let (base, sel) = self.filter_target(layer)?;
        let l = find(&base, layer)?;
        if !matches!(l.kind, LayerKind::Pixel(_)) {
            return Err(failure("Image ▸ Adjustments apply to pixel layers"));
        }
        if l.props.locks.pixels || l.props.locks.all {
            return Err(failure("the layer's pixels are locked"));
        }
        let full = Rect::of_extent(base.canvas);
        let region = sel
            .as_deref()
            .and_then(super::io::selection_bounds)
            .unwrap_or(full)
            .intersect(&full);
        if region.is_empty() {
            return Err(failure("the selection is empty"));
        }
        // The layer alone with the adjustment clipped to it: the adjustment
        // layer's own maths, alpha kept.
        let mut doc = solo(&base, l);
        let mut s = (**doc.state()).clone();
        let mut a = Layer::new(label, LayerKind::Adjustment(adj));
        a.id = LayerId(s.next_id);
        s.next_id += 1;
        a.props.clipped = true;
        s.root.push(Arc::new(a));
        doc = Document::new(s);
        let img = render_region(&self.shared.filters.comp, &doc, 0, region)?;
        self.write_pixels(&base, layer, &img, label)
    }

    /// The smart filters of a smart object, first applied first.
    pub fn smart_filters(&self, layer: u64) -> Result<Vec<SmartFilterRecord>> {
        let st = self.shared.lock()?;
        let s = st.live().state().clone();
        drop(st);
        let l = find(&s, layer)?;
        let LayerKind::SmartObject(so) = &l.kind else {
            return Ok(Vec::new());
        };
        nodes_of(so)?
            .into_iter()
            .enumerate()
            .map(|(i, n)| {
                Ok(SmartFilterRecord {
                    index: i as u32,
                    filter_id: n.spec.id.clone(),
                    name: n.spec.name(),
                    enabled: n.enabled,
                    filter_json: n.spec.json.clone(),
                    opacity: n.opacity,
                    blend_mode: blend_name(n.blend),
                    has_mask: n.mask_png.is_some(),
                })
            })
            .collect()
    }

    /// Edits smart filter `index` of a smart object (one history node).
    pub fn set_smart_filter(
        &self,
        layer: u64,
        index: u32,
        edit: SmartFilterEdit,
    ) -> Result<DocumentUpdate> {
        let i = index as usize;
        let label = match &edit {
            SmartFilterEdit::Enabled { enabled: true } => "Enable Smart Filter",
            SmartFilterEdit::Enabled { enabled: false } => "Disable Smart Filter",
            SmartFilterEdit::Params { .. } => "Edit Smart Filter",
            SmartFilterEdit::Blending { .. } => "Smart Filter Blending Options",
        };
        self.set_nodes(layer, label, move |nodes| {
            let n = nodes
                .get_mut(i)
                .ok_or_else(|| failure(format!("no smart filter {i}")))?;
            match edit {
                SmartFilterEdit::Enabled { enabled } => n.enabled = enabled,
                SmartFilterEdit::Params { filter_json } => {
                    let spec = Spec::parse(&filter_json)?;
                    if spec.id != n.spec.id {
                        return Err(failure(
                            "a smart filter keeps its filter; add a new one instead",
                        ));
                    }
                    n.spec = spec;
                }
                SmartFilterEdit::Blending { mode, opacity } => {
                    if !opacity.is_finite() {
                        return Err(failure("opacity must be finite"));
                    }
                    n.blend = parse_blend(&mode)?;
                    n.opacity = opacity.clamp(0.0, 1.0);
                }
            }
            Ok(())
        })
    }

    /// Deletes smart filter `index` (one history node).
    pub fn remove_smart_filter(&self, layer: u64, index: u32) -> Result<DocumentUpdate> {
        let i = index as usize;
        self.set_nodes(layer, "Delete Smart Filter", move |nodes| {
            if i >= nodes.len() {
                return Err(failure(format!("no smart filter {i}")));
            }
            nodes.remove(i);
            Ok(())
        })
    }

    /// The mask of smart filter `index` as a grey RGBA8 IOSurface (white =
    /// filtered; all white without a mask), cached per mask.
    pub fn smart_filter_mask_thumbnail(&self, layer: u64, index: u32, max_px: u32) -> Result<u32> {
        if max_px == 0 || max_px > 1024 {
            return Err(failure("max_px must be 1…1024"));
        }
        let (canvas, node) = {
            let st = self.shared.lock()?;
            let s = st.live().state();
            let l = find(s, layer)?;
            let LayerKind::SmartObject(so) = &l.kind else {
                return Err(failure(format!("layer {layer} is not a smart object")));
            };
            let sf = so
                .filters
                .get(index as usize)
                .ok_or_else(|| failure(format!("no smart filter {index}")))?;
            (s.canvas, Node::of(sf)?)
        };
        let key = node.mask_png.as_deref().unwrap_or("").to_owned();
        let slot = (layer, index as usize, max_px);
        let fs = &self.shared.filters;
        if let Some((k, s)) = fs
            .mask_thumbs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&slot)
            && *k == key
        {
            return Ok(s.id());
        }
        let level = (0..super::render::MAX_VIEW_LEVEL)
            .find(|&l| {
                let e = canvas.at_level(l);
                e.width.max(e.height) <= max_px
            })
            .unwrap_or(super::render::MAX_VIEW_LEVEL - 1);
        let e = canvas.at_level(level);
        let r = Rect::of_extent(e);
        let m = match &node.mask_png {
            Some(png) => mask_region(png, canvas, level, r)?,
            None => vec![1.0; (e.width * e.height) as usize],
        };
        let surface = Surface::create_rgba8(e.width, e.height).map_err(failure)?;
        surface
            .with_pixels(|px, stride| {
                for y in 0..e.height as usize {
                    for x in 0..e.width as usize {
                        let v = (m[y * e.width as usize + x].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                        let o = y * stride + x * 4;
                        px[o..o + 4].copy_from_slice(&[v, v, v, 255]);
                    }
                }
            })
            .map_err(failure)?;
        let id = surface.id();
        fs.mask_thumbs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(slot, (key, Arc::new(surface)));
        Ok(id)
    }

    /// Filter ▸ Convert for Smart Filters: the layer becomes a smart object
    /// holding it (same id, name, opacity, blend mode and mask; the contents
    /// keep their pixels at identity transform). One history node.
    pub fn convert_for_smart_filters(&self, layer: u64) -> Result<DocumentUpdate> {
        self.clear_preview_state();
        let mut st = self.shared.lock()?;
        st.open()?;
        let before = st.live().state().clone();
        self.commit_pending(&mut st, None)?;
        let s = st.doc.state().clone();
        let l = find(&s, layer)?;
        if matches!(l.kind, LayerKind::SmartObject(_)) {
            return Err(failure("the layer is already a smart object"));
        }
        if matches!(l.kind, LayerKind::Adjustment(_)) {
            return Err(failure("adjustment layers cannot become smart objects"));
        }
        let mut child = DocState::new(s.canvas, s.depth);
        child.profile = s.profile.clone();
        let mut inner = l.clone();
        inner.props = compositor::LayerProps {
            name: l.props.name.clone(),
            ..Default::default()
        };
        inner.mask = None;
        inner.id = LayerId(1);
        child.next_id = 2;
        if let LayerKind::Group { children, .. } = &mut inner.kind {
            // Children keep their ids inside the child document.
            let max = children.iter().map(|c| max_id(c)).max().unwrap_or(1);
            inner.id = LayerId(max + 1);
            child.next_id = max + 2;
        }
        child.root = vec![Arc::new(inner)];
        let mut outer = Layer::new(
            l.props.name.clone(),
            LayerKind::SmartObject(SmartObject::new(child, Affine::IDENTITY)),
        );
        outer.id = l.id;
        outer.props = l.props.clone();
        outer.props.background = false;
        outer.mask = l.mask.clone();
        let (parent, index) = s
            .locate(LayerId(layer))
            .ok_or_else(|| failure("layer not found"))?;
        let applied = st.doc.apply(DocOp::Batch(vec![
            DocOp::RemoveLayer { id: LayerId(layer) },
            DocOp::AddLayer {
                parent,
                index,
                layer: outer,
            },
        ]))?;
        st.labels
            .insert(applied.node, "Convert to Smart Object".into());
        let mut u = self.update(&mut st, &before, Some(&applied), true);
        u.created.clear();
        Ok(u)
    }
}

fn max_id(l: &Layer) -> u64 {
    l.children()
        .map_or(0, |c| c.iter().map(|x| max_id(x)).max().unwrap_or(0))
        .max(l.id.0)
}

/// Test and bench support (not exported over UniFFI).
impl DocumentSession {
    /// Blocks until previews and smart filter bakes are done.
    #[doc(hidden)]
    pub fn wait_filters_idle(&self) {
        self.shared.filters.wait_idle();
    }

    /// Renders `level` of the *presented* document (smart filters baked at
    /// that level, the preview shown), waiting for the worker, and reads it
    /// back as straight f32 RGBA.
    #[doc(hidden)]
    pub fn read_presented_level(&self, level: u8) -> Result<(u32, u32, Vec<f32>)> {
        for _ in 0..8 {
            let pending = {
                let st = self.shared.lock()?;
                let full = Rect::of_extent(st.live().state().canvas.at_level(level));
                presented(&self.shared, st.live(), level, full);
                let i = self.shared.filters.q.lock();
                i.busy || i.preview_job.is_some() || !i.bake_jobs.is_empty()
            };
            if !pending {
                break;
            }
            self.wait_filters_idle();
        }
        let st = self.shared.lock()?;
        let full = Rect::of_extent(st.live().state().canvas.at_level(level));
        let doc = presented(&self.shared, st.live(), level, full);
        let d: &Document = doc.as_deref().unwrap_or(st.live());
        self.shared.render.read_level(d, level)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips() {
        for n in 0..40 {
            let v: Vec<u8> = (0..n).map(|i| (i * 37 % 256) as u8).collect();
            assert_eq!(base64_decode(&base64_encode(&v)).unwrap(), v);
        }
        assert_eq!(base64_encode(b"Man"), "TWFu");
        assert_eq!(base64_encode(b"Ma"), "TWE=");
    }

    #[test]
    fn filter_json_parses_and_rejects() {
        let s = Spec::parse(r#"{"id":"gaussian_blur","params":{"radius":4}}"#).unwrap();
        assert_eq!(s.id, "gaussian_blur");
        assert_eq!(s.name(), "Gaussian Blur");
        assert_eq!(s.at(2).unwrap().1.radius, 1.0);
        assert!(Spec::parse(r#"{"id":"gaussian_blur","params":{"radius":-4}}"#).is_err());
        assert!(Spec::parse(r#"{"params":{}}"#).is_err());
        assert!(Spec::parse(r#"{"id":"twirl","params":{"center":[0.2,0.7]}}"#).is_ok());
        let n = Node::new(s);
        assert_eq!(Node::of(&n.store()).unwrap(), n);
    }
}
