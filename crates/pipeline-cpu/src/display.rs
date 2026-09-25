use engine_api::{
    EngineResult,
    color::{ChromaticAdaptation, WorkingSpace},
    recipe::settings::GamutMapping,
    tile::{TILE_SIZE, Tile, TileLayout},
};

/// Standalone display-kernel parameters (not additional recipe state).
/// The M1 renderer fixes these defaults; ToneSettings.contrast is scene tone.
#[derive(Debug, Clone, Copy)]
pub struct SigmoidSettings {
    pub contrast: f32,
    pub skew: f32,
}
impl Default for SigmoidSettings {
    fn default() -> Self {
        Self {
            contrast: 1.5,
            skew: 0.0,
        }
    }
}

/// Generalized log-logistic sigmoid, anchored at black and 18% grey.
/// Positive contrast is clamped to 0.25..4 and skew to -1..1.
pub fn sigmoid(value: f32, settings: SigmoidSettings) -> f32 {
    Sigmoid::new(settings).eval(value)
}

/// [`sigmoid`] with its per-settings constants evaluated once. Bit-identical
/// to calling [`sigmoid`] per value (same f32 operations in the same order).
#[derive(Debug, Clone, Copy)]
struct Sigmoid {
    p: f32,
    q: f32,
    ln_a: f32,
}
impl Sigmoid {
    fn new(settings: SigmoidSettings) -> Self {
        let p = settings.contrast.clamp(0.25, 4.0);
        let q = settings.skew.clamp(-1.0, 1.0).exp2();
        let a = 0.18 * (0.18f32.powf(-1.0 / q) - 1.0).powf(1.0 / p);
        Self { p, q, ln_a: a.ln() }
    }
    #[inline]
    fn eval(self, value: f32) -> f32 {
        if value <= 0.0 {
            return 0.0;
        }
        let s = 1.0 / (1.0 + (self.p * (self.ln_a - value.ln())).exp());
        // powf(x, 1) == x exactly; skip the call for the default skew.
        if self.q == 1.0 { s } else { s.powf(self.q) }
    }
}
pub fn srgb_oetf(v: f32) -> f32 {
    if v <= 0.0031308 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// Scene-linear Rec.2020 -> sigmoid luminance -> display-linear sRGB ->
/// gamut compression -> sRGB OETF, without quantization or dither.
/// For restoration models; the returned f32 tile excludes input halos.
pub fn display_float(
    tile: &Tile,
    settings: SigmoidSettings,
    gamut: GamutMapping,
) -> EngineResult<Tile> {
    let mut rgb = tile.clone();
    let curve = Sigmoid::new(settings);
    crate::map_rgb(&mut rgb, |v| {
        let y = crate::luminance(v);
        if y <= 0.0 {
            [0.0; 3]
        } else {
            let s = curve.eval(y);
            v.map(|c| c * s / y)
        }
    })?;
    crate::apply_matrix(
        &mut rgb,
        WorkingSpace::LinearRec2020
            .conversion_to(WorkingSpace::LinearSrgb, ChromaticAdaptation::Cat16)?,
    )?;
    let l = tile.layout();
    let n = l.extent.area() as usize;
    let mut data = vec![0.0f32; n * 3];
    let samples = rgb.samples::<f32>()?;
    let plane = l.plane_len();
    let row = l.extent.width + 2 * u32::from(l.halo);
    for y in 0..l.extent.height {
        for x in 0..l.extent.width {
            // Interior sample (x, y) of each plane, skipping the halo.
            let i = ((y + u32::from(l.halo)) * row + x + u32::from(l.halo)) as usize;
            let v = [samples[i], samples[plane + i], samples[2 * plane + i]];
            let grey = (0.2126 * v[0] + 0.7152 * v[1] + 0.0722 * v[2]).clamp(0.0, 1.0);
            let mut chroma = 1.0f32;
            if gamut == GamutMapping::Perceptual {
                for c in v {
                    let d = c - grey;
                    if c < 0.0 {
                        chroma = chroma.min(-grey / d);
                    }
                    if c > 1.0 {
                        chroma = chroma.min((1.0 - grey) / d);
                    }
                }
            }

            for c in 0..3 {
                let linear = if gamut == GamutMapping::Clip {
                    v[c]
                } else {
                    grey + chroma * (v[c] - grey)
                };
                data[c * n + (y * l.extent.width + x) as usize] = srgb_oetf(linear.clamp(0.0, 1.0));
            }
        }
    }
    Tile::from_samples(
        tile.coord(),
        TileLayout {
            extent: l.extent,
            halo: 0,
            channels: 3,
        },
        data,
    )
}

/// Largest EDR headroom (linear multiple of SDR white) the HDR display
/// transform accepts: 8 stops. Current EDR displays reach 16× (4 stops) and
/// the value must stay well inside the half-float surface range.
pub const MAX_HDR_HEADROOM: f32 = 256.0;

/// Clamps an EDR headroom (linear multiple of SDR white) to
/// `1..=`[`MAX_HDR_HEADROOM`]; non-finite values mean SDR (1.0).
pub fn sanitize_headroom(headroom: f32) -> f32 {
    if headroom.is_finite() {
        headroom.clamp(1.0, MAX_HDR_HEADROOM)
    } else {
        1.0
    }
}

/// `ln(a)` of the default display sigmoid rescaled to peak at `headroom`:
/// `s(y) = H / (1 + (a/y)^p)` with `s(0.18) = 0.18`, i.e.
/// `a = 0.18 (H/0.18 − 1)^(1/p)`. At `H = 1` this is the SDR constant.
/// Shared with the GPU kernel so both evaluate the same curve.
pub fn hdr_sigmoid_ln_a(headroom: f32) -> f32 {
    let h = sanitize_headroom(headroom);
    let p = SigmoidSettings::default().contrast;
    (0.18 * (h / 0.18 - 1.0).powf(1.0 / p)).ln()
}

/// EDR display transform: scene-linear Rec.2020 → default sigmoid rescaled
/// so SDR mid-grey stays at 0.18 and highlights roll off to `headroom`
/// (a linear multiple of SDR white) → display-linear sRGB (extended: values
/// above 1.0 are EDR headroom) → hue-preserving chroma compression into
/// `[0, headroom]`. No OETF, quantization or dither: the viewport writes
/// these as half floats for an `extendedLinearSRGB` layer. At
/// `headroom = 1` this is the SDR tone curve without encoding; the SDR
/// [`display`] path is a separate, unchanged function.
pub fn display_linear(tile: &Tile, gamut: GamutMapping, headroom: f32) -> EngineResult<Tile> {
    let h = sanitize_headroom(headroom);
    let p = SigmoidSettings::default().contrast;
    let ln_a = hdr_sigmoid_ln_a(h);
    let mut rgb = tile.clone();
    crate::map_rgb(&mut rgb, |v| {
        let y = crate::luminance(v);
        if y <= 0.0 {
            [0.0; 3]
        } else {
            let s = h / (1.0 + (p * (ln_a - y.ln())).exp());
            v.map(|c| c * s / y)
        }
    })?;
    crate::apply_matrix(
        &mut rgb,
        WorkingSpace::LinearRec2020
            .conversion_to(WorkingSpace::LinearSrgb, ChromaticAdaptation::Cat16)?,
    )?;
    let l = tile.layout();
    let n = l.extent.area() as usize;
    let mut data = vec![0.0f32; n * 3];
    let samples = rgb.samples::<f32>()?;
    let plane = l.plane_len();
    let row = l.extent.width + 2 * u32::from(l.halo);
    for y in 0..l.extent.height {
        for x in 0..l.extent.width {
            let i = ((y + u32::from(l.halo)) * row + x + u32::from(l.halo)) as usize;
            let v = [samples[i], samples[plane + i], samples[2 * plane + i]];
            let grey = (0.2126 * v[0] + 0.7152 * v[1] + 0.0722 * v[2]).clamp(0.0, h);
            let mut chroma = 1.0f32;
            if gamut == GamutMapping::Perceptual {
                for c in v {
                    let d = c - grey;
                    if c < 0.0 {
                        chroma = chroma.min(-grey / d);
                    }
                    if c > h {
                        chroma = chroma.min((h - grey) / d);
                    }
                }
            }
            for c in 0..3 {
                let linear = if gamut == GamutMapping::Clip {
                    v[c]
                } else {
                    grey + chroma * (v[c] - grey)
                };
                data[c * n + (y * l.extent.width + x) as usize] = linear.clamp(0.0, h);
            }
        }
    }
    Tile::from_samples(
        tile.coord(),
        TileLayout {
            extent: l.extent,
            halo: 0,
            channels: 3,
        },
        data,
    )
}

/// Display conversion followed by deterministic ordered 8-bit dither.
pub fn display(tile: &Tile, settings: SigmoidSettings, gamut: GamutMapping) -> EngineResult<Tile> {
    let mut rgb = tile.clone();
    let curve = Sigmoid::new(settings);
    crate::map_rgb(&mut rgb, |v| {
        let y = crate::luminance(v);
        if y <= 0.0 {
            [0.0; 3]
        } else {
            let s = curve.eval(y);
            v.map(|c| c * s / y)
        }
    })?;
    crate::apply_matrix(
        &mut rgb,
        WorkingSpace::LinearRec2020
            .conversion_to(WorkingSpace::LinearSrgb, ChromaticAdaptation::Cat16)?,
    )?;
    let l = tile.layout();
    let n = l.extent.area() as usize;
    let mut data = vec![0u8; n * 3];
    let samples = rgb.samples::<f32>()?;
    let (ox, oy) = tile.coord().pixel_origin(TILE_SIZE);
    const BAYER: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
    let plane = l.plane_len();
    let row = l.extent.width + 2 * u32::from(l.halo);
    for y in 0..l.extent.height {
        for x in 0..l.extent.width {
            // Interior sample (x, y) of each plane, skipping the halo.
            let i = ((y + u32::from(l.halo)) * row + x + u32::from(l.halo)) as usize;
            let v = [samples[i], samples[plane + i], samples[2 * plane + i]];
            let grey = (0.2126 * v[0] + 0.7152 * v[1] + 0.0722 * v[2]).clamp(0.0, 1.0);
            let mut chroma = 1.0f32;
            if gamut == GamutMapping::Perceptual {
                for c in v {
                    let d = c - grey;
                    if c < 0.0 {
                        chroma = chroma.min(-grey / d);
                    }
                    if c > 1.0 {
                        chroma = chroma.min((1.0 - grey) / d);
                    }
                }
            }
            let noise = (f32::from(BAYER[((oy + y) % 4) as usize][((ox + x) % 4) as usize]) + 0.5)
                / 16.0
                - 0.5;
            for c in 0..3 {
                let linear = if gamut == GamutMapping::Clip {
                    v[c]
                } else {
                    grey + chroma * (v[c] - grey)
                };
                data[c * n + (y * l.extent.width + x) as usize] =
                    (srgb_oetf(linear.clamp(0.0, 1.0)) * 255.0 + noise)
                        .round()
                        .clamp(0.0, 255.0) as u8;
            }
        }
    }
    Tile::from_samples(
        tile.coord(),
        TileLayout {
            extent: l.extent,
            halo: 0,
            channels: 3,
        },
        data,
    )
}

#[cfg(test)]
mod hdr_tests {
    use super::*;
    use engine_api::tile::{Extent, TileCoord};

    fn tile(values: &[[f32; 3]]) -> Tile {
        let n = values.len();
        let mut data = vec![0.0; 3 * n];
        for (i, v) in values.iter().enumerate() {
            for c in 0..3 {
                data[c * n + i] = v[c];
            }
        }
        Tile::from_samples(
            TileCoord::new(0, 0, 0),
            TileLayout {
                extent: Extent::new(n as u32, 1),
                halo: 0,
                channels: 3,
            },
            data,
        )
        .unwrap()
    }

    fn decode(v: f32) -> f32 {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    }

    const SCENE: [[f32; 3]; 6] = [
        [0.0, 0.0, 0.0],
        [0.18, 0.18, 0.18],
        [0.05, 0.1, 0.02],
        [1.0, 0.9, 0.8],
        [8.0, 8.0, 8.0],
        [40.0, 10.0, 2.0],
    ];

    #[test]
    fn unit_headroom_is_the_sdr_curve_without_encoding() {
        assert_eq!(hdr_sigmoid_ln_a(1.0), {
            let p = SigmoidSettings::default().contrast;
            (0.18 * (0.18f32.powf(-1.0) - 1.0).powf(1.0 / p)).ln()
        });
        for gamut in [GamutMapping::Perceptual, GamutMapping::Clip] {
            let t = tile(&SCENE);
            let sdr = display_float(&t, SigmoidSettings::default(), gamut).unwrap();
            let lin = display_linear(&t, gamut, 1.0).unwrap();
            for (a, b) in sdr
                .samples::<f32>()
                .unwrap()
                .iter()
                .zip(lin.samples::<f32>().unwrap())
            {
                assert!((decode(*a) - b).abs() < 2e-5, "{a} {b}");
            }
        }
    }

    #[test]
    fn headroom_extends_highlights_and_keeps_mid_grey() {
        let t = tile(&SCENE);
        for h in [2.0f32, 4.0, 16.0] {
            let out = display_linear(&t, GamutMapping::Perceptual, h).unwrap();
            let s = out.samples::<f32>().unwrap();
            let n = SCENE.len();
            let px = |i: usize| [s[i], s[n + i], s[2 * n + i]];
            assert!(s.iter().all(|v| (0.0..=h).contains(v)));
            // Neutral mid-grey is anchored in every headroom.
            for c in px(1) {
                assert!((c - 0.18).abs() < 2e-3, "{c}");
            }
            // Bright neutral scene values use the headroom (> SDR white).
            assert!(px(4).iter().all(|&c| c > 1.0 && c < h), "{:?}", px(4));
        }
        // More headroom never darkens a highlight.
        let lo = display_linear(&t, GamutMapping::Perceptual, 2.0).unwrap();
        let hi = display_linear(&t, GamutMapping::Perceptual, 8.0).unwrap();
        let (lo, hi) = (lo.samples::<f32>().unwrap(), hi.samples::<f32>().unwrap());
        assert!(hi[4] > lo[4]);
        // Non-finite headroom falls back to SDR.
        assert_eq!(sanitize_headroom(f32::NAN), 1.0);
        assert_eq!(sanitize_headroom(0.5), 1.0);
        assert_eq!(sanitize_headroom(1e9), MAX_HDR_HEADROOM);
    }
}
