//! CPU distortion geometry and separable Lanczos-3 reconstruction.
//!
//! Coordinates are pixel centers (integers); the normalized center multiplies
//! `(w-1,h-1)`. Let `R=max(min(w-1,h-1)/2,1)`, `r=|position-center|`.
//! Forward pinch/spherize use `r' = r*(1+b/(1+(r/R)^2))`, with
//! `b=-amount/2` for pinch and `b=amount/2` for spherize. Their derivative
//! stays positive for amounts in [-1,1]. Twirl adds `amount/(1+(r/R)^2)`
//! to the angle. Wave is the shear `x'=x+amount*sin(TAU*y/wavelength+phase)`.
//! Ripple uses `k=TAU/wavelength`, `s=sqrt(r*r+1/(k*k))`, and
//! `r'=r+amount*r/s*sin(k*s+phase)`. This is smooth even at the center;
//! `2*abs(amount)*k < 1` conservatively guarantees positive radial derivative.
//! Radial inverses use 40 bisections; no negative-amount approximation is used.
//! All geometry is evaluated in f64 before returning f32 pixel coordinates.
//!
//! RectangularToPolar forward maps a strip to a disk: angle `TAU*x/(w-1)`,
//! radius `R*y/(h-1)`, with angle zero pointing right and increasing clockwise
//! (image y points down). PolarToRectangular is its reverse. Disk-to-strip uses
//! `atan2(dy,dx) mod TAU`. Round trips exclude the disk center, duplicate strip
//! seam, and negative radii. Offset is addition modulo `(w,h)`; inverse subtracts.
//! Neutral maps are explicit identities, including zero offset independently
//! of amount. Polar modes always transform, regardless of amount.

use std::f64::consts::TAU;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Distortion {
    Pinch,
    Spherize,
    Twirl,
    Wave,
    Ripple,
    PolarToRectangular,
    RectangularToPolar,
    Offset,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DistortParams {
    pub amount: f32,
    pub wavelength: f32,
    pub phase: f32,
    pub offset: [f32; 2],
    pub center: [f32; 2],
}

impl Default for DistortParams {
    fn default() -> Self {
        Self {
            amount: 0.0,
            wavelength: 32.0,
            phase: 0.0,
            offset: [0.0; 2],
            center: [0.5; 2],
        }
    }
}

/// Source-to-destination geometry. Parameters must satisfy [`apply`]'s contract.
/// Radial maps operate on the entire plane, without clamping their coordinates.
pub fn forward_map(
    kind: Distortion,
    p: &DistortParams,
    w: usize,
    h: usize,
    x: f32,
    y: f32,
) -> [f32; 2] {
    map(kind, p, w, h, x, y, false)
}

/// Destination-to-source geometry used for resampling; see [`forward_map`].
pub fn inverse_map(
    kind: Distortion,
    p: &DistortParams,
    w: usize,
    h: usize,
    x: f32,
    y: f32,
) -> [f32; 2] {
    map(kind, p, w, h, x, y, true)
}

fn radial(kind: Distortion, p: &DistortParams, radius: f64, r: f64) -> f64 {
    match kind {
        Distortion::Pinch | Distortion::Spherize => {
            let b = f64::from(p.amount) * if kind == Distortion::Pinch { -0.5 } else { 0.5 };
            r * (1.0 + b / (1.0 + (r / radius).powi(2)))
        }
        Distortion::Ripple => {
            let k = TAU / f64::from(p.wavelength);
            let s = r.hypot(1.0 / k);
            r + f64::from(p.amount) * r / s * (k * s + f64::from(p.phase)).sin()
        }
        _ => r,
    }
}

fn map(
    kind: Distortion,
    p: &DistortParams,
    w: usize,
    h: usize,
    x: f32,
    y: f32,
    inverse: bool,
) -> [f32; 2] {
    if kind == Distortion::Offset && p.offset == [0.0; 2] {
        return [x, y];
    }
    if p.amount == 0.0
        && !matches!(
            kind,
            Distortion::Offset | Distortion::PolarToRectangular | Distortion::RectangularToPolar
        )
    {
        return [x, y];
    }
    let sign = if inverse { -1.0 } else { 1.0 };
    let (x, y) = (f64::from(x), f64::from(y));
    let width = w.saturating_sub(1) as f64;
    let height = h.saturating_sub(1) as f64;
    let cx = f64::from(p.center[0]) * width;
    let cy = f64::from(p.center[1]) * height;
    let radius = (width.min(height) * 0.5).max(1.0);
    let dx = x - cx;
    let dy = y - cy;
    let r = dx.hypot(dy);
    let result = match kind {
        Distortion::Wave => [
            x + sign
                * f64::from(p.amount)
                * (TAU * y / f64::from(p.wavelength) + f64::from(p.phase)).sin(),
            y,
        ],
        Distortion::Twirl => {
            let angle = sign * f64::from(p.amount) / (1.0 + (r / radius).powi(2));
            let (s, c) = angle.sin_cos();
            [cx + c * dx - s * dy, cy + s * dx + c * dy]
        }
        Distortion::Pinch | Distortion::Spherize | Distortion::Ripple => {
            if r == 0.0 {
                return [cx as f32, cy as f32];
            }
            let mapped = if inverse {
                let mut low = 0.0;
                let mut high = if kind == Distortion::Ripple {
                    r + f64::from(p.amount).abs()
                } else {
                    r * 2.0
                };
                for _ in 0..40 {
                    let mid = (low + high) * 0.5;
                    if radial(kind, p, radius, mid) < r {
                        low = mid;
                    } else {
                        high = mid;
                    }
                }
                (low + high) * 0.5
            } else {
                radial(kind, p, radius, r)
            };
            [cx + dx * mapped / r, cy + dy * mapped / r]
        }
        Distortion::Offset => [
            (x + sign * f64::from(p.offset[0]).rem_euclid(w as f64)).rem_euclid(w as f64),
            (y + sign * f64::from(p.offset[1]).rem_euclid(h as f64)).rem_euclid(h as f64),
        ],
        Distortion::PolarToRectangular | Distortion::RectangularToPolar => {
            let to_disk = (kind == Distortion::RectangularToPolar) != inverse;
            if to_disk {
                let angle = TAU * x / width.max(1.0);
                let rad = y * radius / height.max(1.0);
                let (s, c) = angle.sin_cos();
                [cx + rad * c, cy + rad * s]
            } else {
                [
                    dy.atan2(dx).rem_euclid(TAU) * width / TAU,
                    r * height / radius,
                ]
            }
        }
    };
    [result[0] as f32, result[1] as f32]
}

/// Normalized separable Lanczos-3 sampling of all four channels.
/// Taps wrap toroidally for offset; otherwise coordinates/taps clamp to edges.
/// HDR values and negative lobes are retained. Invalid buffers or nonfinite
/// coordinates return transparent black; [`apply`] validates its inputs first.
pub fn sample(pixels: &[[f32; 4]], w: usize, h: usize, x: f32, y: f32, wrap: bool) -> [f32; 4] {
    if w == 0
        || h == 0
        || w.checked_mul(h) != Some(pixels.len())
        || !x.is_finite()
        || !y.is_finite()
    {
        return [0.0; 4];
    }
    let coord = |v: f32, n: usize| {
        if wrap {
            f64::from(v).rem_euclid(n as f64)
        } else {
            f64::from(v).clamp(0.0, (n - 1) as f64)
        }
    };
    let (x, y) = (coord(x, w), coord(y, h));
    let index = |v: f64, n: usize| {
        if wrap {
            v.rem_euclid(n as f64) as usize
        } else {
            v.clamp(0.0, (n - 1) as f64) as usize
        }
    };
    if x.fract() == 0.0 && y.fract() == 0.0 {
        return pixels[index(y, h) * w + index(x, w)];
    }
    let mut sum = [0.0_f64; 4];
    let mut weight_sum = 0.0;
    for j in -2..=3 {
        let sy = y.floor() + f64::from(j);
        let wy = lanczos(y - sy);
        for i in -2..=3 {
            let sx = x.floor() + f64::from(i);
            let weight = wy * lanczos(x - sx);
            if weight == 0.0 {
                continue;
            }
            let pixel = pixels[index(sy, h) * w + index(sx, w)];
            for c in 0..4 {
                sum[c] += weight * f64::from(pixel[c]);
            }
            weight_sum += weight;
        }
    }
    sum.map(|v| (v / weight_sum) as f32)
}

fn lanczos(x: f64) -> f64 {
    let x = x.abs();
    if x == 0.0 {
        return 1.0;
    }
    if x >= 3.0 || x.fract() == 0.0 {
        return 0.0;
    }
    let px = std::f64::consts::PI * x;
    (px.sin() / px) * ((px / 3.0).sin() / (px / 3.0))
}

/// Warp an RGBA image, preserving neutral operations bit-for-bit.
///
/// Dimensions must be positive with exactly `w*h` pixels (polar modes require
/// both dimensions >= 2). All parameters must be finite, wavelength > 0, and
/// center components in [0,1]. Pinch/spherize amount is in [-1,1]; ripple
/// requires `abs(amount) < wavelength/(4*PI)` for a smooth invertible map.
/// Twirl amounts are radians; wave/ripple/offset amounts are pixel distances.
/// Cancellation is checked before allocation, every 256 pixels, and at return.
pub fn apply(
    kind: Distortion,
    p: &DistortParams,
    pixels: &[[f32; 4]],
    w: usize,
    h: usize,
    cancel: &std::sync::atomic::AtomicBool,
) -> engine_api::EngineResult<Vec<[f32; 4]>> {
    use engine_api::EngineError;
    use std::sync::atomic::Ordering;
    let check_cancel = || {
        if cancel.load(Ordering::Relaxed) {
            Err(EngineError::Cancelled)
        } else {
            Ok(())
        }
    };
    check_cancel()?;
    if w == 0 || h == 0 || w.checked_mul(h) != Some(pixels.len()) {
        return Err(EngineError::invalid(
            "dimensions",
            "expected positive dimensions and exactly w*h pixels",
        ));
    }
    if matches!(
        kind,
        Distortion::PolarToRectangular | Distortion::RectangularToPolar
    ) && (w < 2 || h < 2)
    {
        return Err(EngineError::invalid(
            "dimensions",
            "polar transformations require width and height >= 2",
        ));
    }
    for (name, value) in [
        ("amount", p.amount),
        ("wavelength", p.wavelength),
        ("phase", p.phase),
        ("offset.x", p.offset[0]),
        ("offset.y", p.offset[1]),
        ("center.x", p.center[0]),
        ("center.y", p.center[1]),
    ] {
        if !value.is_finite() {
            return Err(EngineError::invalid(name, "must be finite"));
        }
    }
    if p.wavelength <= 0.0 {
        return Err(EngineError::invalid("wavelength", "must be positive"));
    }
    if p.center.iter().any(|v| !(0.0..=1.0).contains(v)) {
        return Err(EngineError::invalid(
            "center",
            "components must be in [0,1]",
        ));
    }
    if matches!(kind, Distortion::Pinch | Distortion::Spherize) && !(-1.0..=1.0).contains(&p.amount)
    {
        return Err(EngineError::invalid(
            "amount",
            "pinch/spherize amount must be in [-1,1]",
        ));
    }
    if kind == Distortion::Ripple
        && 2.0 * f64::from(p.amount).abs() * TAU / f64::from(p.wavelength) >= 1.0
    {
        return Err(EngineError::invalid(
            "amount",
            "invertible ripple requires abs(amount) < wavelength/(4*PI)",
        ));
    }
    let identity = match kind {
        Distortion::Offset => p.offset == [0.0; 2],
        Distortion::PolarToRectangular | Distortion::RectangularToPolar => false,
        _ => p.amount == 0.0,
    };
    let mut output = Vec::new();
    output
        .try_reserve_exact(pixels.len())
        .map_err(|_| EngineError::ResourceExhausted {
            resource: "distortion output buffer".into(),
        })?;
    for (i, pixel) in pixels.iter().enumerate() {
        if i % 256 == 0 {
            check_cancel()?;
        }
        if identity {
            output.push(*pixel);
            continue;
        }
        let q = inverse_map(kind, p, w, h, (i % w) as f32, (i / w) as f32);
        output.push(sample(pixels, w, h, q[0], q[1], kind == Distortion::Offset));
    }
    check_cancel()?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_maps_are_exact_even_outside_image() {
        let p = DistortParams::default();
        for kind in [
            Distortion::Pinch,
            Distortion::Spherize,
            Distortion::Twirl,
            Distortion::Wave,
            Distortion::Ripple,
            Distortion::Offset,
        ] {
            for [x, y] in [[0.0, 0.0], [13.25, 27.5], [-4.25, 110.0]] {
                assert_eq!(forward_map(kind, &p, 101, 73, x, y), [x, y]);
                assert_eq!(inverse_map(kind, &p, 101, 73, x, y), [x, y]);
            }
        }
    }

    #[test]
    fn radial_models_round_trip_at_signed_limits_and_center() {
        for kind in [Distortion::Pinch, Distortion::Spherize, Distortion::Ripple] {
            for amount in [-1.0, -0.65, 0.65, 1.0] {
                let p = DistortParams {
                    amount,
                    wavelength: 19.0,
                    phase: 1.7,
                    ..Default::default()
                };
                for y in [0.0, 18.25, 36.0, 58.7, 72.0] {
                    for x in [0.0, 25.5, 50.0, 74.25, 100.0] {
                        let q = forward_map(kind, &p, 101, 73, x, y);
                        let r = inverse_map(kind, &p, 101, 73, q[0], q[1]);
                        assert!(
                            (r[0] - x).abs() < 0.001 && (r[1] - y).abs() < 0.001,
                            "{kind:?}, {amount}: {r:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn apply_rejects_invalid_parameters_and_dimensions() {
        use engine_api::EngineError;
        let pixels = [[0.0; 4]; 4];
        let cancel = std::sync::atomic::AtomicBool::new(false);
        for p in [
            DistortParams {
                amount: f32::NAN,
                ..Default::default()
            },
            DistortParams {
                wavelength: 0.0,
                ..Default::default()
            },
            DistortParams {
                wavelength: -1.0,
                ..Default::default()
            },
            DistortParams {
                wavelength: f32::INFINITY,
                ..Default::default()
            },
            DistortParams {
                phase: f32::INFINITY,
                ..Default::default()
            },
            DistortParams {
                offset: [0.0, f32::NAN],
                ..Default::default()
            },
            DistortParams {
                center: [-0.1, 0.5],
                ..Default::default()
            },
            DistortParams {
                center: [0.5, 1.1],
                ..Default::default()
            },
            DistortParams {
                center: [f32::NAN, 0.5],
                ..Default::default()
            },
            DistortParams {
                amount: 1.01,
                ..Default::default()
            },
            DistortParams {
                amount: -1.01,
                ..Default::default()
            },
        ] {
            assert!(matches!(
                apply(Distortion::Pinch, &p, &pixels, 2, 2, &cancel),
                Err(EngineError::InvalidArgument { .. })
            ));
        }
        let p = DistortParams {
            amount: 4.0,
            wavelength: 16.0,
            ..Default::default()
        };
        assert!(apply(Distortion::Ripple, &p, &pixels, 2, 2, &cancel).is_err());
        for (w, h) in [(0, 0), (1, 1), (usize::MAX, 2)] {
            assert!(
                apply(
                    Distortion::Twirl,
                    &DistortParams::default(),
                    &pixels,
                    w,
                    h,
                    &cancel
                )
                .is_err()
            );
        }
        assert!(
            apply(
                Distortion::PolarToRectangular,
                &DistortParams::default(),
                &pixels,
                1,
                4,
                &cancel
            )
            .is_err()
        );
    }

    #[test]
    fn cancellation_wins_even_for_identity() {
        let cancel = std::sync::atomic::AtomicBool::new(true);
        for amount in [0.0, 0.5] {
            let p = DistortParams {
                amount,
                ..Default::default()
            };
            assert_eq!(
                apply(Distortion::Twirl, &p, &[[0.0; 4]; 4], 2, 2, &cancel),
                Err(engine_api::EngineError::Cancelled)
            );
        }
    }

    #[test]
    fn apply_preserves_identities_and_warps_all_kinds() {
        let pixels: Vec<_> = (0..63)
            .map(|i| [i as f32 / 63.0, (i % 9) as f32, (i / 9) as f32, 0.5])
            .collect();
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let neutral = DistortParams::default();
        for kind in [
            Distortion::Pinch,
            Distortion::Spherize,
            Distortion::Twirl,
            Distortion::Wave,
            Distortion::Ripple,
            Distortion::Offset,
        ] {
            assert_eq!(
                apply(kind, &neutral, &pixels, 9, 7, &cancel).unwrap(),
                pixels
            );
        }
        let p = DistortParams {
            amount: 0.8,
            wavelength: 16.0,
            phase: 0.7,
            offset: [2.0, -1.0],
            ..Default::default()
        };
        for kind in [
            Distortion::Pinch,
            Distortion::Spherize,
            Distortion::Twirl,
            Distortion::Wave,
            Distortion::Ripple,
            Distortion::Offset,
            Distortion::PolarToRectangular,
            Distortion::RectangularToPolar,
        ] {
            let output = apply(kind, &p, &pixels, 9, 7, &cancel).unwrap();
            assert_ne!(output, pixels, "{kind:?}");
            assert!(output.iter().flatten().all(|x| x.is_finite()));
            if kind == Distortion::Offset {
                assert_eq!(output[0], pixels[16]);
            }
        }
    }

    #[test]
    fn lanczos_preserves_constant_rgba_and_integer_samples() {
        let constant = [0.23, -0.5, 4.0, 0.7];
        let pixels = vec![constant; 63];
        for wrap in [false, true] {
            for [x, y] in [[0.25, 0.75], [-3.1, 8.9], [8.75, 6.25], [4.4, 2.3]] {
                let q = sample(&pixels, 9, 7, x, y, wrap);
                for c in 0..4 {
                    assert!((q[c] - constant[c]).abs() < 1e-6);
                }
            }
            let pixels: Vec<_> = (0..63)
                .map(|i| [i as f32, (i * i) as f32, -0.25, 0.5])
                .collect();
            for y in 0..7 {
                for x in 0..9 {
                    assert_eq!(
                        sample(&pixels, 9, 7, x as f32, y as f32, wrap),
                        pixels[y * 9 + x]
                    );
                }
            }
            assert_eq!(
                sample(&pixels, 9, 7, -1.0, 0.0, wrap),
                pixels[if wrap { 8 } else { 0 }]
            );
        }
        assert_eq!(sample(&[], 0, 0, 0.0, 0.0, false), [0.0; 4]);
    }

    #[test]
    fn lanczos_has_negative_lobes_not_bilinear() {
        let mut pixels = vec![[0.0; 4]; 9];
        pixels[4] = [1.0; 4];
        let q = sample(&pixels, 9, 1, 5.5, 0.0, false);
        assert!(q[0] < -0.1 && q[0] > -0.2);
    }

    #[test]
    fn polar_modes_and_offset_round_trip_non_degenerate_grid() {
        let p = DistortParams {
            offset: [21.25, -12.5],
            ..Default::default()
        };
        for y in [7.25, 18.0, 43.5, 63.25] {
            for x in [9.25, 26.0, 61.5, 89.75] {
                let disk = forward_map(Distortion::RectangularToPolar, &p, 101, 73, x, y);
                assert!((disk[0] - x).abs() + (disk[1] - y).abs() > 0.1);
                let strip = inverse_map(
                    Distortion::RectangularToPolar,
                    &p,
                    101,
                    73,
                    disk[0],
                    disk[1],
                );
                let strip2 = forward_map(
                    Distortion::PolarToRectangular,
                    &p,
                    101,
                    73,
                    disk[0],
                    disk[1],
                );
                let disk2 = inverse_map(Distortion::PolarToRectangular, &p, 101, 73, x, y);
                for q in [strip, strip2] {
                    assert!((q[0] - x).abs() < 0.001 && (q[1] - y).abs() < 0.001);
                }
                assert_eq!(disk, disk2);
                let q = forward_map(Distortion::Offset, &p, 101, 73, x, y);
                assert!((q[0] - (x + 21.25).rem_euclid(101.0)).abs() < 0.001);
                assert!((q[1] - (y - 12.5).rem_euclid(73.0)).abs() < 0.001);
                let r = inverse_map(Distortion::Offset, &p, 101, 73, q[0], q[1]);
                assert!((r[0] - x).abs() < 0.001 && (r[1] - y).abs() < 0.001);
            }
        }
    }

    #[test]
    fn radial_and_wave_maps_are_nontrivial_and_round_trip() {
        let p = DistortParams {
            amount: 0.65,
            wavelength: 19.0,
            phase: 0.4,
            center: [0.43, 0.57],
            ..Default::default()
        };
        for kind in [
            Distortion::Pinch,
            Distortion::Spherize,
            Distortion::Twirl,
            Distortion::Wave,
            Distortion::Ripple,
        ] {
            let mut moved = false;
            for y in [3.25, 17.5, 39.25, 62.75] {
                for x in [2.75, 23.5, 59.25, 94.5] {
                    let q = forward_map(kind, &p, 101, 73, x, y);
                    moved |= (q[0] - x).abs() + (q[1] - y).abs() > 0.01;
                    let r = inverse_map(kind, &p, 101, 73, q[0], q[1]);
                    assert!(
                        (r[0] - x).abs() < 0.001 && (r[1] - y).abs() < 0.001,
                        "{kind:?}: {x},{y} -> {q:?} -> {r:?}"
                    );
                    let q = inverse_map(kind, &p, 101, 73, x, y);
                    let r = forward_map(kind, &p, 101, 73, q[0], q[1]);
                    assert!((r[0] - x).abs() < 0.001 && (r[1] - y).abs() < 0.001);
                }
            }
            assert!(moved, "{kind:?} was identity");
        }
    }
}
