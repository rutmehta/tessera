//! Tile programs: the layer tree flattened for one tile, and the CPU
//! executor that runs them over a (sub-)rectangle of the tile.

use std::sync::Arc;

use engine_api::tile::{TILE_SIZE, Tile, TileCoord, TileLayout};
use engine_api::{EngineError, EngineResult};

use super::cache::{NodeKey, Part};
use super::pixel::{Kb, Params, adjust_px, blend_px, unpremul};
use super::{Compositor, DocRef};
use crate::adjust::Adjustment;
use crate::blend::{BlendMode, blend_pixel};
use crate::document::{Fill, GroupMode, Knockout, Layer, LayerKind, SmartObject};
use crate::raster::{load_normalized, load_normalized_region};

impl Compositor {
    /// Live CPU reference fallback, returning straight planar f32 RGBA.
    /// Replays backdrop prefixes over the halo using a fresh zero-cache
    /// compositor on every call; never bakes adjustments or reuses damage tiles.
    /// Ordinary isolated/pass-through/clipping groups are supported. Layer
    /// styles are rejected because their eager source compilation cannot replay
    /// adjustment prefixes. Neighbourhood adjustments inside smart-object child
    /// documents remain unsupported (their renderer uses the standard path).
    /// Unlike `render_tile`, this explicitly opts into expensive halo replay.
    pub fn render_tile_with_neighbourhood(
        &self,
        doc: &crate::edit::Document,
        coord: TileCoord,
    ) -> EngineResult<Tile> {
        if super::effects::has_styles(doc.state()) {
            return Err(EngineError::Unsupported {
                what: "neighbourhood CPU fallback does not support layer styles".into(),
            });
        }
        let fresh = Self::new(0);
        let job = fresh.job(
            DocRef {
                state: doc.state(),
                key: doc.key(),
            },
            coord,
        )?;
        let ops = job.compile()?;
        let acc = job.run_with_neighbourhood(&ops)?;
        super::unpremultiply(
            &Tile::from_samples(coord, job.layout(), acc)?.with_premultiplied(true)?,
        )
    }
}

/// Where a blended layer's straight RGBA comes from.
pub(crate) enum Src<'a> {
    /// CPU neighbourhood-effects program; the GPU port rejects this source.
    Styled(super::effects::StyledTile),
    Raster(&'a Layer),
    Fill(&'a Fill),
    Smart(&'a Layer, &'a SmartObject),
    /// A cached isolated-group composite (premultiplied f32).
    Group(Tile),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameKind {
    Root,
    Isolated,
    Clip,
    PassThrough,
}

pub(crate) enum Op<'a> {
    Blend {
        layer: &'a Layer,
        src: Src<'a>,
        params: Params,
        mask: bool,
    },
    Adjust {
        layer: &'a Layer,
        adj: &'a Adjustment,
        params: Params,
    },
    Push(FrameKind),
    Pop {
        layer: &'a Layer,
        params: Params,
        mask: bool,
        pass: bool,
        cache: Option<NodeKey>,
    },
    SnapshotBackground,
}

/// Local rectangle inside a tile, `[x0, x1) × [y0, y1)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Region {
    pub x0: usize,
    pub y0: usize,
    pub x1: usize,
    pub y1: usize,
}

/// Everything needed to render one tile of one document.
pub(crate) struct TileJob<'a> {
    pub comp: &'a Compositor,
    pub doc: DocRef<'a>,
    pub coord: TileCoord,
    pub w: usize,
    pub n: usize,
    pub origin: (u32, u32),
    pub region: Region,
    pub full: bool,
}

impl<'a> TileJob<'a> {
    pub fn layout(&self) -> TileLayout {
        TileLayout {
            extent: engine_api::tile::Extent::new(self.w as u32, (self.n / self.w) as u32),
            halo: 0,
            channels: 4,
        }
    }

    // ---------------------------------------------------------------- compile

    pub fn compile(&self) -> EngineResult<Vec<Op<'a>>> {
        let mut ops = Vec::new();
        self.compile_list(&self.doc.state.root, true, &mut ops)?;
        Ok(ops)
    }

    fn compile_list(
        &self,
        children: &'a [Arc<Layer>],
        root: bool,
        ops: &mut Vec<Op<'a>>,
    ) -> EngineResult<()> {
        let mut i = 0;
        while i < children.len() {
            let base: &'a Layer = &children[i];
            i += 1;
            let start = i;
            while i < children.len() && children[i].props.clipped {
                i += 1;
            }
            if !base.props.visible {
                continue;
            }
            let clipped: Vec<&'a Layer> = children[start..i]
                .iter()
                .filter(|c| c.props.visible)
                .map(|c| &**c)
                .collect();
            let base_is_adjustment = matches!(base.kind, LayerKind::Adjustment(_));
            if clipped.is_empty() || base_is_adjustment {
                self.emit(base, false, ops)?;
                for c in clipped {
                    self.emit(c, false, ops)?;
                }
            } else {
                ops.push(Op::Push(FrameKind::Clip));
                self.emit_with(base, Params::clip_base(base), true, ops)?;
                for c in clipped {
                    self.emit(c, true, ops)?;
                }
                ops.push(Op::Pop {
                    layer: base,
                    params: Params::clip_pop(base),
                    mask: false,
                    pass: false,
                    cache: None,
                });
            }
            if root && start == 1 && base.props.background {
                ops.push(Op::SnapshotBackground);
            }
        }
        Ok(())
    }

    fn emit(&self, layer: &'a Layer, atop: bool, ops: &mut Vec<Op<'a>>) -> EngineResult<()> {
        if !layer.props.styles.effects.is_empty() {
            return self.emit_styles(layer, Params::of(layer, atop), ops);
        }
        if let LayerKind::Group {
            mode: GroupMode::PassThrough,
            children,
        } = &layer.kind
            && !atop
        {
            ops.push(Op::Push(FrameKind::PassThrough));
            self.compile_list(children, false, ops)?;
            ops.push(Op::Pop {
                layer,
                params: Params::of(layer, false),
                mask: true,
                pass: true,
                cache: None,
            });
            return Ok(());
        }
        if let LayerKind::Adjustment(adj) = &layer.kind {
            adj.validate()?;
            ops.push(Op::Adjust {
                layer,
                adj,
                params: Params::of(layer, atop),
            });
            return Ok(());
        }
        self.emit_with(layer, Params::of(layer, atop), true, ops)
    }

    /// Emits a layer as a blended source with explicit params.
    fn emit_with(
        &self,
        layer: &'a Layer,
        params: Params,
        mask: bool,
        ops: &mut Vec<Op<'a>>,
    ) -> EngineResult<()> {
        if !layer.props.styles.effects.is_empty() {
            return self.emit_styles(layer, params, ops);
        }
        let src = match &layer.kind {
            LayerKind::Pixel(_) | LayerKind::Text(_) => Src::Raster(layer),
            LayerKind::Fill(f) => Src::Fill(f),
            LayerKind::SmartObject(so) => Src::Smart(layer, so),
            LayerKind::Adjustment(_) => return Err(EngineError::internal("adjustment as source")),
            LayerKind::Group { children, .. } => {
                let (l, x, y) = (self.coord.level, self.coord.x, self.coord.y);
                let key = NodeKey {
                    doc: self.doc.key,
                    node: layer.id.0,
                    part: Part::Group,
                    stamp: if super::effects::has_styles(self.doc.state) {
                        self.doc.state.rev
                    } else {
                        layer.stamp(l, x, y)
                    },
                    coord: self.coord,
                };
                if let Some(t) = self.comp.cache_get(&key) {
                    Src::Group(t)
                } else {
                    ops.push(Op::Push(FrameKind::Isolated));
                    self.compile_list(children, false, ops)?;
                    ops.push(Op::Pop {
                        layer,
                        params,
                        mask,
                        pass: false,
                        cache: self.full.then_some(key),
                    });
                    return Ok(());
                }
            }
        };
        ops.push(Op::Blend {
            layer,
            src,
            params,
            mask,
        });
        Ok(())
    }

    // ---------------------------------------------------------------- sources

    /// Loads straight RGBA into `out` over the region. Returns false when the
    /// source is fully transparent in this tile (the op can be skipped).
    pub fn load_src(&self, src: &Src<'_>, out: &mut [f32]) -> EngineResult<bool> {
        let (n, w, r) = (self.n, self.w, self.region);
        match src {
            Src::Styled(_) => Err(EngineError::Unsupported {
                what: "layer styles require CPU compositing".into(),
            }),
            Src::Raster(layer) => {
                let raster = layer
                    .raster()
                    .ok_or_else(|| EngineError::internal("no raster"))?;
                let Some(tile) = self.comp.raster_level(
                    self.doc.key,
                    layer.id.0,
                    Part::Content,
                    raster,
                    self.coord,
                )?
                else {
                    let v = raster.default_value();
                    if v == 0.0 {
                        return Ok(false);
                    }
                    for c in 0..4 {
                        for y in r.y0..r.y1 {
                            out[c * n + y * w + r.x0..c * n + y * w + r.x1].fill(v);
                        }
                    }
                    return Ok(true);
                };
                if self.full {
                    load_normalized(&tile, out)?;
                } else {
                    load_normalized_region(&tile, out, (r.x0, r.y0, r.x1, r.y1))?;
                }
                Ok(true)
            }
            Src::Fill(f) => {
                let d = (1u32 << self.coord.level) as f32;
                for y in r.y0..r.y1 {
                    let cy = (self.origin.1 as f32 + y as f32 + 0.5) * d;
                    for x in r.x0..r.x1 {
                        let cx = (self.origin.0 as f32 + x as f32 + 0.5) * d;
                        let v = f.sample(cx, cy);
                        let i = y * w + x;
                        for c in 0..4 {
                            out[c * n + i] = v[c];
                        }
                    }
                }
                Ok(true)
            }
            Src::Smart(layer, so) => {
                let Some(tile) = self.comp.smart_tile(self.doc, layer, so, self.coord)? else {
                    return Ok(false);
                };
                copy_region(tile.samples::<f32>()?, out, n, w, r, 4);
                Ok(true)
            }
            Src::Group(tile) => {
                self.unpremul_into(tile.samples::<f32>()?, out);
                Ok(true)
            }
        }
    }

    fn unpremul_into(&self, pm: &[f32], out: &mut [f32]) {
        let (n, w, r) = (self.n, self.w, self.region);
        for y in r.y0..r.y1 {
            for i in y * w + r.x0..y * w + r.x1 {
                let c = unpremul([pm[i], pm[n + i], pm[2 * n + i], pm[3 * n + i]]);
                out[i] = c[0];
                out[n + i] = c[1];
                out[2 * n + i] = c[2];
                out[3 * n + i] = pm[3 * n + i];
            }
        }
    }

    /// Loads the effective mask (`1 − d(1 − m)`) of `layer` into `out`
    /// (one plane). Returns false when the layer has no active mask.
    pub fn load_mask(&self, layer: &Layer, out: &mut [f32]) -> EngineResult<bool> {
        let Some(m) = layer.mask.as_ref().filter(|m| m.enabled) else {
            return Ok(false);
        };
        let d = m.density.clamp(0.0, 1.0);
        let (n, w, r) = (self.n, self.w, self.region);
        match self
            .comp
            .raster_level(self.doc.key, layer.id.0, Part::Mask, &m.raster, self.coord)?
        {
            None => {
                let v = 1.0 - d * (1.0 - m.raster.default_value());
                for y in r.y0..r.y1 {
                    out[y * w + r.x0..y * w + r.x1].fill(v);
                }
            }
            Some(t) => {
                let mut tmp = vec![0.0; n];
                load_normalized_region(&t, &mut tmp, (r.x0, r.y0, r.x1, r.y1))?;
                for y in r.y0..r.y1 {
                    for i in y * w + r.x0..y * w + r.x1 {
                        out[i] = 1.0 - d * (1.0 - tmp[i]);
                    }
                }
            }
        }
        Ok(true)
    }

    // ---------------------------------------------------------------- execute

    /// Runs the program; returns the premultiplied root accumulator (only
    /// the region is meaningful).
    pub fn run(&self, ops: &[Op<'_>]) -> EngineResult<Vec<f32>> {
        self.run_until(ops, None, false)
    }

    fn run_with_neighbourhood(&self, ops: &[Op<'_>]) -> EngineResult<Vec<f32>> {
        self.run_until(ops, None, true)
    }

    fn run_until(
        &self,
        ops: &[Op<'_>],
        target: Option<u64>,
        neighbourhood: bool,
    ) -> EngineResult<Vec<f32>> {
        let n = self.n;
        let mut frames: Vec<(FrameKind, Vec<f32>)> = vec![(FrameKind::Root, vec![0.0; 4 * n])];
        let mut deep: Option<Vec<f32>> = None;
        let mut src = vec![0.0f32; 4 * n];
        let mut mask = vec![0.0f32; n];
        let clamp = !self.doc.state.depth.is_float();
        for op in ops {
            match op {
                Op::Blend {
                    layer,
                    src: s,
                    params,
                    mask: use_mask,
                } => {
                    if let Src::Styled(styled) = s {
                        self.run_styles(&mut frames, deep.as_deref(), styled, params)?;
                        continue;
                    }
                    if !self.load_src(s, &mut src)? {
                        continue;
                    }
                    if *use_mask && self.load_mask(layer, &mut mask)? {
                        self.mul_alpha(&mut src, &mask);
                    }
                    self.comp.stats.bump_blend();
                    self.blend_top(&mut frames, deep.as_deref(), &src, params);
                }
                Op::Adjust { layer, adj, params } => {
                    if target == Some(layer.id.0) {
                        // The adjustment consumes its current group/clip frame,
                        // not the root accumulator. Do not execute the target.
                        return Ok(frames.pop().ok_or_else(stack)?.1);
                    }
                    let has_mask = self.load_mask(layer, &mut mask)?;
                    let acc = &mut frames.last_mut().ok_or_else(stack)?.1;
                    if let Adjustment::ShadowsHighlights { settings } = adj {
                        settings.validate()?;
                        if settings.needs_neighbourhood() && !neighbourhood {
                            // Root/group region stamps and partial-damage reuse do not
                            // yet cover the backdrop halo. Never substitute a tile-local
                            // bilateral (seams), a pointwise result, or a stale cache hit.
                            return Err(EngineError::Unsupported {
                                what: "Shadows/Highlights needs a padded backdrop; use Compositor::render_tile_with_neighbourhood for live CPU fallback".into(),
                            });
                        }
                        self.adjust_shadows(
                            acc,
                            settings,
                            params,
                            has_mask.then_some(&mask[..]),
                            clamp,
                            layer.id.0,
                        )?;
                    } else {
                        self.adjust(acc, adj, params, has_mask.then_some(&mask[..]), clamp);
                    }
                }
                Op::Push(kind) => {
                    let acc = match kind {
                        FrameKind::PassThrough => frames.last().ok_or_else(stack)?.1.clone(),
                        _ => vec![0.0; 4 * n],
                    };
                    frames.push((*kind, acc));
                }
                Op::Pop {
                    layer,
                    params,
                    mask: use_mask,
                    pass,
                    cache,
                } => {
                    let (_, child) = frames.pop().ok_or_else(stack)?;
                    if frames.is_empty() {
                        return Err(stack());
                    }
                    let has_mask = *use_mask && self.load_mask(layer, &mut mask)?;
                    if *pass {
                        let t0 = params.opacity * params.fill;
                        let parent = &mut frames.last_mut().ok_or_else(stack)?.1;
                        self.lerp_pass(parent, &child, t0, has_mask.then_some(&mask[..]));
                    } else {
                        if let Some(key) = cache {
                            self.comp.stats.bump_group();
                            let t = Tile::from_samples(self.coord, self.layout(), child.clone())?
                                .with_premultiplied(true)?;
                            self.comp.cache_put(*key, t);
                        }
                        self.unpremul_into(&child, &mut src);
                        if has_mask {
                            self.mul_alpha(&mut src, &mask);
                        }
                        self.blend_top(&mut frames, deep.as_deref(), &src, params);
                    }
                }
                Op::SnapshotBackground => deep = Some(frames[0].1.clone()),
            }
        }
        if target.is_some() {
            return Err(EngineError::internal(
                "neighbourhood adjustment prefix not found",
            ));
        }
        if frames.len() != 1 {
            return Err(stack());
        }
        Ok(frames.pop().ok_or_else(stack)?.1)
    }

    fn mul_alpha(&self, src: &mut [f32], mask: &[f32]) {
        let (n, w, r) = (self.n, self.w, self.region);
        for y in r.y0..r.y1 {
            for i in y * w + r.x0..y * w + r.x1 {
                src[3 * n + i] *= mask[i];
            }
        }
    }

    fn lerp_pass(&self, parent: &mut [f32], child: &[f32], t0: f32, mask: Option<&[f32]>) {
        let (n, w, r) = (self.n, self.w, self.region);
        for y in r.y0..r.y1 {
            for i in y * w + r.x0..y * w + r.x1 {
                let t = t0 * mask.map_or(1.0, |m| m[i]);
                for c in 0..4 {
                    let j = c * n + i;
                    parent[j] += t * (child[j] - parent[j]);
                }
            }
        }
    }

    pub(super) fn blend_top(
        &self,
        frames: &mut [(FrameKind, Vec<f32>)],
        deep: Option<&[f32]>,
        src: &[f32],
        p: &Params,
    ) {
        let top = frames.len() - 1;
        let (lo, hi) = frames.split_at_mut(top);
        let (kind, acc) = (&hi[0].0, &mut hi[0].1);
        let kb: KbSrc<'_> = match p.knockout {
            Knockout::None => KbSrc::Off,
            Knockout::Deep => deep.map_or(KbSrc::Transparent, KbSrc::Buf),
            Knockout::Shallow => match kind {
                FrameKind::Root => deep.map_or(KbSrc::Transparent, KbSrc::Buf),
                FrameKind::PassThrough => {
                    lo.last().map_or(KbSrc::Transparent, |f| KbSrc::Buf(&f.1))
                }
                FrameKind::Isolated | FrameKind::Clip => KbSrc::Transparent,
            },
        };
        macro_rules! go {
            ($($m:ident),*) => {
                match p.mode {
                    $(BlendMode::$m => self.blend_loop(acc, src, p, kb, &|b, s| blend_pixel(BlendMode::$m, b, s)),)*
                }
            };
        }
        go!(
            Normal,
            Dissolve,
            Darken,
            Multiply,
            ColorBurn,
            LinearBurn,
            DarkerColor,
            Lighten,
            Screen,
            ColorDodge,
            LinearDodge,
            LighterColor,
            Overlay,
            SoftLight,
            HardLight,
            VividLight,
            LinearLight,
            PinLight,
            HardMix,
            Difference,
            Exclusion,
            Subtract,
            Divide,
            Hue,
            Saturation,
            Color,
            Luminosity
        );
    }

    #[inline(always)]
    fn blend_loop<F: Fn([f32; 3], [f32; 3]) -> [f32; 3]>(
        &self,
        acc: &mut [f32],
        src: &[f32],
        p: &Params,
        kb: KbSrc<'_>,
        f: &F,
    ) {
        let (n, w, r) = (self.n, self.w, self.region);
        let (ox, oy) = self.origin;
        let (ar, rest) = acc.split_at_mut(n);
        let (ag, rest) = rest.split_at_mut(n);
        let (ab, aa) = rest.split_at_mut(n);
        let (sr, sg, sb, sa) = (
            &src[..n],
            &src[n..2 * n],
            &src[2 * n..3 * n],
            &src[3 * n..4 * n],
        );
        let simple = matches!(kb, KbSrc::Off)
            && p.blend_if.is_none()
            && !p.atop
            && p.mode != BlendMode::Dissolve;
        for y in r.y0..r.y1 {
            let (i0, i1) = (y * w + r.x0, y * w + r.x1);
            if simple {
                // Common case: branch-free, bounds-check-free row so LLVM
                // can if-convert and vectorize. Same maths as `blend_px`
                // (a transparent source pixel yields exactly the backdrop).
                let k = p.opacity * p.fill;
                let (ar, ag, ab, aa) = (
                    &mut ar[i0..i1],
                    &mut ag[i0..i1],
                    &mut ab[i0..i1],
                    &mut aa[i0..i1],
                );
                let (sr, sg, sb, sa) = (&sr[i0..i1], &sg[i0..i1], &sb[i0..i1], &sa[i0..i1]);
                let m = ar.len();
                let (ag, ab, aa, sr, sg, sb, sa) = (
                    &mut ag[..m],
                    &mut ab[..m],
                    &mut aa[..m],
                    &sr[..m],
                    &sg[..m],
                    &sb[..m],
                    &sa[..m],
                );
                for i in 0..m {
                    let a = sa[i].max(0.0) * k;
                    let alpha_b = aa[i];
                    let inv = if alpha_b > 0.0 { 1.0 / alpha_b } else { 0.0 };
                    let cb = [ar[i] * inv, ag[i] * inv, ab[i] * inv];
                    let cs = [sr[i], sg[i], sb[i]];
                    let bl = f(cb, cs);
                    let (u, v, wt) = (a * (1.0 - alpha_b), a * alpha_b, 1.0 - a);
                    ar[i] = u * cs[0] + v * bl[0] + wt * ar[i];
                    ag[i] = u * cs[1] + v * bl[1] + wt * ag[i];
                    ab[i] = u * cs[2] + v * bl[2] + wt * ab[i];
                    aa[i] = a + wt * alpha_b;
                }
                continue;
            }
            for i in i0..i1 {
                let sigma = sa[i];
                if sigma <= 0.0 {
                    continue;
                }
                let k = match kb {
                    KbSrc::Off => Kb::Off,
                    KbSrc::Transparent => Kb::Transparent,
                    KbSrc::Buf(b) => Kb::Px([b[i], b[n + i], b[2 * n + i], b[3 * n + i]]),
                };
                let x = ox + (i - y * w) as u32;
                let o = blend_px(
                    [ar[i], ag[i], ab[i], aa[i]],
                    [sr[i], sg[i], sb[i], sigma],
                    p,
                    k,
                    x,
                    oy + y as u32,
                    f,
                );
                ar[i] = o[0];
                ag[i] = o[1];
                ab[i] = o[2];
                aa[i] = o[3];
            }
        }
    }

    /// Reconstruct only earlier adjustment prefixes. Each recursive replay
    /// terminates at an earlier op, including when inside a nested group.
    fn shadows_backdrop(
        &self,
        acc: &[f32],
        target: u64,
        halo: usize,
    ) -> EngineResult<Vec<[f32; 4]>> {
        let e = self.doc.state.canvas.at_level(self.coord.level);
        let (w, h) = (self.w + 2 * halo, self.n / self.w + 2 * halo);
        let mut tiles = std::collections::HashMap::new();
        tiles.insert((self.coord.x, self.coord.y), (acc.to_vec(), self.w, self.n));
        let mut padded = Vec::with_capacity(w * h);
        for y in 0..h {
            let gy = (self.origin.1 as i64 + y as i64 - halo as i64)
                .clamp(0, i64::from(e.height) - 1) as u32;
            for x in 0..w {
                let gx = (self.origin.0 as i64 + x as i64 - halo as i64)
                    .clamp(0, i64::from(e.width) - 1) as u32;
                let key = (gx / TILE_SIZE, gy / TILE_SIZE);
                if let std::collections::hash_map::Entry::Vacant(entry) = tiles.entry(key) {
                    let coord = TileCoord::new(self.coord.level, key.0, key.1);
                    let job = self.comp.job(self.doc, coord)?;
                    let ops = job.compile()?;
                    entry.insert((job.run_until(&ops, Some(target), true)?, job.w, job.n));
                }
                let (pm, tw, n) = &tiles[&key];
                let i = (gy % TILE_SIZE) as usize * tw + (gx % TILE_SIZE) as usize;
                let p = [pm[i], pm[n + i], pm[2 * n + i], pm[3 * n + i]];
                let rgb = unpremul(p);
                padded.push([rgb[0], rgb[1], rgb[2], p[3]]);
            }
        }
        Ok(padded)
    }

    #[allow(clippy::too_many_arguments)]
    fn adjust_shadows(
        &self,
        acc: &mut [f32],
        settings: &crate::adjust::shadows::ShadowsHighlights,
        p: &Params,
        mask: Option<&[f32]>,
        clamp: bool,
        target: u64,
    ) -> EngineResult<()> {
        let (n, w, r) = (self.n, self.w, self.region);
        let mut backdrop = Vec::with_capacity((r.x1 - r.x0) * (r.y1 - r.y0));
        for y in r.y0..r.y1 {
            for i in y * w + r.x0..y * w + r.x1 {
                let b = [acc[i], acc[n + i], acc[2 * n + i], acc[3 * n + i]];
                let rgb = unpremul(b);
                backdrop.push([rgb[0], rgb[1], rgb[2], b[3]]);
            }
        }
        let rw = r.x1 - r.x0;
        let rh = r.y1 - r.y0;
        let adjusted = if settings.needs_neighbourhood() {
            let halo = settings.halo(self.coord.level);
            let padded = self.shadows_backdrop(acc, target, halo)?;
            settings.apply_padded(
                &padded,
                w + 2 * halo,
                n / w + 2 * halo,
                [halo + r.x0, halo + r.y0, halo + r.x1, halo + r.y1],
                self.coord.level,
            )?
        } else {
            settings.apply_padded(&backdrop, rw, rh, [0, 0, rw, rh], self.coord.level)?
        };
        for y in r.y0..r.y1 {
            for x in r.x0..r.x1 {
                let i = y * w + x;
                let b = [acc[i], acc[n + i], acc[2 * n + i], acc[3 * n + i]];
                let a = adjusted[(y - r.y0) * rw + x - r.x0];
                let rgb = [a[0], a[1], a[2]];
                let rgb = if clamp {
                    rgb.map(|v| v.clamp(0.0, 1.0))
                } else {
                    rgb
                };
                let wt = p.opacity * p.fill * mask.map_or(1.0, |m| m[i]);
                let out = adjust_px(
                    b,
                    rgb,
                    wt,
                    p,
                    self.origin.0 + x as u32,
                    self.origin.1 + y as u32,
                );
                for c in 0..3 {
                    acc[c * n + i] = out[c];
                }
            }
        }
        Ok(())
    }

    fn adjust(
        &self,
        acc: &mut [f32],
        adj: &Adjustment,
        p: &Params,
        mask: Option<&[f32]>,
        clamp: bool,
    ) {
        let compiled = adj.compile();
        let (n, w, r) = (self.n, self.w, self.region);
        let (ox, oy) = self.origin;
        let w0 = p.opacity * p.fill;
        for y in r.y0..r.y1 {
            for i in y * w + r.x0..y * w + r.x1 {
                let b = [acc[i], acc[n + i], acc[2 * n + i], acc[3 * n + i]];
                if b[3] <= 0.0 {
                    continue;
                }
                let mut a = compiled.apply_at(unpremul(b), ox + (i - y * w) as u32, oy + y as u32);
                if clamp {
                    a = a.map(|v| v.clamp(0.0, 1.0));
                }
                let wt = w0 * mask.map_or(1.0, |m| m[i]);
                let o = adjust_px(b, a, wt, p, ox + (i - y * w) as u32, oy + y as u32);
                acc[i] = o[0];
                acc[n + i] = o[1];
                acc[2 * n + i] = o[2];
            }
        }
    }
}

#[derive(Clone, Copy)]
enum KbSrc<'a> {
    Off,
    Transparent,
    Buf(&'a [f32]),
}

fn stack() -> EngineError {
    EngineError::internal("unbalanced tile program")
}

fn copy_region(from: &[f32], to: &mut [f32], n: usize, w: usize, r: Region, planes: usize) {
    for c in 0..planes {
        for y in r.y0..r.y1 {
            let a = c * n + y * w;
            to[a + r.x0..a + r.x1].copy_from_slice(&from[a + r.x0..a + r.x1]);
        }
    }
}
