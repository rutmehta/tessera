//! Soft proofing for the develop viewport (docs/01 §2.26).
//!
//! The viewport contract is display-encoded sRGB (see `surface.rs`), so the
//! proof is a presentation step: a 33³ LUT from viewport sRGB to the
//! simulated print (in sRGB), with an out-of-gamut flag per node, which the
//! Metal presenter applies while sampling the surface. Toggling the proof
//! never re-renders, never changes the recipe (no history entry) and never
//! reaches an export. Limitation: colours outside sRGB are already clipped
//! before the proof, so the gamut warning is exact only for sRGB content.
use crate::{DevelopSession, RenderingIntent, Result, failure};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
};

#[derive(Clone, Debug, PartialEq, Eq, Hash, uniffi::Record)]
pub struct SoftProofOptions {
    /// ICC output profile to simulate (see `printer_profiles`).
    pub profile_path: String,
    pub intent: RenderingIntent,
    pub black_point_compensation: bool,
    /// Absolute colorimetric proof-to-display leg: show paper white and ink black.
    pub simulate_paper: bool,
}

/// `size`³ nodes, red fastest, then green, then blue. Each node is RGBA
/// 16-bit unorm: the proofed colour in display-encoded sRGB, and alpha 65535
/// where the printer cannot reproduce the colour (ΔE76 > 2 after a clipped
/// round trip), else 0.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SoftProofLut {
    pub size: u32,
    pub rgba: Vec<u16>,
    /// The simulated profile's description.
    pub profile_name: String,
    /// Share of LUT nodes out of the printer's gamut (0–1), for the panel.
    pub out_of_gamut: f32,
}

pub const PROOF_LUT_SIZE: u32 = 33;

type Cache = Mutex<HashMap<(Vec<u8>, SoftProofOptions), Arc<SoftProofLut>>>;
fn cache() -> &'static Cache {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// Builds (or reuses) the viewport proof LUT for `options`.
pub fn soft_proof_lut(options: &SoftProofOptions) -> Result<Arc<SoftProofLut>> {
    let bytes = std::fs::read(&options.profile_path)
        .map_err(|e| failure(format!("{}: {e}", options.profile_path)))?;
    let key = (bytes, options.clone());
    if let Some(lut) = cache().lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return Ok(lut.clone());
    }
    let name = color_mgmt::describe_output(std::path::Path::new(&options.profile_path), &key.0)
        .map(|p| p.name)
        .ok_or_else(|| failure("not an RGB, CMYK or gray output (printer) profile"))?;
    let mut registry = color_mgmt::Registry::new();
    let srgb = registry
        .builtin(color_mgmt::Builtin::Srgb)
        .map_err(failure)?;
    let printer = registry.load_bytes(&key.0).map_err(failure)?;
    let transform = color_mgmt::Transform::proof(
        &srgb,
        &srgb,
        &printer,
        color_mgmt::TransformOptions {
            intent: options.intent.into(),
            black_point_compensation: options.black_point_compensation,
            simulate_paper: options.simulate_paper,
            ..Default::default()
        },
    )
    .map_err(failure)?;
    let size = PROOF_LUT_SIZE as usize;
    let step = 1.0 / (size - 1) as f32;
    let mut rgba = Vec::with_capacity(size * size * size * 4);
    let mut out = 0usize;
    let unorm = |v: f32| (v.clamp(0.0, 1.0) * 65535.0).round() as u16;
    for b in 0..size {
        for g in 0..size {
            for r in 0..size {
                let rgb = [r as f32 * step, g as f32 * step, b as f32 * step];
                let proofed = transform.apply(rgb);
                let warn = transform.gamut_warning(rgb).proof;
                out += usize::from(warn);
                rgba.extend(proofed.map(unorm));
                rgba.push(if warn { u16::MAX } else { 0 });
            }
        }
    }
    let lut = Arc::new(SoftProofLut {
        size: PROOF_LUT_SIZE,
        rgba,
        profile_name: name,
        out_of_gamut: out as f32 / (size * size * size) as f32,
    });
    let mut cache = cache().lock().unwrap_or_else(|e| e.into_inner());
    if cache.len() > 16 {
        cache.clear();
    }
    cache.insert(key, lut.clone());
    Ok(lut)
}

#[uniffi::export]
impl DevelopSession {
    /// Soft proofing toggle for this session's viewport: `Some` returns the
    /// presentation LUT for the simulated print, `None` turns proofing off
    /// (returns `None`). The render, recipe and history are untouched.
    /// Blocking on first use of a profile (tens of milliseconds).
    pub fn soft_proof(&self, options: Option<SoftProofOptions>) -> Result<Option<SoftProofLut>> {
        options
            .map(|o| soft_proof_lut(&o).map(|lut| (*lut).clone()))
            .transpose()
    }
}
