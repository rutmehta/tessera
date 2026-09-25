//! M2 crop/straighten and coordinate-stable creative effects.
use engine_api::{
    EngineError, EngineResult,
    recipe::settings::{EffectsSettings, GeometrySettings},
    tile::{Extent, Tile},
};

/// Crop and straighten in one inverse map, using normalized Lanczos3.
/// Positive angles rotate the displayed image clockwise. See GEOMETRY_EFFECTS_M2.md.
pub fn geometry(image: &crate::Image, s: &GeometrySettings) -> EngineResult<crate::Image> {
    geometry_mapped(image, s, false, |p, _| Some(p))
}

pub(crate) fn geometry_mapped(
    image: &crate::Image,
    s: &GeometrySettings,
    lens_active: bool,
    lookup: impl Fn([f64; 2], usize) -> Option<[f64; 2]>,
) -> EngineResult<crate::Image> {
    let r = s.crop.rect;
    if !r.is_valid()
        || !s.crop.angle.is_finite()
        || !(-45.0..=45.0).contains(&s.crop.angle)
        || s.crop.aspect.is_some_and(|a| a.contains(&0))
    {
        return Err(EngineError::invalid(
            "crop",
            "valid rectangle, nonzero aspect and angle in -45..=45 required",
        ));
    }
    if s.orientation != 1 || s.constrain_crop {
        return Err(EngineError::Unsupported {
            what: "EXIF orientation and constrain-crop are not implemented".into(),
        });
    }
    let upright = crate::upright::inverse(image, s, &lookup)?;
    let upright_active = upright != lens::Homography::IDENTITY;
    let lens_active = lens_active || upright_active;
    let t = &s.transform;
    if [
        t.vertical,
        t.horizontal,
        t.rotate,
        t.aspect,
        t.scale,
        t.offset_x,
        t.offset_y,
    ]
    .iter()
    .any(|v| !v.is_finite())
        || !(50.0..=150.0).contains(&t.scale)
    {
        return Err(EngineError::invalid(
            "transform",
            "finite controls and scale 50..150 required",
        ));
    }
    let transform_active = *t != Default::default();
    if r == engine_api::recipe::settings::NormalizedRect::FULL
        && s.crop.angle == 0.
        && !lens_active
        && !transform_active
    {
        return Ok(image.clone());
    }
    let (iw, ih) = (image.width() as f32, image.height() as f32);
    let (cw, ch) = ((r.right - r.left) * iw, (r.bottom - r.top) * ih);
    let (w, h) = (cw.round().max(1.) as u32, ch.round().max(1.) as u32);
    let (cx, cy) = ((r.left + r.right) * iw / 2., (r.top + r.bottom) * ih / 2.);
    let (sin, cos) = s.crop.angle.to_radians().sin_cos();
    let mut planes = vec![vec![0.; w as usize * h as usize]; image.planes().len()];
    for y in 0..h {
        for x in 0..w {
            let dx = (x as f32 + 0.5) * cw / w as f32 - cw / 2.;
            let dy = (y as f32 + 0.5) * ch / h as f32 - ch / 2.;
            let sx = cx + cos * dx + sin * dy - 0.5;
            let sy = cy - sin * dx + cos * dy - 0.5;
            for (channel, (src, dst)) in image.planes().iter().zip(&mut planes).enumerate() {
                let (mut sx, mut sy) = (sx, sy);
                if transform_active || lens_active {
                    let mut p = [
                        2. * (sx as f64 + 0.5) / iw as f64 - 1.,
                        2. * (sy as f64 + 0.5) / ih as f64 - 1.,
                    ];
                    if transform_active {
                        p[0] -= t.offset_x.clamp(-100., 100.) as f64 / 50.;
                        p[1] -= t.offset_y.clamp(-100., 100.) as f64 / 50.;
                        let (sin, cos) = (t.rotate.clamp(-10., 10.) as f64).to_radians().sin_cos();
                        // Rotate in pixel metric, not stretched normalized coordinates.
                        p = [
                            cos * p[0] + sin * p[1] * ih as f64 / iw as f64,
                            -sin * p[0] * iw as f64 / ih as f64 + cos * p[1],
                        ];
                        p[0] /= t.scale as f64 / 100.
                            * (t.aspect.clamp(-100., 100.) as f64 / 100.).exp2();
                        p[1] /= t.scale as f64 / 100.;
                        let d = 1.
                            - t.horizontal.clamp(-100., 100.) as f64 / 200. * p[0]
                            - t.vertical.clamp(-100., 100.) as f64 / 200. * p[1];
                        if d.abs() < 1e-8 {
                            continue;
                        }
                        p = [p[0] / d, p[1] / d];
                    }
                    let Some(p) = upright.map(p).and_then(|p| lookup(p, channel)) else {
                        continue;
                    };
                    sx = ((p[0] + 1.) * iw as f64 / 2. - 0.5) as f32;
                    sy = ((p[1] + 1.) * ih as f64 / 2. - 0.5) as f32;
                }
                if !sx.is_finite()
                    || !sy.is_finite()
                    || sx < -0.5
                    || sy < -0.5
                    || sx >= iw - 0.5
                    || sy >= ih - 0.5
                {
                    continue;
                }
                let mut sum = 0.;
                let mut weights = 0.;
                for ky in sy.floor() as i64 - 2..=sy.floor() as i64 + 3 {
                    for kx in sx.floor() as i64 - 2..=sx.floor() as i64 + 3 {
                        let weight = lanczos3(sx - kx as f32) * lanczos3(sy - ky as f32);
                        let ix = kx.clamp(0, image.width() as i64 - 1) as usize;
                        let iy = ky.clamp(0, image.height() as i64 - 1) as usize;
                        sum += src[iy * image.width() as usize + ix] * weight;
                        weights += weight;
                    }
                }
                dst[y as usize * w as usize + x as usize] =
                    (sum / weights).clamp(-f32::MAX, f32::MAX);
            }
        }
    }
    crate::Image::new(w, h, planes)
}
fn lanczos3(x: f32) -> f32 {
    if x.abs() < 1e-12 {
        1.
    } else if x.abs() >= 3. {
        0.
    } else {
        let p = std::f32::consts::PI * x;
        p.sin() / p * (p / 3.).sin() / (p / 3.)
    }
}

/// Extent is the full level-0 coordinate domain, not the tile size.
/// Includes halos; clamped edge coordinates and stateless grain avoid tile seams.
pub fn effects(tile: &mut Tile, s: &EffectsSettings, extent: Extent) -> EngineResult<()> {
    effects_in_crop(tile, s, extent, &Default::default())
}

/// Evaluate effects in the final crop's frame before the Geometry resample.
pub fn effects_in_crop(
    tile: &mut Tile,
    s: &EffectsSettings,
    extent: Extent,
    crop: &engine_api::recipe::settings::Crop,
) -> EngineResult<()> {
    use engine_api::{recipe::settings::VignetteStyle, tile::TILE_SIZE};
    if !crop.rect.is_valid() || !crop.angle.is_finite() {
        return Err(EngineError::invalid(
            "crop",
            "invalid effects coordinate frame",
        ));
    }
    let v = &s.vignette;
    let g = &s.grain;
    if [
        v.amount,
        v.midpoint,
        v.roundness,
        v.feather,
        v.highlights,
        g.amount,
        g.size,
        g.roughness,
    ]
    .iter()
    .any(|x| !x.is_finite())
    {
        return Err(EngineError::invalid("effects", "parameters must be finite"));
    }
    if s.lens_blur.is_some() {
        return Err(EngineError::Unsupported {
            what: "M2 lens blur requires depth inference".into(),
        });
    }
    let l = tile.layout();
    let coord = tile.coord();
    let e = extent.at_level(coord.level);
    let ox = u64::from(coord.x) * u64::from(TILE_SIZE);
    let oy = u64::from(coord.y) * u64::from(TILE_SIZE);
    if extent.width == 0
        || extent.height == 0
        || l.channels != 3
        || ox + u64::from(l.extent.width) > u64::from(e.width)
        || oy + u64::from(l.extent.height) > u64::from(e.height)
    {
        return Err(EngineError::invalid(
            "effects tile",
            "RGB tile must fit nonempty full image extent at its level",
        ));
    }
    let input = tile.samples::<f32>()?;
    if input.iter().any(|x| !x.is_finite()) {
        return Err(EngineError::invalid(
            "effects tile",
            "finite samples required",
        ));
    }
    if v.amount == 0. && g.amount == 0. {
        return Ok(());
    }
    let amount = v.amount.clamp(-100., 100.) / 100.;
    let p = 2. + 3. * (1. - v.roundness.clamp(-100., 100.) / 100.);
    let midpoint = 0.05 + 0.9 * v.midpoint.clamp(0., 100.) / 100.;
    let feather = v.feather.clamp(0., 100.) / 100.;
    let rough = g.roughness.clamp(0., 100.) / 100.;
    let size = 0.5 + 7.5 * g.size.clamp(0., 100.) / 100.;
    let data = tile.samples_mut::<f32>()?;
    let n = l.plane_len();
    for row in 0..l.rows() {
        for col in 0..l.stride() {
            let gx = (ox as i64 + col as i64 - i64::from(l.halo)).clamp(0, i64::from(e.width) - 1)
                as f32;
            let gy = (oy as i64 + row as i64 - i64::from(l.halo)).clamp(0, i64::from(e.height) - 1)
                as f32;
            let r = crop.rect;
            let cw = (r.right - r.left) * e.width as f32;
            let ch = (r.bottom - r.top) * e.height as f32;
            let dx = gx + 0.5 - (r.left + r.right) * e.width as f32 / 2.;
            let dy = gy + 0.5 - (r.top + r.bottom) * e.height as f32 / 2.;
            let (sin, cos) = crop.angle.to_radians().sin_cos();
            let u = (cos * dx - sin * dy) / cw + 0.5;
            let vv = (sin * dx + cos * dy) / ch + 0.5;
            let radius = ((2. * u - 1.).abs().powf(p) + (2. * vv - 1.).abs().powf(p)).powf(1. / p);
            let mask = if feather == 0. {
                if radius >= midpoint { 1. } else { 0. }
            } else {
                smooth((radius - midpoint) / (feather * (1.5 - midpoint)))
            };
            let i = row * l.stride() + col;
            let mut rgb = [data[i], data[n + i], data[2 * n + i]];
            let y = 0.2627 * rgb[0] + 0.6780 * rgb[1] + 0.0593 * rgb[2];
            let protect = if amount < 0. {
                1. - v.highlights.clamp(0., 100.) / 100. * smooth(y)
            } else {
                1.
            };
            let a = amount * mask * protect;
            if a != 0. {
                rgb = match v.style {
                    VignetteStyle::HighlightPriority => rgb.map(|c| c * 2f32.powf(2. * a)),
                    VignetteStyle::ColorPriority => perceptual_vignette(rgb, a),
                    VignetteStyle::PaintOverlay => {
                        let target = if a > 0. { 1. } else { 0. };
                        rgb.map(|c| c * (1. - a.abs()) + target * a.abs())
                    }
                };
            }
            if g.amount > 0. {
                // Reference pixel coordinates, independent of tile/preview resolution.
                let px = u * (r.right - r.left) * extent.width as f32 / size;
                let py = vv * (r.bottom - r.top) * extent.height as f32 / size;
                let noise = (value_noise(px, py)
                    + rough * 0.5 * value_noise(2. * px + 19., 2. * py + 7.))
                    / (1. + rough * 0.5);
                let delta = noise * (0.025 + 0.075 * rough) * g.amount.clamp(0., 100.) / 100.;
                rgb = rgb.map(|c| c + delta);
            }
            for c in 0..3 {
                data[c * n + i] = rgb[c].clamp(-f32::MAX, f32::MAX);
            }
        }
    }
    Ok(())
}
fn smooth(x: f32) -> f32 {
    let t = x.clamp(0., 1.);
    t * t * (3. - 2. * t)
}
fn value_noise(x: f32, y: f32) -> f32 {
    fn hash(x: i64, y: i64) -> f32 {
        let mut z = (x as u64).wrapping_mul(0x9e3779b97f4a7c15)
            ^ (y as u64).wrapping_mul(0xbf58476d1ce4e5b9)
            ^ 0x5445535345524132;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^= z >> 31;
        (z >> 11) as f32 / (1u64 << 53) as f32 * 2. - 1.
    }
    let ix = x.floor() as i64;
    let iy = y.floor() as i64;
    let tx = smooth(x - x.floor());
    let ty = smooth(y - y.floor());
    let a = hash(ix, iy) * (1. - tx) + hash(ix + 1, iy) * tx;
    let b = hash(ix, iy + 1) * (1. - tx) + hash(ix + 1, iy + 1) * tx;
    a * (1. - ty) + b * ty
}
// Scale extreme vectors before dot products to avoid infinity cancellation.
fn finite_dot(v: [f32; 3], weights: [f32; 3]) -> f32 {
    let scale = if v.iter().any(|c| c.abs() > f32::MAX / 16.) {
        16.
    } else {
        1.
    };
    let v = v.map(|c| c / scale);
    ((v[0] * weights[0] + v[1] * weights[1] + v[2] * weights[2]) * scale).clamp(-f32::MAX, f32::MAX)
}

// CIE Lab D65: change L* only, retaining a*, b* (therefore chroma and hue).
fn perceptual_vignette(rgb: [f32; 3], a: f32) -> [f32; 3] {
    let xyz = [
        finite_dot(rgb, [0.636_958_06, 0.144_616_9, 0.168_880_97]),
        finite_dot(rgb, [0.262_700_2, 0.677_998_07, 0.059_301_715]),
        finite_dot(rgb, [0., 0.028_072_692, 1.060_985_1]),
    ];
    let white = [0.950_455_9, 1., 1.089_057_8];
    let f = |t: f32| {
        if t > 216. / 24389. {
            t.cbrt()
        } else {
            (t * (841. / 108.) + 4. / 29.).clamp(-f32::MAX, f32::MAX)
        }
    };
    let inv = |t: f32| {
        if t > 6. / 29. {
            (t * t * t).clamp(-f32::MAX, f32::MAX)
        } else {
            (t - 4. / 29.) * (108. / 841.)
        }
    };
    let fy = f(xyz[1]);
    // Algebraically (targetL - L) / 116, without overflowing L*.
    let delta = if a < 0. {
        a * (fy - 16. / 116.)
    } else {
        a * (1. - fy)
    };
    let xyz = std::array::from_fn(|i| {
        let t = (xyz[i] / white[i]).clamp(-f32::MAX, f32::MAX);
        let t = (f(t) + delta).clamp(-f32::MAX, f32::MAX);
        (inv(t) * white[i]).clamp(-f32::MAX, f32::MAX)
    });
    [
        finite_dot(xyz, [1.716_651_2, -0.355_670_78, -0.253_366_3]),
        finite_dot(xyz, [-0.666_684_3, 1.616_481_2, 0.015_768_547]),
        finite_dot(xyz, [0.017_639_857, -0.042_770_613, 0.942_103_15]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_api::{recipe::settings::NormalizedRect, tile::TileCoord};
    #[test]
    fn lanczos_uses_scalar_f32_rounding() {
        let x = 0.37f32;
        let p = std::f32::consts::PI * x;
        let expected = p.sin() / p * (p / 3.).sin() / (p / 3.);
        assert_eq!(lanczos3(0.37), expected);
    }
    #[test]
    fn geometry_and_lab_extreme_samples_stay_finite() {
        let image = crate::Image::new(9, 9, vec![vec![f32::MAX; 81]]).unwrap();
        let mut s = GeometrySettings::default();
        s.crop.angle = 13.;
        let out = geometry(&image, &s).unwrap();
        assert!(out.planes()[0].iter().all(|v| v.is_finite()));
        assert!(out.planes()[0][40] > f32::MAX * 0.99);
        for rgb in [
            [-f32::MAX; 3],
            [f32::MAX, -f32::MAX, f32::MAX],
            [f32::MAX; 3],
        ] {
            for amount in [-1., -0.5, 0.5, 1.] {
                assert!(
                    perceptual_vignette(rgb, amount)
                        .iter()
                        .all(|v| v.is_finite())
                );
            }
        }
    }
    #[test]
    fn geometry_identity_and_integer_crop() {
        let image = crate::Image::new(4, 2, vec![vec![-0.0, 1., 2., 3., 4., 5., 6., 7.]]).unwrap();
        let mut s = GeometrySettings::default();
        let out = geometry(&image, &s).unwrap();
        assert_eq!(
            out.planes()[0]
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>(),
            image.planes()[0]
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>()
        );
        s.crop.rect = NormalizedRect {
            left: 0.25,
            top: 0.,
            right: 0.75,
            bottom: 1.,
        };
        let out = geometry(&image, &s).unwrap();
        assert_eq!((out.width(), out.height()), (2, 2));
        for (v, expected) in out.planes()[0].iter().zip([1., 2., 5., 6.]) {
            assert!((v - expected).abs() < 1e-6);
        }
    }
    #[test]
    fn straighten_fractional_crop_and_validation() {
        let image =
            crate::Image::new(16, 16, vec![(0..256).map(|i| (i % 16) as f32).collect()]).unwrap();
        let mut s = GeometrySettings::default();
        s.crop.angle = 30.;
        let out = geometry(&image, &s).unwrap();
        assert_eq!((out.width(), out.height()), (16, 16));
        assert!((out.planes()[0][8 * 16 + 12] - image.planes()[0][8 * 16 + 12]).abs() > 0.1);
        s.crop.angle = 0.;
        s.crop.rect.left = 0.03125;
        s.crop.rect.right = 0.53125;
        let out = geometry(&image, &s).unwrap();
        assert!((out.planes()[0][4] - 4.5).abs() < 1e-5);
        s.crop.angle = f32::NAN;
        assert!(geometry(&image, &s).is_err());
        s.crop.angle = 46.;
        assert!(geometry(&image, &s).is_err());
        s = GeometrySettings::default();
        s.transform.vertical = 10.;
        assert!(geometry(&image, &s).is_ok());
        s.transform.scale = 0.;
        assert!(geometry(&image, &s).is_err());
    }
    #[test]
    fn vignette_styles_controls_and_grain_are_active() {
        use engine_api::recipe::settings::VignetteStyle;
        let image = crate::Image::new(
            64,
            64,
            vec![vec![0.8; 4096], vec![0.4; 4096], vec![0.2; 4096]],
        )
        .unwrap();
        let base = image.tile(TileCoord::new(0, 0, 0), 0, 1).unwrap();
        let mut s = EffectsSettings::default();
        s.vignette.amount = -70.;
        let run = |s: &EffectsSettings| {
            let mut t = base.clone();
            effects(&mut t, s, Extent::new(64, 64)).unwrap();
            t.samples::<f32>().unwrap().to_vec()
        };
        let original = base.samples::<f32>().unwrap();
        let dark = run(&s);
        assert!(dark[0] < original[0]);
        assert!((dark[32 * 64 + 32] - 0.8).abs() < 1e-6);
        for field in 0..4 {
            let mut changed = s.clone();
            match field {
                0 => changed.vignette.midpoint = 10.,
                1 => changed.vignette.roundness = -80.,
                2 => changed.vignette.feather = 0.,
                _ => changed.vignette.highlights = 100.,
            };
            assert_ne!(dark, run(&changed));
        }
        s.vignette.style = VignetteStyle::ColorPriority;
        let color = run(&s);
        assert_ne!(dark, color);
        s.vignette.style = VignetteStyle::PaintOverlay;
        assert_ne!(color, run(&s));
        assert_ne!(dark, run(&s));
        s = EffectsSettings::default();
        s.grain.amount = 70.;
        let grain = run(&s);
        assert_ne!(grain, original);
        assert_eq!(grain, run(&s));
        s.grain.size = 80.;
        assert_ne!(grain, run(&s));
        s.grain.size = 25.;
        s.grain.roughness = 0.;
        assert_ne!(grain, run(&s));
        assert!(grain.iter().all(|x| x.is_finite()));
    }
    #[test]
    fn effects_global_coordinates_match_halos_and_levels() {
        use engine_api::tile::TileLayout;
        let image = crate::Image::new(520, 17, vec![vec![0.5; 520 * 17]; 3]).unwrap();
        let mut s = EffectsSettings::default();
        s.vignette.amount = -55.;
        s.grain.amount = 60.;
        let mut left = image.tile(TileCoord::new(0, 0, 0), 2, 1).unwrap();
        let mut right = image.tile(TileCoord::new(0, 1, 0), 2, 1).unwrap();
        effects(&mut left, &s, Extent::new(520, 17)).unwrap();
        effects(&mut right, &s, Extent::new(520, 17)).unwrap();
        for y in 0..17 {
            for x in -2..2 {
                for c in 0..3 {
                    assert_eq!(
                        left.samples::<f32>().unwrap()[left.layout().index(c, 256 + x, y).unwrap()],
                        right.samples::<f32>().unwrap()[right.layout().index(c, x, y).unwrap()]
                    );
                }
            }
        }
        // Same normalized pixel center in odd-sized levels: center survives exactly.
        let layout = TileLayout {
            extent: Extent::new(1, 1),
            halo: 0,
            channels: 3,
        };
        let mut fine =
            Tile::from_samples(TileCoord::new(0, 0, 0), layout, vec![0.5f32; 3]).unwrap();
        let mut coarse =
            Tile::from_samples(TileCoord::new(4, 0, 0), layout, vec![0.5f32; 3]).unwrap();
        effects(&mut fine, &s, Extent::new(1, 1)).unwrap();
        effects(&mut coarse, &s, Extent::new(1, 1)).unwrap();
        assert_eq!(
            fine.samples::<f32>().unwrap(),
            coarse.samples::<f32>().unwrap()
        );
        // A level-1 tile uses the 260-wide domain, not the 520-wide domain.
        let layout = TileLayout {
            extent: Extent::new(4, 9),
            halo: 0,
            channels: 3,
        };
        let mut level =
            Tile::from_samples(TileCoord::new(1, 1, 0), layout, vec![0.5f32; 108]).unwrap();
        s.grain.amount = 0.;
        effects(&mut level, &s, Extent::new(520, 17)).unwrap();
        let reference = crate::Image::new(260, 9, vec![vec![0.5; 260 * 9]; 3]).unwrap();
        let mut reference = reference.tile(TileCoord::new(0, 1, 0), 0, 1).unwrap();
        effects(&mut reference, &s, Extent::new(260, 9)).unwrap();
        assert_eq!(
            level.samples::<f32>().unwrap(),
            reference.samples::<f32>().unwrap()
        );
    }
    #[test]
    fn effects_errors_are_atomic_and_extremes_finite() {
        use engine_api::recipe::settings::{LensBlur, VignetteStyle};
        let image = crate::Image::new(2, 2, vec![vec![-1., 0., 4., f32::MAX]; 3]).unwrap();
        let base = image.tile(TileCoord::new(0, 0, 0), 0, 1).unwrap();
        let mut tile = base.clone();
        let mut s = EffectsSettings::default();
        s.grain.amount = f32::NAN;
        assert!(effects(&mut tile, &s, Extent::new(2, 2)).is_err());
        assert!(tile.shares_buffer_with(&base));
        s = EffectsSettings::default();
        s.lens_blur = Some(LensBlur::default());
        assert!(effects(&mut tile, &s, Extent::new(2, 2)).is_err());
        s = EffectsSettings::default();
        assert!(effects(&mut tile, &s, Extent::new(0, 2)).is_err());
        for style in [
            VignetteStyle::HighlightPriority,
            VignetteStyle::ColorPriority,
            VignetteStyle::PaintOverlay,
        ] {
            for amount in [-100., 100.] {
                let mut tile = base.clone();
                s.vignette.style = style;
                s.vignette.amount = amount;
                s.vignette.midpoint = 0.;
                s.vignette.feather = 0.;
                s.grain.amount = 100.;
                effects(&mut tile, &s, Extent::new(2, 2)).unwrap();
                assert!(tile.samples::<f32>().unwrap().iter().all(|v| v.is_finite()));
            }
        }
    }
    #[test]
    fn rotation_constant_center_and_black_corners() {
        let image = crate::Image::new(9, 9, vec![vec![2.; 81]; 3]).unwrap();
        let mut s = GeometrySettings::default();
        s.crop.angle = 45.;
        let out = geometry(&image, &s).unwrap();
        assert_eq!(out.planes()[0][0], 0.);
        assert!((out.planes()[0][40] - 2.).abs() < 1e-6);
        s.crop.rect.right = s.crop.rect.left;
        assert!(geometry(&image, &s).is_err());
        let tiny = crate::Image::new(1, 1, vec![vec![2.]]).unwrap();
        s = GeometrySettings::default();
        s.crop.angle = -45.;
        assert_eq!(geometry(&tiny, &s).unwrap().planes()[0], vec![2.]);
    }
    #[test]
    fn effects_default_is_exact_and_shares_storage() {
        let image = crate::Image::new(2, 1, vec![vec![-0.0, 2.]; 3]).unwrap();
        let mut tile = image.tile(TileCoord::new(0, 0, 0), 1, 1).unwrap();
        let original = tile.clone();
        effects(&mut tile, &EffectsSettings::default(), Extent::new(2, 1)).unwrap();
        assert!(tile.shares_buffer_with(&original));
    }
}

#[cfg(test)]
mod composed_map_tests {
    use super::*;
    #[test]
    fn one_lookup_per_channel_combines_crop_transform_and_lens() {
        let image = crate::Image::new(
            64,
            48,
            vec![(0..64 * 48).map(|i| (i % 64) as f32 / 64.).collect(); 3],
        )
        .unwrap();
        let mut s = GeometrySettings::default();
        s.crop.rect.left = 0.25;
        s.crop.rect.right = 0.75;
        s.crop.angle = 2.;
        s.transform.rotate = 3.;
        s.transform.scale = 120.;
        let calls = std::cell::Cell::new(0usize);
        let out = geometry_mapped(&image, &s, true, |p, _| {
            calls.set(calls.get() + 1);
            Some(
                lens::BrownConrady {
                    k1: 0.1,
                    ..Default::default()
                }
                .distort(p),
            )
        })
        .unwrap();
        assert_eq!(
            calls.get(),
            out.width() as usize * out.height() as usize * 3
        );
        // Independent source location for a point well away from the support boundary.
        let x = 20.;
        let y = 20.;
        let (sn, cs) = 2_f64.to_radians().sin_cos();
        let dx = x + 0.5 - 16.;
        let dy = y + 0.5 - 24.;
        let px = 2. * (32. + cs * dx + sn * dy) / 64. - 1.;
        let py = 2. * (24. - sn * dx + cs * dy) / 48. - 1.;
        let (sn, cs) = 3_f64.to_radians().sin_cos();
        let q = [
            (cs * px + sn * py * 48. / 64.) / 1.2,
            (-sn * px * 64. / 48. + cs * py) / 1.2,
        ];
        let source_x = (q[0] * (1. + 0.1 * (q[0] * q[0] + q[1] * q[1])) + 1.) * 32. - 0.5;
        assert!((out.planes()[0][20 * 32 + 20] as f64 - source_x / 64.).abs() < 0.001);
    }
}
