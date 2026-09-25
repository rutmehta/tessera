//! Scalar f32 reference operators. See OPERATORS.md for formulas and scope.
mod color;
mod locals;
pub mod masks;
pub use locals::{adjust_local, blend_local, locals_image};
mod color_detail;
mod geometry_effects;
mod tone_extra;
pub use color_detail::{DETAIL_HALO, color, detail, detail_halo};
pub use geometry_effects::{effects, effects_in_crop, geometry};
pub use tone_extra::{tone_extra, tone_extra_image};
mod display;
mod embedded_lens;
mod image;
mod lens_resolve;
mod optics;
mod render;
mod upright;
pub use display::{SigmoidSettings, display, display_float, sigmoid, srgb_oetf};
pub use image::Image;
pub use lens_resolve::{CorrectionSource, LensContext, ResolvedLens, resolve_lens};
pub use render::{
    RenderSource, Rgb8Image, has_m2_settings, render, render_linear_scaled,
    render_linear_scaled_with_lens, render_scaled, validate_settings,
};
mod mosaic;
pub use color::{
    apply_matrix, as_shot_temperature_tint, camera_to_xyz, temperature_white, white_balance_matrix,
};
use engine_api::{EngineError, EngineResult, recipe::settings::ToneSettings, tile::Tile};
pub use mosaic::{DemosaicAlgorithm, demosaic, inverse_linearize, reconstruct_highlights};

/// Apply a point operator to planar RGB, including any halo samples.
pub fn map_rgb(tile: &mut Tile, mut op: impl FnMut([f32; 3]) -> [f32; 3]) -> EngineResult<()> {
    if tile.layout().channels != 3 {
        return Err(EngineError::invalid("tile", "expected three RGB planes"));
    }
    let n = tile.layout().plane_len();
    let data = tile.samples_mut::<f32>()?;
    for i in 0..n {
        let rgb = op([data[i], data[n + i], data[2 * n + i]]);
        for c in 0..3 {
            data[c * n + i] = rgb[c];
        }
    }
    Ok(())
}

/// Scene-linear exposure and monotone, luminance-only tonal adjustments.
pub fn tone(tile: &mut Tile, settings: &ToneSettings) -> EngineResult<()> {
    let values = [
        settings.exposure,
        settings.contrast,
        settings.highlights,
        settings.shadows,
        settings.whites,
        settings.blacks,
    ];
    if values.iter().any(|v| !v.is_finite()) {
        return Err(EngineError::invalid("tone", "parameters must be finite"));
    }
    let gain = settings.exposure.clamp(-10.0, 10.0).exp2();
    let neutral = values[1..].iter().all(|&v| v == 0.0);
    map_rgb(tile, |rgb| {
        let rgb = rgb.map(|v| v * gain);
        if neutral {
            return rgb;
        }
        let y = luminance(rgb);
        if y <= 0.0 {
            return rgb;
        }
        let z = (y / 0.18).ln_1p();
        let pivot = 2.0f32.ln();
        let slope = (settings.contrast.clamp(-100.0, 100.0) / 100.0).exp2();
        // Bounded slope in log space avoids exp(log(Y)^2) overflow at +10 EV.
        let z = slope * z + (1.0 - slope) * 2.0 * pivot * -(-z).exp_m1();

        let softplus = |v: f32| v.max(0.0) + (-v.abs()).exp().ln_1p();
        let mut out = z;
        for (amount, center, upper) in [
            (settings.blacks, 0.25, false),
            (settings.shadows, 0.8, false),
            (settings.highlights, 1.5, true),
            (settings.whites, 2.5, true),
        ] {
            let upper_integral = softplus(z - center) - softplus(-center);
            let region = if upper {
                upper_integral
            } else {
                z - upper_integral
            };
            // Each derivative contributes at most 0.2 in magnitude.
            out += 0.2 * amount.clamp(-100.0, 100.0) / 100.0 * region;
        }

        let scale = 0.18 * out.exp_m1() / y;
        rgb.map(|v| v * scale)
    })
}

fn luminance(rgb: [f32; 3]) -> f32 {
    0.2627 * rgb[0] + 0.6780 * rgb[1] + 0.0593 * rgb[2]
}
