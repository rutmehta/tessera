//! Gradient-domain (Poisson) healing and the patch tool.

use compositor::{Raster, Rect};
use engine_api::{EngineError, EngineResult};

use crate::pixels::Pixels;

/// SOR iterations for a `w × h` region.
pub fn default_iterations(w: usize, h: usize) -> usize {
    (3 * w.max(h)).clamp(64, 4000)
}

/// Solves `Δf = Δg` on `omega` with `f = dest` on its boundary (Dirichlet)
/// by successive over-relaxation, in place in `dest`. Pixels outside the
/// buffer are treated as reflecting (Neumann). `channels` (≤ 4) leading
/// channels are solved; the rest keep `dest`.
///
/// The initial guess is `g` shifted by the mean boundary difference, so a
/// source that differs from the destination only by a constant converges
/// immediately.
pub fn poisson_blend(
    dest: &mut [[f32; 4]],
    guide: &[[f32; 4]],
    omega: &[bool],
    w: usize,
    h: usize,
    channels: usize,
    iterations: usize,
) {
    let n = w * h;
    assert!(dest.len() == n && guide.len() == n && omega.len() == n);
    let channels = channels.min(4);
    let neighbours = |i: usize| {
        let (x, y) = (i % w, i / w);
        let mut v = [usize::MAX; 4];
        if x > 0 {
            v[0] = i - 1;
        }
        if x + 1 < w {
            v[1] = i + 1;
        }
        if y > 0 {
            v[2] = i - w;
        }
        if y + 1 < h {
            v[3] = i + w;
        }
        v
    };
    // Mean boundary offset.
    let mut off = [0.0f64; 4];
    let mut cnt = 0usize;
    for i in 0..n {
        if omega[i] {
            continue;
        }
        if neighbours(i).iter().any(|&q| q != usize::MAX && omega[q]) {
            for c in 0..channels {
                off[c] += f64::from(dest[i][c] - guide[i][c]);
            }
            cnt += 1;
        }
    }
    if cnt > 0 {
        for o in &mut off[..channels] {
            *o /= cnt as f64;
        }
    }
    let inside: Vec<usize> = (0..n).filter(|&i| omega[i]).collect();
    for &i in &inside {
        for c in 0..channels {
            dest[i][c] = guide[i][c] + off[c] as f32;
        }
    }
    let omega_sor = 1.9f32;
    for _ in 0..iterations {
        let mut delta = 0.0f32;
        for &i in &inside {
            let nb = neighbours(i);
            let k = nb.iter().filter(|&&q| q != usize::MAX).count() as f32;
            if k == 0.0 {
                continue;
            }
            for c in 0..channels {
                let mut sum = 0.0f32;
                for &q in &nb {
                    if q != usize::MAX {
                        sum += dest[q][c] + guide[i][c] - guide[q][c];
                    }
                }
                let upd = sum / k - dest[i][c];
                dest[i][c] += omega_sor * upd;
                delta = delta.max(upd.abs());
            }
        }
        if delta < 1e-6 {
            break;
        }
    }
}

/// Patch tool: replaces the region selected by `mask` (1-channel,
/// canvas-sized; > 0.5 is inside) with content sampled at `p + offset`,
/// Poisson-blended into the surroundings. Soft mask values blend the healed
/// result with the original. Returns the changed rect.
pub fn patch(
    target: &mut Raster,
    mask: &Raster,
    offset: [f32; 2],
    rev: u64,
) -> EngineResult<Option<Rect>> {
    if mask.channels() != 1 || mask.extent() != target.extent() {
        return Err(EngineError::invalid(
            "mask",
            "must be single-channel, canvas-sized",
        ));
    }
    if !offset.iter().all(|v| v.is_finite()) {
        return Err(EngineError::invalid("offset", "non-finite"));
    }
    let canvas = Rect::of_extent(target.extent());
    let Some(coarse) = mask.bounds() else {
        return Ok(None);
    };
    let mut m = Pixels::new(mask);
    let mut tight = Rect::default();
    for y in coarse.y0..coarse.y1 {
        for x in coarse.x0..coarse.x1 {
            if m.get(x, y)[0] > 0.0 {
                tight = tight.union(&Rect::new(x, y, x + 1, y + 1));
            }
        }
    }
    if tight.is_empty() {
        return Ok(None);
    }
    let rr = tight.inflate(1).intersect(&canvas);
    let (w, h) = (rr.width() as usize, rr.height() as usize);
    let ch = target.channels();
    let expand = |p: [f32; 4]| match ch {
        1 => [p[0], p[0], p[0], 1.0],
        3 => [p[0], p[1], p[2], 1.0],
        _ => p,
    };
    let mut t = Pixels::new(target);
    let mut omega = vec![false; w * h];
    let mut alpha = vec![0.0f32; w * h];
    let mut dest = vec![[0.0f32; 4]; w * h];
    let mut guide = vec![[0.0f32; 4]; w * h];
    for y in rr.y0..rr.y1 {
        for x in rr.x0..rr.x1 {
            let i = (y - rr.y0) as usize * w + (x - rr.x0) as usize;
            let a = m.get(x, y)[0].clamp(0.0, 1.0);
            alpha[i] = a;
            omega[i] = a > 0.5;
            dest[i] = expand(t.get(x, y));
            guide[i] = expand(t.sample(x as f32 + 0.5 + offset[0], y as f32 + 0.5 + offset[1]));
        }
    }
    let orig = dest.clone();
    let solve = if ch == 1 { 1 } else { 3 };
    poisson_blend(
        &mut dest,
        &guide,
        &omega,
        w,
        h,
        solve,
        default_iterations(w, h),
    );
    // Soft selection edges outside Ω still get a partial copy of the
    // nearest healed values through the alpha blend below.
    target.edit_region(rr, rev, |x, y, p| {
        let i = (i64::from(y) - rr.y0) as usize * w + (i64::from(x) - rr.x0) as usize;
        let a = alpha[i];
        if a <= 0.0 {
            return;
        }
        let f = if omega[i] { dest[i] } else { guide[i] };
        let n = if ch == 1 { 1 } else { 3 };
        for c in 0..n {
            p[c] = orig[i][c] + (f[c] - orig[i][c]) * a;
        }
    })?;
    Ok(Some(rr))
}
