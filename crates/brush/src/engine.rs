//! Brush settings, live strokes and compositing into a [`Raster`].

use std::sync::Arc;

use compositor::blend::{blend_pixel, dissolve_threshold, lum};
use compositor::{BlendMode, Raster, Rect};
use engine_api::tile::Tile;
use engine_api::{EngineError, EngineResult};

use crate::dynamics::Dynamics;
use crate::gpu::GpuDabRenderer;
use crate::heal::{default_iterations, poisson_blend};
use crate::pixels::Pixels;
use crate::planner::{Dab, Planner};
use crate::sparse::Sparse;
use crate::stroke::{InputPoint, Smoothing};
use crate::symmetry::Symmetry;
use crate::tip::{DualBrush, Pose, Texture, Tip, wet_edges};

/// Where clone/heal paint comes from: the pixel painted at `p` is sampled
/// at `p + offset` ("aligned" clone source).
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct CloneSource {
    /// Source offset, pixels.
    pub offset: [f32; 2],
    /// Source raster; `None` samples the target's stroke-start snapshot.
    #[serde(with = "crate::serde_raster")]
    pub source: Option<Raster>,
}

/// What a stroke does to the target.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default)]
pub enum PaintMode {
    /// Deposit `color` with the brush blend mode.
    #[default]
    Paint,
    /// Remove alpha (on a 1-channel mask: paint black; on RGB: paint white).
    Erase,
    /// Clone stamp.
    Clone(CloneSource),
    /// Healing brush: clone, then Poisson-blend every dab footprint into
    /// its surroundings.
    Heal(CloneSource),
}

/// Complete brush preset.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct Brush {
    /// Tip.
    pub tip: Tip,
    /// Diameter, pixels.
    pub size: f32,
    /// Spacing as a fraction of the (pressure-controlled) diameter.
    pub spacing: f32,
    /// Per-dab flow `0..=1`.
    pub flow: f32,
    /// Stroke opacity cap `0..=1`.
    pub opacity: f32,
    /// Straight RGB paint colour.
    pub color: [f32; 3],
    /// Blend mode of the paint over the target.
    pub blend: BlendMode,
    /// Paint / erase / clone / heal.
    pub mode: PaintMode,
    /// Dynamics.
    pub dynamics: Dynamics,
    /// Dual brush.
    pub dual: Option<DualBrush>,
    /// Texture.
    pub texture: Option<Texture>,
    /// Wet edges.
    pub wet_edges: bool,
    /// Airbrush rate (dabs/s while the pen is held still); `None` = off.
    pub airbrush: Option<f32>,
    /// Smoothing.
    pub smoothing: Smoothing,
    /// Symmetry.
    pub symmetry: Symmetry,
    /// Seed of the Dissolve pattern.
    pub dissolve_seed: u32,
}

impl Default for Brush {
    fn default() -> Self {
        Self {
            tip: Tip::round(1.0),
            size: 20.0,
            spacing: 0.25,
            flow: 1.0,
            opacity: 1.0,
            color: [0.0; 3],
            blend: BlendMode::Normal,
            mode: PaintMode::Paint,
            dynamics: Dynamics::default(),
            dual: None,
            texture: None,
            wet_edges: false,
            airbrush: None,
            smoothing: Smoothing::default(),
            symmetry: Symmetry::None,
            dissolve_seed: 0,
        }
    }
}

impl Brush {
    /// Rejects non-finite or out-of-range settings.
    pub fn validate(&self) -> EngineResult<()> {
        let ok = |v: f32, lo: f32, hi: f32| v.is_finite() && v >= lo && v <= hi;
        if !ok(self.size, 0.1, 10_000.0) {
            return Err(EngineError::invalid("brush.size", "must be in 0.1..=10000"));
        }
        if !ok(self.spacing, 0.01, 10.0) {
            return Err(EngineError::invalid(
                "brush.spacing",
                "must be in 0.01..=10",
            ));
        }
        if !ok(self.flow, 0.0, 1.0) || !ok(self.opacity, 0.0, 1.0) {
            return Err(EngineError::invalid(
                "brush",
                "flow and opacity must be in 0..=1",
            ));
        }
        if !self.color.iter().all(|c| c.is_finite()) {
            return Err(EngineError::invalid("brush.color", "non-finite"));
        }
        if !ok(self.tip.roundness, 0.01, 1.0) || !self.tip.angle.is_finite() {
            return Err(EngineError::invalid(
                "brush.tip",
                "roundness 0.01..=1, finite angle",
            ));
        }
        if let Some(r) = self.airbrush
            && !ok(r, 0.0, 10_000.0)
        {
            return Err(EngineError::invalid(
                "brush.airbrush",
                "rate must be in 0..=10000",
            ));
        }
        if let Some(d) = &self.dual
            && (!ok(d.size, 0.1, 10_000.0) || !d.scatter.is_finite())
        {
            return Err(EngineError::invalid(
                "brush.dual",
                "invalid size or scatter",
            ));
        }
        validate_preset_fields(self)?;
        Ok(())
    }

    /// True if the GPU dab renderer can rasterize this brush (round or
    /// sampled tip, optional wet edges; no dual brush or texture).
    pub fn gpu_compatible(&self) -> bool {
        self.dual.is_none() && self.texture.is_none() && !matches!(self.mode, PaintMode::Heal(_))
    }

    /// Axis-aligned pixel bounds of a dab.
    pub fn dab_bounds(&self, dab: &Dab) -> Rect {
        let e = self.tip.half_extent(dab.size);
        Rect::new(
            (dab.x - e).floor() as i64,
            (dab.y - e).floor() as i64,
            (dab.x + e).ceil() as i64,
            (dab.y + e).ceil() as i64,
        )
    }

    /// Final coverage of `dab` at canvas point `(px, py)`: tip × dual brush,
    /// then wet edges and texture.
    #[inline]
    pub fn dab_coverage(&self, dab: &Dab, px: f32, py: f32) -> f32 {
        let (mut c, rn) = self.tip.coverage(dab, px - dab.x, py - dab.y);
        if c <= 0.0 {
            return 0.0;
        }
        if let Some(db) = &self.dual {
            let pose = Pose {
                size: db.size,
                angle: db.tip.angle,
                roundness: db.tip.roundness,
                flip_x: false,
                flip_y: false,
            };
            let m = dab
                .dual
                .iter()
                .map(|p| db.tip.coverage_pose(&pose, px - p[0], py - p[1]).0)
                .fold(0.0f32, f32::max);
            c *= m;
        }
        if self.wet_edges {
            c = wet_edges(c, rn);
        }
        if let Some(t) = &self.texture {
            c = t.apply(c, px, py);
        }
        c
    }
}

// Serde can construct settings without going through sampled-tip constructors.
// Validate every stored setting before planners or samplers can index/allocate.
fn validate_preset_fields(b: &Brush) -> EngineResult<()> {
    let unit = |v: f32| v.is_finite() && (0.0..=1.0).contains(&v);
    let positive = |v: f32| v.is_finite() && v > 0.0;
    let nonnegative = |v: f32| v.is_finite() && v >= 0.0;
    let sample = |s: &crate::SampledTip| {
        s.width > 0
            && s.height > 0
            && s.width <= crate::abr::MAX_SIDE
            && s.height <= crate::abr::MAX_SIDE
            && u64::from(s.width) * u64::from(s.height) == s.data.len() as u64
            && s.data.iter().all(|&v| unit(v))
    };
    let tip = |t: &Tip| {
        t.angle.is_finite()
            && (0.01..=1.0).contains(&t.roundness)
            && match &t.shape {
                crate::TipShape::Round { hardness } => unit(*hardness),
                crate::TipShape::Sampled(s) => sample(s),
            }
    };
    if !tip(&b.tip) {
        return Err(EngineError::invalid(
            "brush.tip",
            "invalid tip geometry or sample data",
        ));
    }
    if let Some(d) = &b.dual
        && (!tip(&d.tip) || !nonnegative(d.scatter) || !(1..=1024).contains(&d.count))
    {
        return Err(EngineError::invalid(
            "brush.dual",
            "invalid tip, scatter or count (1..=1024)",
        ));
    }
    if let Some(t) = &b.texture
        && (!sample(&t.pattern) || !positive(t.scale) || !unit(t.depth))
    {
        return Err(EngineError::invalid(
            "brush.texture",
            "invalid pattern, scale or depth",
        ));
    }
    let d = &b.dynamics;
    if [d.size, d.roundness, d.flow, d.opacity]
        .iter()
        .any(|j| !unit(j.jitter) || !unit(j.minimum))
        || !unit(d.angle_jitter)
        || !unit(d.count_jitter)
        || !nonnegative(d.scatter)
        || !(1..=1024).contains(&d.count)
    {
        return Err(EngineError::invalid(
            "brush.dynamics",
            "invalid jitter, scatter or count (1..=1024)",
        ));
    }
    if !nonnegative(b.smoothing.string_length) {
        return Err(EngineError::invalid(
            "brush.smoothing",
            "string length must be finite and nonnegative",
        ));
    }
    let symmetry_ok = match b.symmetry {
        Symmetry::None => true,
        Symmetry::Vertical { x } => x.is_finite(),
        Symmetry::Horizontal { y } => y.is_finite(),
        Symmetry::Dual { x, y } => x.is_finite() && y.is_finite(),
        Symmetry::Diagonal { cx, cy } => cx.is_finite() && cy.is_finite(),
        Symmetry::Radial { cx, cy, count } | Symmetry::Mandala { cx, cy, count } => {
            cx.is_finite() && cy.is_finite() && (1..=1024).contains(&count)
        }
    };
    if !symmetry_ok {
        return Err(EngineError::invalid(
            "brush.symmetry",
            "invalid axis or count (1..=1024)",
        ));
    }
    if let PaintMode::Clone(s) | PaintMode::Heal(s) = &b.mode
        && !s.offset.iter().all(|v| v.is_finite())
    {
        return Err(EngineError::invalid("brush.source", "non-finite offset"));
    }
    Ok(())
}

/// Accumulates one dab coverage into a stroke-buffer value ("alpha darken":
/// flow builds up towards the dab's opacity, which it never exceeds).
#[inline]
pub fn accumulate(m: f32, coverage: f32, flow: f32, opacity: f32) -> f32 {
    if m < opacity {
        m + (opacity - m) * (flow * coverage).min(1.0)
    } else {
        m
    }
}

/// Expands 1/3/4-channel normalized samples to straight RGBA.
#[inline]
fn rgba(px: [f32; 4], channels: u8) -> [f32; 4] {
    match channels {
        1 => [px[0], px[0], px[0], 1.0],
        3 => [px[0], px[1], px[2], 1.0],
        _ => px,
    }
}

/// Composites straight colour `s` at alpha `a` over pixel `b` with `mode`
/// (W3C compositing: `co = a(1−ab)s + a·ab·B(b, s) + (1−a)·ab·b`). A
/// 1-channel target lerps towards the paint luminance; a 3-channel target
/// is opaque.
pub fn composite(
    b: [f32; 4],
    s: [f32; 3],
    a: f32,
    mode: BlendMode,
    channels: u8,
    x: u32,
    y: u32,
    seed: u32,
) -> [f32; 4] {
    let mut a = a.clamp(0.0, 1.0);
    if mode == BlendMode::Dissolve {
        a = if a > dissolve_threshold(x, y, seed) {
            1.0
        } else {
            0.0
        };
    }
    if a <= 0.0 {
        return b;
    }
    match channels {
        1 => {
            let v = b[0] + (lum(s) - b[0]) * a;
            [v, 0.0, 0.0, 0.0]
        }
        3 => {
            let m = blend_pixel(mode, [b[0], b[1], b[2]], s);
            [
                b[0] + (m[0] - b[0]) * a,
                b[1] + (m[1] - b[1]) * a,
                b[2] + (m[2] - b[2]) * a,
                0.0,
            ]
        }
        _ => {
            let ab = b[3];
            let ao = a + ab * (1.0 - a);
            if ao <= 0.0 {
                return [0.0; 4];
            }
            let m = blend_pixel(mode, [b[0], b[1], b[2]], s);
            let c = |i: usize| (a * (1.0 - ab) * s[i] + a * ab * m[i] + (1.0 - a) * ab * b[i]) / ao;
            [c(0), c(1), c(2), ao]
        }
    }
}

fn erase(b: [f32; 4], a: f32, channels: u8) -> [f32; 4] {
    match channels {
        1 => [b[0] * (1.0 - a), 0.0, 0.0, 0.0],
        3 => [
            b[0] + (1.0 - b[0]) * a,
            b[1] + (1.0 - b[1]) * a,
            b[2] + (1.0 - b[2]) * a,
            0.0,
        ],
        _ => [b[0], b[1], b[2], b[3] * (1.0 - a)],
    }
}

/// A live stroke over a target raster.
#[derive(Debug)]
pub struct Stroke {
    brush: Brush,
    planner: Planner,
    base: Pixels,
    source: Option<Pixels>,
    selection: Option<Pixels>,
    mask: Sparse<f32>,
    heal: Sparse<Option<[f32; 4]>>,
    dabs: Vec<Dab>,
    dirty: Option<Rect>,
    canvas: Rect,
    channels: u8,
    gpu: Option<Arc<GpuDabRenderer>>,
    gpu_batches: usize,
}

impl Stroke {
    /// Starts a stroke over `base` (snapshotted; cloning a raster is cheap).
    pub fn new(brush: Brush, base: &Raster, seed: u64) -> EngineResult<Self> {
        brush.validate()?;
        let channels = base.channels();
        if !matches!(channels, 1 | 3 | 4) {
            return Err(EngineError::invalid(
                "raster",
                "brush targets 1, 3 or 4 channels",
            ));
        }
        let source = match &brush.mode {
            PaintMode::Clone(c) | PaintMode::Heal(c) => {
                if !c.offset.iter().all(|v| v.is_finite()) {
                    return Err(EngineError::invalid("clone.offset", "non-finite"));
                }
                Some(Pixels::new(c.source.as_ref().unwrap_or(base)))
            }
            _ => None,
        };
        Ok(Self {
            planner: Planner::new(&brush, seed),
            base: Pixels::new(base),
            source,
            selection: None,
            mask: Sparse::new(),
            heal: Sparse::new(),
            dabs: Vec::new(),
            dirty: None,
            canvas: Rect::of_extent(base.extent()),
            channels,
            gpu: None,
            gpu_batches: 0,
            brush,
        })
    }

    /// Limits paint by a canvas-sized single-channel selection.
    pub fn with_selection(mut self, selection: &Raster) -> EngineResult<Self> {
        if selection.channels() != 1 || Rect::of_extent(selection.extent()) != self.canvas {
            return Err(EngineError::invalid(
                "selection",
                "must be single-channel, canvas-sized",
            ));
        }
        self.selection = Some(Pixels::new(selection));
        Ok(self)
    }

    /// Rasterizes compatible dabs on the GPU (see [`Brush::gpu_compatible`];
    /// other brushes keep the CPU path).
    pub fn with_gpu(mut self, gpu: Arc<GpuDabRenderer>) -> Self {
        self.gpu = Some(gpu);
        self
    }

    /// The brush.
    pub fn brush(&self) -> &Brush {
        &self.brush
    }

    /// Every dab placed so far.
    pub fn dabs(&self) -> &[Dab] {
        &self.dabs
    }

    /// Dab batches rasterized on the GPU (0 when gated to the CPU).
    pub fn gpu_batches(&self) -> usize {
        self.gpu_batches
    }

    /// Union of all dirty rects so far.
    pub fn dirty(&self) -> Option<Rect> {
        self.dirty
    }

    /// Stroke-buffer alpha at a pixel (before selection).
    pub fn stroke_alpha(&self, x: i64, y: i64) -> f32 {
        self.mask.get(x, y)
    }

    /// Adds a pointer sample; returns the rect whose pixels changed.
    pub fn add_point(&mut self, p: InputPoint) -> EngineResult<Option<Rect>> {
        if !(p.x.is_finite() && p.y.is_finite() && p.pressure.is_finite()) {
            return Err(EngineError::invalid("point", "non-finite"));
        }
        let dabs = self.planner.push(p);
        self.render(dabs)
    }

    /// Airbrush time step.
    pub fn tick(&mut self, dt: f32) -> EngineResult<Option<Rect>> {
        let dabs = self.planner.tick(dt);
        self.render(dabs)
    }

    /// Ends the stroke.
    pub fn finish(&mut self) -> EngineResult<Option<Rect>> {
        let dabs = self.planner.finish();
        self.render(dabs)
    }

    /// Rasterizes dabs into the stroke buffer.
    pub fn render(&mut self, dabs: Vec<Dab>) -> EngineResult<Option<Rect>> {
        let mut dirty = Rect::default();
        let rects: Vec<Rect> = dabs
            .iter()
            .map(|d| self.brush.dab_bounds(d).intersect(&self.canvas))
            .collect();
        for r in &rects {
            dirty = dirty.union(r);
        }
        let mut done = false;
        if let Some(gpu) = self.gpu.clone().filter(|_| self.brush.gpu_compatible())
            && !dirty.is_empty()
        {
            let live: Vec<Dab> = dabs
                .iter()
                .zip(&rects)
                .filter(|(_, r)| !r.is_empty())
                .map(|(d, _)| d.clone())
                .collect();
            let mut data = self.mask.read_rect(dirty);
            // Any GPU failure falls back to the CPU path for this batch.
            if gpu
                .render(
                    dirty,
                    &mut data,
                    &live,
                    &self.brush.tip,
                    self.brush.wet_edges,
                )
                .is_ok()
            {
                self.mask.write_rect(dirty, &data);
                self.gpu_batches += 1;
                done = true;
            }
        }
        if !done {
            for (dab, r) in dabs.iter().zip(&rects) {
                if r.is_empty() {
                    continue;
                }
                self.rasterize(dab, *r);
                if matches!(self.brush.mode, PaintMode::Heal(_)) {
                    self.heal_dab(dab, *r);
                }
            }
        }
        self.dabs.extend(dabs);
        if dirty.is_empty() {
            return Ok(None);
        }
        self.dirty = Some(self.dirty.map_or(dirty, |d| d.union(&dirty)));
        Ok(Some(dirty))
    }

    fn rasterize(&mut self, dab: &Dab, r: Rect) {
        for y in r.y0..r.y1 {
            let py = y as f32 + 0.5;
            for x in r.x0..r.x1 {
                let c = self.brush.dab_coverage(dab, x as f32 + 0.5, py);
                if c <= 0.0 {
                    continue;
                }
                let m = self.mask.get_mut(x, y);
                *m = accumulate(*m, c, dab.flow, dab.opacity);
            }
        }
    }

    fn clone_offset(&self) -> [f32; 2] {
        match &self.brush.mode {
            PaintMode::Clone(c) | PaintMode::Heal(c) => c.offset,
            _ => [0.0; 2],
        }
    }

    fn source_rgba(&mut self, x: i64, y: i64) -> [f32; 4] {
        let o = self.clone_offset();
        let channels = self.channels;
        match &mut self.source {
            Some(s) => {
                let ch = s.raster().channels();
                rgba(s.sample(x as f32 + 0.5 + o[0], y as f32 + 0.5 + o[1]), ch)
            }
            None => rgba(self.base.get(x, y), channels),
        }
    }

    /// Poisson-blends the dab footprint: the source's gradients inside,
    /// the currently displayed pixels on the boundary.
    fn heal_dab(&mut self, dab: &Dab, r: Rect) {
        let rr = r.inflate(1).intersect(&self.canvas);
        let (w, h) = (rr.width() as usize, rr.height() as usize);
        let mut omega = vec![false; w * h];
        let mut dest = vec![[0.0f32; 4]; w * h];
        let mut guide = vec![[0.0f32; 4]; w * h];
        let mut any = false;
        for y in rr.y0..rr.y1 {
            for x in rr.x0..rr.x1 {
                let i = (y - rr.y0) as usize * w + (x - rr.x0) as usize;
                let inside = x >= r.x0
                    && x < r.x1
                    && y >= r.y0
                    && y < r.y1
                    && self.brush.dab_coverage(dab, x as f32 + 0.5, y as f32 + 0.5) > 1e-3;
                omega[i] = inside;
                any |= inside;
                let ch = self.channels;
                dest[i] = match self.heal.get(x, y) {
                    Some(c) => c,
                    None => rgba(self.base.get(x, y), ch),
                };
                guide[i] = self.source_rgba(x, y);
            }
        }
        if !any {
            return;
        }
        let rgb = if self.channels == 1 { 1 } else { 3 };
        poisson_blend(
            &mut dest,
            &guide,
            &omega,
            w,
            h,
            rgb,
            default_iterations(w, h),
        );
        for y in rr.y0..rr.y1 {
            for x in rr.x0..rr.x1 {
                let i = (y - rr.y0) as usize * w + (x - rr.x0) as usize;
                if omega[i] {
                    *self.heal.get_mut(x, y) = Some(dest[i]);
                }
            }
        }
    }

    /// Final pixel of the stroke over the base snapshot at `(x, y)`.
    pub fn shade(&mut self, x: u32, y: u32, px: &mut [f32; 4]) {
        let (xi, yi) = (i64::from(x), i64::from(y));
        let base = self.base.get(xi, yi);
        let mut a = self.mask.get(xi, yi);
        if let Some(sel) = &mut self.selection {
            a *= sel.get(xi, yi)[0].clamp(0.0, 1.0);
        }
        if a <= 0.0 {
            *px = base;
            return;
        }
        let (ch, blend, seed) = (self.channels, self.brush.blend, self.brush.dissolve_seed);
        *px = match self.brush.mode {
            PaintMode::Paint => composite(base, self.brush.color, a, blend, ch, x, y, seed),
            PaintMode::Erase => erase(base, a, ch),
            PaintMode::Clone(_) => {
                let s = self.source_rgba(xi, yi);
                composite(base, [s[0], s[1], s[2]], a * s[3], blend, ch, x, y, seed)
            }
            PaintMode::Heal(_) => {
                let s = match self.heal.get(xi, yi) {
                    Some(c) => c,
                    None => self.source_rgba(xi, yi),
                };
                composite(base, [s[0], s[1], s[2]], a, blend, ch, x, y, seed)
            }
        };
        if ch == 1 {
            px[1] = 0.0;
        }
    }

    /// Writes the composited stroke over `rect` into `target` (which must
    /// be the raster the stroke started on, or one of its descendants).
    pub fn apply(&mut self, target: &mut Raster, rect: Rect, rev: u64) -> EngineResult<()> {
        if Rect::of_extent(target.extent()) != self.canvas || target.channels() != self.channels {
            return Err(EngineError::invalid(
                "target",
                "does not match the stroke base",
            ));
        }
        target.edit_region(rect, rev, |x, y, p| self.shade(x, y, p))
    }

    /// New tiles covering `rect` (for `DocOp::PaintTiles`).
    pub fn render_tiles(&mut self, rect: Rect) -> EngineResult<Vec<(u32, u32, Tile)>> {
        let base = self.base.raster().clone();
        base.render_region(rect, |x, y, p| self.shade(x, y, p))
    }
}
