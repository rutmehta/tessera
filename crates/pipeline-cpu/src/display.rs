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
/// gamut compression -> sRGB OETF -> deterministic ordered 8-bit dither.
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
