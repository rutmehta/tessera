//! Deterministic forward-energy seam carving of premultiplied planar RGBA.
use crate::{Error, Image, Result, check_cancel};
use engine_api::jobs::CancellationToken;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContentAwareScale {
    pub target_width: usize,
    pub target_height: usize,
    pub amount: f32,
    /// Protection sampled on the supplied input grid, including at mip levels.
    pub protect: Option<Vec<f32>>,
}

pub fn apply(input: &Image, params: &ContentAwareScale) -> Result<Image> {
    apply_with_cancel(input, params, &CancellationToken::new())
}

/// Apply content-aware scaling with cooperative cancellation during seam work.
pub fn apply_with_cancel(
    input: &Image,
    params: &ContentAwareScale,
    cancel: &CancellationToken,
) -> Result<Image> {
    let mut check = || check_cancel(cancel);
    apply_checked(input, params, &mut check)
}

fn apply_checked(
    input: &Image,
    params: &ContentAwareScale,
    check: &mut impl FnMut() -> Result<()>,
) -> Result<Image> {
    check()?;
    validate(input, params)?;
    let mut output = Image::new(input.width, input.height, input.planes.clone())?;
    let mut mask = params
        .protect
        .clone()
        .unwrap_or_else(|| vec![0.0; input.width * input.height]);
    if params.target_width == 0 {
        return Err(Error::Invalid("zero target width".into()));
    }
    let intermediate = |start: usize, target: usize| {
        if params.amount == 1.0 {
            target
        } else {
            let seams = (start.abs_diff(target) as f64 * f64::from(params.amount)).round() as usize;
            if target >= start {
                start + seams
            } else {
                start - seams
            }
        }
    };
    let width = intermediate(input.width, params.target_width);
    let height = intermediate(input.height, params.target_height);
    (output, mask) = resize_width_checked(output, mask, width, check)?;
    check()?;
    if output.height != height {
        (output, mask) = transpose_checked(&output, &mask, check)?;
        (output, mask) = resize_width_checked(output, mask, height, check)?;
        (output, _) = transpose_checked(&output, &mask, check)?;
    }
    check()?;
    if output.width != params.target_width || output.height != params.target_height {
        return resample_checked(&output, params.target_width, params.target_height, check);
    }
    check()?;
    Ok(output)
}

/// Apply optional caller-supplied skin protection. The hook receives source
/// `(x, y, premultiplied_rgba)` exactly once per source pixel, in row order,
/// and returns a finite protection score in [0, 1]. Scores are combined with
/// the explicit mask using max and travel with the pixels during carving.
/// No built-in detector or color-space/skin-tone assumptions are imposed.
pub fn apply_with_skin_protection<F>(
    input: &Image,
    params: &ContentAwareScale,
    hook: F,
) -> Result<Image>
where
    F: FnMut(usize, usize, [f32; 4]) -> f32,
{
    apply_with_skin_protection_and_cancel(input, params, hook, &CancellationToken::new())
}

/// Apply caller-supplied skin protection with cancellation during hook generation and carving.
pub fn apply_with_skin_protection_and_cancel<F>(
    input: &Image,
    params: &ContentAwareScale,
    mut hook: F,
    cancel: &CancellationToken,
) -> Result<Image>
where
    F: FnMut(usize, usize, [f32; 4]) -> f32,
{
    check_cancel(cancel)?;
    validate(input, params)?;
    let mut combined = params.clone();
    let mut mask = combined
        .protect
        .take()
        .unwrap_or_else(|| vec![0.0; input.width * input.height]);
    for y in 0..input.height {
        for x in 0..input.width {
            check_cancel(cancel)?;
            let i = y * input.width + x;
            let score = hook(x, y, std::array::from_fn(|c| input.planes[c][i]));
            if !score.is_finite() || !(0.0..=1.0).contains(&score) {
                return Err(Error::Invalid(
                    "skin protection score must be in [0, 1]".into(),
                ));
            }
            mask[i] = mask[i].max(score);
            check_cancel(cancel)?;
        }
    }
    combined.protect = Some(mask);
    apply_with_cancel(input, &combined, cancel)
}

fn validate(input: &Image, params: &ContentAwareScale) -> Result<()> {
    let invalid = |message: &str| Error::Invalid(message.into());
    if input.width == 0
        || input.height == 0
        || params.target_width == 0
        || params.target_height == 0
    {
        return Err(invalid("content-aware dimensions must be nonzero"));
    }
    let n = input
        .width
        .checked_mul(input.height)
        .ok_or_else(|| invalid("image size overflow"))?;
    let largest = input
        .width
        .max(params.target_width)
        .checked_mul(input.height.max(params.target_height));
    if largest.is_none_or(|n| n > isize::MAX as usize / std::mem::size_of::<f64>()) {
        return Err(invalid("content-aware buffer size overflow"));
    }
    if input
        .planes
        .iter()
        .any(|p| p.len() != n || p.iter().any(|v| !v.is_finite()))
    {
        return Err(invalid("invalid content-aware image planes"));
    }
    if !params.amount.is_finite() || !(0.0..=1.0).contains(&params.amount) {
        return Err(invalid("content-aware amount must be in [0, 1]"));
    }
    if params.protect.as_ref().is_some_and(|p| {
        p.len() != n || p.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
    }) {
        return Err(invalid(
            "protect mask must match source and contain values in [0, 1]",
        ));
    }
    Ok(())
}

fn resample_checked(
    input: &Image,
    width: usize,
    height: usize,
    check: &mut impl FnMut() -> Result<()>,
) -> Result<Image> {
    check()?;
    let mut planes = std::array::from_fn(|_| vec![0.0; width * height]);
    for y in 0..height {
        check()?;
        let sy = ((y as f64 + 0.5) * input.height as f64 / height as f64 - 0.5)
            .clamp(0.0, (input.height - 1) as f64);
        let y0 = sy.floor() as usize;
        let y1 = (y0 + 1).min(input.height - 1);
        let fy = sy - y0 as f64;
        for x in 0..width {
            if x & 1023 == 0 {
                check()?;
            }
            let sx = ((x as f64 + 0.5) * input.width as f64 / width as f64 - 0.5)
                .clamp(0.0, (input.width - 1) as f64);
            let x0 = sx.floor() as usize;
            let x1 = (x0 + 1).min(input.width - 1);
            let fx = sx - x0 as f64;
            for (c, plane) in planes.iter_mut().enumerate() {
                let p = &input.planes[c];
                let top = f64::from(p[y0 * input.width + x0]) * (1.0 - fx)
                    + f64::from(p[y0 * input.width + x1]) * fx;
                let bottom = f64::from(p[y1 * input.width + x0]) * (1.0 - fx)
                    + f64::from(p[y1 * input.width + x1]) * fx;
                plane[y * width + x] = (top * (1.0 - fy) + bottom * fy) as f32;
            }
        }
    }
    check()?;
    Image::new(width, height, planes)
}

fn resize_width_checked(
    mut image: Image,
    mut mask: Vec<f32>,
    target: usize,
    check: &mut impl FnMut() -> Result<()>,
) -> Result<(Image, Vec<f32>)> {
    while image.width > target {
        check()?;
        let seam = find_seam_checked(&image, &mask, check)?;
        (image, mask) = remove_checked(&image, &mask, &seam, check)?;
    }
    while image.width < target {
        check()?;
        let count = (target - image.width).min((image.width - 1).max(1));
        (image, mask) = insert_batch_checked(&image, &mask, count, check)?;
    }
    check()?;
    Ok((image, mask))
}

// Discover distinct seams on a shrinking copy, mapping each row back to
// original coordinates. This avoids re-inserting the same low-energy seam
// within a batch; enlargements beyond 2x necessarily use additional batches.
fn insert_batch_checked(
    image: &Image,
    mask: &[f32],
    count: usize,
    check: &mut impl FnMut() -> Result<()>,
) -> Result<(Image, Vec<f32>)> {
    check()?;
    let (w, h) = (image.width, image.height);
    let mut selected = vec![vec![false; w]; h];
    if w == 1 {
        for row in &mut selected {
            check()?;
            row[0] = true;
        }
    } else {
        let mut work = Image::new(w, h, image.planes.clone())?;
        let mut work_mask = mask.to_vec();
        let mut indices: Vec<Vec<usize>> = (0..h).map(|_| (0..w).collect()).collect();
        for _ in 0..count {
            check()?;
            let seam = find_seam_checked(&work, &work_mask, check)?;
            for y in 0..h {
                check()?;
                selected[y][indices[y].remove(seam[y])] = true;
            }
            (work, work_mask) = remove_checked(&work, &work_mask, &seam, check)?;
        }
    }
    let mut planes: [Vec<f32>; 4] = std::array::from_fn(|_| Vec::with_capacity((w + count) * h));
    let mut out_mask = Vec::with_capacity((w + count) * h);
    for (y, selected_row) in selected.iter().enumerate() {
        check()?;
        for (x, &selected) in selected_row.iter().enumerate() {
            if x & 1023 == 0 {
                check()?;
            }
            let i = y * w + x;
            for (c, plane) in planes.iter_mut().enumerate() {
                plane.push(image.planes[c][i]);
            }
            out_mask.push(mask[i]);
            if selected {
                let neighbor = y * w
                    + if x + 1 < w {
                        x + 1
                    } else {
                        x.saturating_sub(1)
                    };
                for (c, plane) in planes.iter_mut().enumerate() {
                    plane.push(
                        (f64::from(image.planes[c][i]) * 0.5
                            + f64::from(image.planes[c][neighbor]) * 0.5)
                            as f32,
                    );
                }
                out_mask.push(mask[i].max(mask[neighbor]));
            }
        }
    }
    check()?;
    Ok((Image::new(w + count, h, planes)?, out_mask))
}

fn transpose_checked(
    image: &Image,
    mask: &[f32],
    check: &mut impl FnMut() -> Result<()>,
) -> Result<(Image, Vec<f32>)> {
    check()?;
    let mut planes = std::array::from_fn(|_| vec![0.0; image.width * image.height]);
    let mut out_mask = vec![0.0; mask.len()];
    for y in 0..image.height {
        check()?;
        for x in 0..image.width {
            if x & 1023 == 0 {
                check()?;
            }
            let i = y * image.width + x;
            let j = x * image.height + y;
            for (c, plane) in planes.iter_mut().enumerate() {
                plane[j] = image.planes[c][i];
            }
            out_mask[j] = mask[i];
        }
    }
    check()?;
    Ok((Image::new(image.height, image.width, planes)?, out_mask))
}

// RGBA distances include alpha boundaries and chromatic edges. f64 keeps
// finite HDR f32 input from overflowing cumulative path costs.
fn distance(image: &Image, a: usize, b: usize) -> f64 {
    image
        .planes
        .iter()
        .map(|p| (f64::from(p[a]) - f64::from(p[b])).abs())
        .sum()
}

fn find_seam_checked(
    image: &Image,
    mask: &[f32],
    check: &mut impl FnMut() -> Result<()>,
) -> Result<Vec<usize>> {
    check()?;
    let (w, h) = (image.width, image.height);
    let mut energy = vec![0.0f64; w * h];
    let mut max_energy = 1.0f64;
    for y in 0..h {
        for x in 0..w {
            if x & 1023 == 0 {
                check()?;
            }
            let i = y * w + x;
            let dx = distance(
                image,
                y * w + x.saturating_sub(1),
                y * w + (x + 1).min(w - 1),
            );
            let dy = distance(
                image,
                y.saturating_sub(1) * w + x,
                (y + 1).min(h - 1) * w + x,
            );
            energy[i] = dx.hypot(dy);
            max_energy = max_energy.max(energy[i]);
        }
    }
    // Protection is a strong soft cost, not an impossible-path constraint.
    let penalty = max_energy * (h as f64 + 1.0) * 16.0;
    let mut costs = vec![0.0; w * h];
    let mut parent = vec![0; w * h];
    for y in 0..h {
        for x in 0..w {
            if x & 1023 == 0 {
                check()?;
            }
            let i = y * w + x;
            let base = energy[i] + f64::from(mask[i]) * penalty;
            if y == 0 {
                costs[i] = base;
                continue;
            }
            let left = y * w + x.saturating_sub(1);
            let right = y * w + (x + 1).min(w - 1);
            let up = (y - 1) * w + x;
            let straight = distance(image, left, right);
            let mut best = f64::INFINITY;
            let mut from = x;
            for px in x.saturating_sub(1)..=(x + 1).min(w - 1) {
                let extra = if px < x {
                    distance(image, up, left)
                } else if px > x {
                    distance(image, up, right)
                } else {
                    0.0
                };
                let cost = costs[(y - 1) * w + px] + straight + extra;
                if cost < best {
                    best = cost;
                    from = px;
                }
            }
            costs[i] = base + best;
            parent[i] = from;
        }
    }
    let mut x = (0..w)
        .min_by(|&a, &b| costs[(h - 1) * w + a].total_cmp(&costs[(h - 1) * w + b]))
        .unwrap();
    let mut seam = vec![0; h];
    for y in (0..h).rev() {
        if y & 1023 == 0 {
            check()?;
        }
        seam[y] = x;
        x = parent[y * w + x];
    }
    check()?;
    Ok(seam)
}

fn remove_checked(
    image: &Image,
    mask: &[f32],
    seam: &[usize],
    check: &mut impl FnMut() -> Result<()>,
) -> Result<(Image, Vec<f32>)> {
    check()?;
    let mut planes: [Vec<f32>; 4] =
        std::array::from_fn(|_| Vec::with_capacity((image.width - 1) * image.height));
    let mut out_mask = Vec::with_capacity((image.width - 1) * image.height);
    for (y, &sx) in seam.iter().enumerate() {
        for x in 0..image.width {
            if x & 1023 == 0 {
                check()?;
            }
            if x == sx {
                continue;
            }
            let i = y * image.width + x;
            for (c, plane) in planes.iter_mut().enumerate() {
                plane.push(image.planes[c][i]);
            }
            out_mask.push(mask[i]);
        }
    }
    check()?;
    Ok((Image::new(image.width - 1, image.height, planes)?, out_mask))
}

#[cfg(test)]
mod cancellation_tests {
    use super::*;
    use engine_api::jobs::CancellationToken;

    #[test]
    fn seam_search_checks_cancellation_during_its_inner_work() {
        let image = Image::new(4, 4, std::array::from_fn(|_| vec![0.25; 16])).unwrap();
        let mask = vec![0.0; 16];
        let token = CancellationToken::new();
        let mut checks = 0;
        let result = find_seam_checked(&image, &mask, &mut || {
            checks += 1;
            if checks == 3 {
                token.cancel();
            }
            token.check().map_err(|_| Error::Cancelled)
        });
        assert!(matches!(result, Err(Error::Cancelled)));
        assert_eq!(checks, 3);
    }

    #[test]
    fn cancellation_after_width_stops_before_height_work() {
        let image = Image::new(4, 4, std::array::from_fn(|_| vec![0.25; 16])).unwrap();
        let params = ContentAwareScale {
            target_width: 3,
            target_height: 3,
            amount: 1.0,
            protect: None,
        };
        let token = CancellationToken::new();
        let mut reached_width_boundary = false;
        let mut reached_height = false;
        let result = apply_checked(&image, &params, &mut |phase| {
            if phase == SeamCheckpoint::AfterWidth {
                reached_width_boundary = true;
                token.cancel();
            }
            if phase == SeamCheckpoint::BeforeHeight {
                reached_height = true;
            }
            token.check().map_err(|_| Error::Cancelled)
        });
        assert!(reached_width_boundary);
        assert!(!reached_height);
        assert!(matches!(result, Err(Error::Cancelled)));
    }
}
