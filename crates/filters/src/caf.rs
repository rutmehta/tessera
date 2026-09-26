//! Deterministic CPU PatchMatch synthesis (not the offset-only brush Patch tool).
//! All masks are canvas-sized, row-major coverage in [0,1]. Inputs are immutable.
use crate::{Buffer, checkpoint};
use compositor::{Raster, Rect, raster::Depth};
use engine_api::{EngineError, EngineResult};
use std::{collections::VecDeque, sync::atomic::AtomicBool};

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SamplingArea {
    /// All unselected pixels are eligible; entire patch footprints are checked.
    #[default]
    Auto,
    Rect(Rect),
    Custom(Vec<f32>),
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColourAdaptation {
    None,
    #[default]
    Default,
    High,
    VeryHigh,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FillParams {
    pub patch_radius: u32,
    pub iterations: u32,
    pub seed: u64,
    pub sampling: SamplingArea,
    pub colour_adaptation: ColourAdaptation,
    pub output_new_layer: bool,
    /// Search 0, +/-limit/2 and +/-limit radians (0..=pi).
    pub rotation_radians: f32,
    /// Search unit scale and both endpoints (0.25..=4, must contain 1).
    /// Coordinates are nearest-neighbour sampled to preserve source texture.
    pub scale_range: [f32; 2],
    pub mirror: bool,
}
impl Default for FillParams {
    fn default() -> Self {
        Self {
            patch_radius: 3,
            iterations: 5,
            seed: 1,
            sampling: SamplingArea::Auto,
            colour_adaptation: ColourAdaptation::Default,
            output_new_layer: false,
            rotation_radians: 0.0,
            scale_range: [1.0, 1.0],
            mirror: false,
        }
    }
}
pub struct FillResult {
    pub composite: Raster,
    /// Straight RGBA paint layer, transparent outside coverage. Caller inserts
    /// this raster into the document; this function never mutates a layer tree.
    pub new_layer: Option<Raster>,
}

pub(crate) fn validate_mask(mask: &[f32], n: usize) -> EngineResult<()> {
    if mask.len() != n
        || mask
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
    {
        return Err(EngineError::invalid(
            "caf.mask",
            "expected canvas-sized finite coverage in [0,1]",
        ));
    }
    Ok(())
}
fn neighbours(i: usize, w: usize, h: usize) -> impl Iterator<Item = usize> {
    let x = i % w;
    let y = i / w;
    [
        x.checked_sub(1).map(|_| i - 1),
        (x + 1 < w).then_some(i + 1),
        y.checked_sub(1).map(|_| i - w),
        (y + 1 < h).then_some(i + w),
    ]
    .into_iter()
    .flatten()
}
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }
    fn index(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}
#[derive(Clone, Copy, Default)]
struct Match {
    x: i32,
    y: i32,
    t: usize,
}
#[derive(Clone, Default)]
struct Donors {
    runs: Vec<(usize, usize, usize)>,
    total: usize,
}
impl Donors {
    fn get(&self, index: usize, t: usize) -> Match {
        let run = self.runs.partition_point(|&(_, _, end)| end <= index);
        let (x, y, _) = self.runs[run];
        let before = if run == 0 { 0 } else { self.runs[run - 1].2 };
        Match {
            x: (x + index - before) as i32,
            y: y as i32,
            t,
        }
    }
}
#[derive(Clone)]
struct Transform {
    matrix: [f32; 4],
}
impl Transform {
    fn offset(&self, x: i32, y: i32) -> (i32, i32) {
        (
            (self.matrix[0] * x as f32 + self.matrix[1] * y as f32).round() as i32,
            (self.matrix[2] * x as f32 + self.matrix[3] * y as f32).round() as i32,
        )
    }
}
fn transforms(p: &FillParams) -> EngineResult<Vec<Transform>> {
    if !p.rotation_radians.is_finite()
        || !(0.0..=std::f32::consts::PI).contains(&p.rotation_radians)
        || !p.scale_range.iter().all(|v| v.is_finite())
        || !(0.25..=1.0).contains(&p.scale_range[0])
        || !(1.0..=4.0).contains(&p.scale_range[1])
    {
        return Err(EngineError::invalid(
            "caf.transforms",
            "rotation 0..pi, scale range 0.25..4 containing 1 required",
        ));
    }
    let mut angles = vec![0.0];
    if p.rotation_radians > 0.0 {
        angles.extend([
            -p.rotation_radians,
            -p.rotation_radians / 2.0,
            p.rotation_radians / 2.0,
            p.rotation_radians,
        ]);
    }
    let mut scales = vec![1.0];
    for s in p.scale_range {
        if !scales.contains(&s) {
            scales.push(s);
        }
    }
    let mut out = Vec::new();
    for mirror in 0..if p.mirror { 2 } else { 1 } {
        for &scale in &scales {
            for &a in &angles {
                let (s, c) = a.sin_cos();
                let flip = if mirror == 0 { 1.0 } else { -1.0 };
                out.push(Transform {
                    matrix: [scale * c * flip, -scale * s, scale * s * flip, scale * c],
                });
            }
        }
    }
    Ok(out)
}
/// Fill selected pixels from unselected source patches. Alternating NNF
/// propagation plus exponentially shrinking random search minimizes patch SSD.
/// A complete eligible transformed patch must exist or an error is returned.
pub fn fill(
    input: &Raster,
    mask: &[f32],
    params: &FillParams,
    cancel: &AtomicBool,
) -> EngineResult<FillResult> {
    checkpoint(cancel)?;
    let src = Buffer::read(input, cancel)?;
    validate_mask(mask, src.pixels.len())?;
    let ts = transforms(params)?;
    if params.patch_radius == 0
        || params.patch_radius > 16
        || params.iterations == 0
        || params.iterations > 32
    {
        return Err(EngineError::invalid(
            "caf",
            "patch radius 1..16 and iterations 1..32 required",
        ));
    }
    if let SamplingArea::Custom(m) = &params.sampling {
        validate_mask(m, mask.len())?;
    }
    if mask.iter().all(|&m| m == 0.0) {
        return Ok(FillResult {
            composite: input.clone(),
            new_layer: params
                .output_new_layer
                .then(|| Raster::new(input.extent(), 4, Depth::F32, 0.0)),
        });
    }
    let r = params.patch_radius as i32;
    let (w, h) = (src.w, src.h);
    let allowed: Vec<bool> = (0..w * h)
        .map(|i| {
            mask[i] == 0.0
                && match &params.sampling {
                    SamplingArea::Auto => true,
                    SamplingArea::Rect(rect) => {
                        (i % w) as i64 >= rect.x0
                            && ((i % w) as i64) < rect.x1
                            && (i / w) as i64 >= rect.y0
                            && ((i / w) as i64) < rect.y1
                    }
                    SamplingArea::Custom(m) => m[i] > 0.0,
                }
        })
        .collect();
    let footprints: Vec<Vec<(i32, i32)>> = ts
        .iter()
        .map(|t| {
            (-r..=r)
                .flat_map(|y| (-r..=r).map(move |x| t.offset(x, y)))
                .collect()
        })
        .collect();
    // Summed-area exclusion counts make the common, untransformed footprint
    // test constant-time. Other transforms retain their exact rounded footprint.
    let stride = w + 1;
    let mut excluded = vec![0u32; stride * (h + 1)];
    for y in 0..h {
        checkpoint(cancel)?;
        let mut row = 0;
        for x in 0..w {
            row += u32::from(!allowed[y * w + x]);
            excluded[(y + 1) * stride + x + 1] = excluded[y * stride + x + 1] + row;
        }
    }
    let valid = |q: Match| {
        if q.t == 0 {
            if q.x < r || q.y < r || q.x + r >= w as i32 || q.y + r >= h as i32 {
                return false;
            }
            let (x0, y0) = ((q.x - r) as usize, (q.y - r) as usize);
            let (x1, y1) = ((q.x + r + 1) as usize, (q.y + r + 1) as usize);
            return excluded[y1 * stride + x1] - excluded[y0 * stride + x1]
                == excluded[y1 * stride + x0] - excluded[y0 * stride + x0];
        }
        footprints[q.t].iter().all(|&(dx, dy)| {
            let (x, y) = (q.x + dx, q.y + dy);
            x >= 0 && y >= 0 && x < w as i32 && y < h as i32 && allowed[y as usize * w + x as usize]
        })
    };
    // Store horizontal runs, not one 16-byte Match for every source pixel.
    // Prefix lengths still sample every eligible donor with equal probability.
    let mut donors = vec![Donors::default(); ts.len()];
    for (t, list) in donors.iter_mut().enumerate() {
        for y in 0..h {
            checkpoint(cancel)?;
            let mut start = None;
            for x in 0..=w {
                if x < w
                    && valid(Match {
                        x: x as i32,
                        y: y as i32,
                        t,
                    })
                {
                    start.get_or_insert(x);
                } else if let Some(x0) = start.take() {
                    list.total += x - x0;
                    list.runs.push((x0, y, list.total));
                }
            }
        }
    }
    let available: Vec<usize> = (0..ts.len()).filter(|&t| donors[t].total > 0).collect();
    if available.is_empty() {
        return Err(EngineError::invalid(
            "caf.sampling",
            "no complete unselected source patch",
        ));
    }
    let mut rng = Rng(params.seed);
    let mut bounds = [w, h, 0, 0];
    for (i, &m) in mask.iter().enumerate() {
        if m > 0.0 {
            bounds[0] = bounds[0].min(i % w);
            bounds[1] = bounds[1].min(i / w);
            bounds[2] = bounds[2].max(i % w + 1);
            bounds[3] = bounds[3].max(i / w + 1);
        }
    }
    let bw = bounds[2] - bounds[0];
    let local = |i: usize| (i / w - bounds[1]) * bw + i % w - bounds[0];
    let mut nnf = vec![Match::default(); bw * (bounds[3] - bounds[1])];
    let mut dst = src.clone();
    let mut known: Vec<bool> = mask.iter().map(|&v| v == 0.0).collect();
    // Seed only the selected boundary, then visit the hole, not the canvas.
    // Known pixels remain separate from queued pixels: removed object colours
    // must never participate in front initialization.
    let mut visited = vec![false; nnf.len()];
    let mut queue = VecDeque::new();
    for y in bounds[1]..bounds[3] {
        checkpoint(cancel)?;
        for x in bounds[0]..bounds[2] {
            let i = y * w + x;
            if mask[i] > 0.0 && neighbours(i, w, h).any(|n| known[n]) {
                visited[local(i)] = true;
                queue.push_back(i);
            }
        }
    }
    let mut order = Vec::new();
    while let Some(i) = queue.pop_front() {
        if order.len() % 4096 == 0 {
            checkpoint(cancel)?;
        }
        order.push(i);
        for q in neighbours(i, w, h) {
            if mask[q] > 0.0 && !visited[local(q)] {
                visited[local(q)] = true;
                queue.push_back(q);
            }
        }
    }
    let shifted = |q: Match, i: usize, n: usize| {
        let (dx, dy) = ts[q.t].offset(
            (i % w) as i32 - (n % w) as i32,
            (i / w) as i32 - (n / w) as i32,
        );
        Match {
            x: q.x + dx,
            y: q.y + dy,
            ..q
        }
    };
    for &i in &order {
        checkpoint(cancel)?;
        let mut best = donors[available[0]].get(0, available[0]);
        let mut cost = f32::INFINITY;
        let mut consider = |q: Match| {
            if cost == 0.0 {
                return true;
            }
            let e = patch_cost(&src, &dst, &known, i, q, r, &footprints[q.t]);
            if e < cost {
                cost = e;
                best = q;
            }
            cost == 0.0
        };
        // Coherent neighbours are usually much better than a random seed.
        for n in neighbours(i, w, h) {
            if mask[n] > 0.0 && known[n] {
                let q = shifted(nnf[local(n)], i, n);
                if valid(q) && consider(q) {
                    break;
                }
            }
        }
        if cost > 0.0 {
            'random: for &t in &available {
                for _ in 0..64 {
                    let q = donors[t].get(rng.index(donors[t].total), t);
                    let e = patch_cost(&src, &dst, &known, i, q, r, &footprints[q.t]);
                    if e < cost {
                        cost = e;
                        best = q;
                    }
                    if cost == 0.0 {
                        break 'random;
                    }
                }
            }
        }
        nnf[local(i)] = best;
        dst.pixels[i] = src.at(best.x, best.y);
        known[i] = true;
    }
    // Raster forward/backward propagation, then shrinking random search. The
    // NNF stores translation AND transform, propagated in source coordinates.
    order.sort_unstable();
    for iteration in 0..params.iterations {
        for k in 0..order.len() {
            checkpoint(cancel)?;
            let i = order[if iteration % 2 == 0 {
                k
            } else {
                order.len() - 1 - k
            }];
            let mut best = nnf[local(i)];
            let mut cost = patch_cost(&src, &dst, &known, i, best, r, &footprints[best.t]);
            if cost == 0.0 {
                continue;
            }
            for n in neighbours(i, w, h) {
                if mask[n] > 0.0 {
                    let q = shifted(nnf[local(n)], i, n);
                    if valid(q) {
                        let e = patch_cost(&src, &dst, &known, i, q, r, &footprints[q.t]);
                        if e < cost {
                            best = q;
                            cost = e;
                        }
                    }
                }
            }
            let mut radius = w.max(h) as i32;
            while radius > 0 && cost > 0.0 {
                let q = Match {
                    x: best.x + rng.index((2 * radius + 1) as usize) as i32 - radius,
                    y: best.y + rng.index((2 * radius + 1) as usize) as i32 - radius,
                    t: best.t,
                };
                for t in [best.t, available[rng.index(available.len())]] {
                    let q = Match { t, ..q };
                    if valid(q) {
                        let e = patch_cost(&src, &dst, &known, i, q, r, &footprints[q.t]);
                        if e < cost {
                            best = q;
                            cost = e;
                        }
                    }
                }
                radius /= 2;
            }
            nnf[local(i)] = best;
            dst.pixels[i] = src.at(best.x, best.y);
        }
    }
    if params.colour_adaptation != ColourAdaptation::None {
        let mut guide = dst.clone();
        let edge = (bounds[1].saturating_sub(1)..(bounds[3] + 1).min(h)).flat_map(|y| {
            (bounds[0].saturating_sub(1)..(bounds[2] + 1).min(w)).map(move |x| y * w + x)
        });
        for i in edge {
            if mask[i] == 0.0 {
                let mut sum = [0.0; 4];
                let mut n = 0.0;
                for j in neighbours(i, w, h) {
                    if mask[j] > 0.0 {
                        let q = shifted(nnf[local(j)], i, j);
                        let p = src.at(q.x, q.y);
                        for c in 0..4 {
                            sum[c] += p[c];
                        }
                        n += 1.0;
                    }
                }
                if n > 0.0 {
                    for (c, value) in sum.iter().enumerate() {
                        guide.pixels[i][c] = value / n;
                    }
                }
            }
        }
        gradient_blend(
            &src,
            &guide,
            &mut dst,
            mask,
            params.colour_adaptation,
            cancel,
        )?;
    }
    finish(input, &src, &dst, mask, params.output_new_layer, cancel)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveMode {
    Move,
    Extend,
}
/// Cut/copy a selection, fill its original hole in Move mode, then solve the
/// pasted seam against that background. Sampling always reads the immutable
/// pre-move image; overlapping moves cannot feed pasted pixels back into CAF.
/// Integer offsets preserve source detail. Out-of-canvas pixels are clipped.
pub fn move_or_extend(
    input: &Raster,
    mask: &[f32],
    offset: [i32; 2],
    mode: MoveMode,
    params: &FillParams,
    seam: ColourAdaptation,
    cancel: &AtomicBool,
) -> EngineResult<FillResult> {
    checkpoint(cancel)?;
    let src = Buffer::read(input, cancel)?;
    validate_mask(mask, src.pixels.len())?;
    if offset == [0, 0] || mask.iter().all(|&m| m == 0.0) {
        return Ok(FillResult {
            composite: input.clone(),
            new_layer: params
                .output_new_layer
                .then(|| Raster::new(input.extent(), 4, Depth::F32, 0.0)),
        });
    }
    let (w, h) = (src.w, src.h);
    let mut pasted = vec![0.0; w * h];
    let mut guide = src.clone();
    for y in 0..h {
        checkpoint(cancel)?;
        for x in 0..w {
            let (sx, sy) = (
                x as i64 - i64::from(offset[0]),
                y as i64 - i64::from(offset[1]),
            );
            if sx >= 0 && sy >= 0 && sx < w as i64 && sy < h as i64 {
                let i = y * w + x;
                let q = sy as usize * w + sx as usize;
                pasted[i] = mask[q];
                guide.pixels[i] = src.pixels[q];
            }
        }
    }
    let background = if mode == MoveMode::Move {
        let mut p = params.clone();
        p.output_new_layer = false;
        Buffer::read(&fill(input, mask, &p, cancel)?.composite, cancel)?
    } else {
        src.clone()
    };
    let mut paint = guide.clone();
    gradient_blend(&background, &guide, &mut paint, &pasted, seam, cancel)?;
    let mut result = background.clone();
    let mut coverage = vec![0.0; w * h];
    for i in 0..w * h {
        let a = pasted[i];
        let b = if mode == MoveMode::Move { mask[i] } else { 0.0 };
        coverage[i] = a + b - a * b;
        for c in 0..4 {
            result.pixels[i][c] = if a == 1.0 {
                paint.pixels[i][c]
            } else {
                background.pixels[i][c] + a * (paint.pixels[i][c] - background.pixels[i][c])
            };
        }
    }
    // finish applies coverage once; unmix the already-composited result first.
    let mut paint = result;
    for (i, &a) in coverage.iter().enumerate() {
        if a > 0.0 && a < 1.0 {
            for c in 0..4 {
                paint.pixels[i][c] = src.pixels[i][c] + (paint.pixels[i][c] - src.pixels[i][c]) / a;
            }
        }
    }
    finish(
        input,
        &src,
        &paint,
        &coverage,
        params.output_new_layer,
        cancel,
    )
}
/// Screened Poisson: progressively relax the source-colour anchor while
/// keeping its gradients and the destination's Dirichlet boundary.
fn gradient_blend(
    base: &Buffer,
    guide: &Buffer,
    dst: &mut Buffer,
    mask: &[f32],
    level: ColourAdaptation,
    cancel: &AtomicBool,
) -> EngineResult<()> {
    let lambda = match level {
        ColourAdaptation::None => return Ok(()),
        ColourAdaptation::Default => 1.0,
        ColourAdaptation::High => 0.1,
        ColourAdaptation::VeryHigh => 0.0,
    };
    let inside: Vec<usize> = mask
        .iter()
        .enumerate()
        .filter_map(|(i, &v)| (v > 0.0).then_some(i))
        .collect();
    for _ in 0..(4 * base.w.max(base.h)).clamp(64, 2000) {
        checkpoint(cancel)?;
        let mut delta = 0.0f32;
        for &i in &inside {
            let mut nb = [0; 4];
            let mut count = 0;
            for j in neighbours(i, base.w, base.h) {
                nb[count] = j;
                count += 1;
            }
            let nb = &nb[..count];
            for c in 0..3 {
                let mut sum = lambda * guide.pixels[i][c];
                for &j in nb {
                    sum += if mask[j] > 0.0 {
                        dst.pixels[j][c]
                    } else {
                        base.pixels[j][c]
                    };
                    sum += guide.pixels[i][c] - guide.pixels[j][c];
                }
                let v = sum / (nb.len() as f32 + lambda);
                delta = delta.max((v - dst.pixels[i][c]).abs());
                dst.pixels[i][c] = v;
            }
        }
        if delta < 1e-6 {
            break;
        }
    }
    Ok(())
}
pub(crate) fn finish(
    input: &Raster,
    src: &Buffer,
    paint: &Buffer,
    mask: &[f32],
    new_layer: bool,
    cancel: &AtomicBool,
) -> EngineResult<FillResult> {
    checkpoint(cancel)?;
    let mut composite = input.clone();
    let mut layer = new_layer.then(|| Raster::new(input.extent(), 4, Depth::F32, 0.0));
    let revision = input
        .max_rev()
        .checked_add(1)
        .ok_or_else(|| EngineError::invalid("filters", "revision overflow"))?;
    let (nx, ny) = input.grid();
    // Preserve COW tiles outside coverage. Writing a small fill must not copy
    // and re-encode an entire canvas (or allocate a canvas-sized paint layer).
    for ty in 0..ny {
        for tx in 0..nx {
            checkpoint(cancel)?;
            let x0 = tx as usize * 256;
            let y0 = ty as usize * 256;
            let x1 = (x0 + 256).min(src.w);
            let y1 = (y0 + 256).min(src.h);
            let mut touched = false;
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = y * src.w + x;
                    let m = mask[i];
                    if m > 0.0 {
                        touched = true;
                        for c in 0..4 {
                            let v = if m == 1.0 {
                                paint.pixels[i][c]
                            } else {
                                src.pixels[i][c] + m * (paint.pixels[i][c] - src.pixels[i][c])
                            };
                            if !v.is_finite() || !paint.pixels[i][c].is_finite() {
                                return Err(EngineError::invalid("filters", "nonfinite result"));
                            }
                        }
                    }
                }
            }
            if !touched {
                continue;
            }
            let rect = Rect::new(x0 as i64, y0 as i64, x1 as i64, y1 as i64);
            composite.edit_region(rect, revision, |x, y, p| {
                let i = y as usize * src.w + x as usize;
                let m = mask[i];
                if m > 0.0 {
                    for (c, value) in p.iter_mut().enumerate() {
                        *value = if m == 1.0 {
                            paint.pixels[i][c]
                        } else {
                            src.pixels[i][c] + m * (paint.pixels[i][c] - src.pixels[i][c])
                        };
                    }
                }
            })?;
            if let Some(layer) = &mut layer {
                layer.edit_region(rect, 1, |x, y, p| {
                    let i = y as usize * src.w + x as usize;
                    if mask[i] > 0.0 {
                        *p = paint.pixels[i];
                        p[3] *= mask[i];
                    }
                })?;
            }
        }
    }
    checkpoint(cancel)?;
    Ok(FillResult {
        composite,
        new_layer: layer,
    })
}
fn patch_cost(
    src: &Buffer,
    dst: &Buffer,
    known: &[bool],
    i: usize,
    q: Match,
    r: i32,
    footprint: &[(i32, i32)],
) -> f32 {
    let mut sum = 0.0;
    let mut count = 0.0;
    let (x, y) = ((i % src.w) as i32, (i / src.w) as i32);
    for dy in -r..=r {
        for dx in -r..=r {
            let (tx, ty) = (x + dx, y + dy);
            if tx < 0 || ty < 0 || tx >= src.w as i32 || ty >= src.h as i32 || (dx == 0 && dy == 0)
            {
                continue;
            }
            let j = ty as usize * src.w + tx as usize;
            if !known[j] {
                continue;
            }
            let (sx, sy) = footprint[((dy + r) * (2 * r + 1) + dx + r) as usize];
            let a = dst.pixels[j];
            let b = src.at(q.x + sx, q.y + sy);
            for c in 0..3 {
                sum += (a[c] - b[c]).powi(2);
            }
            count += 3.0;
        }
    }
    if count == 0.0 { 0.0 } else { sum / count }
}
