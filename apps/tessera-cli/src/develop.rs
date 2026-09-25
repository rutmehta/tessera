use anyhow::{Result, ensure};
use clap::Args;
use engine_api::recipe::{DevelopSettings, EditMeta, settings::WhiteBalanceMode};
use sidecar::Sidecar;
use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Args)]
#[group(required = true, multiple = true)]
pub struct Basic {
    #[arg(long, allow_hyphen_values = true)]
    exposure: Option<f32>,
    #[arg(long, allow_hyphen_values = true)]
    contrast: Option<f32>,
    #[arg(long, allow_hyphen_values = true)]
    highlights: Option<f32>,
    #[arg(long, allow_hyphen_values = true)]
    shadows: Option<f32>,
    #[arg(long, allow_hyphen_values = true)]
    whites: Option<f32>,
    #[arg(long, allow_hyphen_values = true)]
    blacks: Option<f32>,
    #[arg(long, allow_hyphen_values = true)]
    texture: Option<f32>,
    #[arg(long, allow_hyphen_values = true)]
    clarity: Option<f32>,
    #[arg(long, allow_hyphen_values = true)]
    dehaze: Option<f32>,
    #[arg(long, allow_hyphen_values = true)]
    temperature: Option<f32>,
    #[arg(long, allow_hyphen_values = true)]
    tint: Option<f32>,
    #[arg(long, allow_hyphen_values = true)]
    vibrance: Option<f32>,
    #[arg(long, allow_hyphen_values = true)]
    saturation: Option<f32>,
}
impl Basic {
    fn apply(&self, settings: &mut DevelopSettings) {
        macro_rules! set { ($target:expr, $($field:ident),*) => { $(if let Some(value) = self.$field { $target.$field = value; })* }; }
        set!(
            settings.tone,
            exposure,
            contrast,
            highlights,
            shadows,
            whites,
            blacks,
            texture,
            clarity,
            dehaze
        );
        set!(settings.color, vibrance, saturation);
        set!(settings.white_balance, temperature, tint);
        if self.temperature.is_some() || self.tint.is_some() {
            settings.white_balance.mode = WhiteBalanceMode::Custom;
        }
    }
}
pub fn set(path: &Path, basic: &Basic) -> Result<DevelopSettings> {
    for (name, value, min, max) in [
        ("exposure", basic.exposure, -10., 10.),
        ("contrast", basic.contrast, -100., 100.),
        ("highlights", basic.highlights, -100., 100.),
        ("shadows", basic.shadows, -100., 100.),
        ("whites", basic.whites, -100., 100.),
        ("blacks", basic.blacks, -100., 100.),
        ("texture", basic.texture, -100., 100.),
        ("clarity", basic.clarity, -100., 100.),
        ("dehaze", basic.dehaze, -100., 100.),
        ("vibrance", basic.vibrance, -100., 100.),
        ("saturation", basic.saturation, -100., 100.),
        ("temperature", basic.temperature, 2000., 50000.),
        ("tint", basic.tint, -150., 150.),
    ] {
        if let Some(value) = value {
            ensure!(
                value.is_finite() && (min..=max).contains(&value),
                "{name} must be finite and within {min}..={max}"
            );
        }
    }
    ensure!(path.is_file(), "image does not exist: {}", path.display());
    // The sidecar format uses stems, so reject ambiguous RAW/JPEG siblings.
    for entry in std::fs::read_dir(path.parent().unwrap_or(Path::new(".")))? {
        let peer = entry?.path();
        if peer != path && peer.is_file() && peer.file_stem() == path.file_stem() {
            let ext = peer
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            ensure!(
                !matches!(
                    ext.as_str(),
                    "jpg"
                        | "jpeg"
                        | "cr3"
                        | "cr2"
                        | "nef"
                        | "arw"
                        | "raf"
                        | "dng"
                        | "png"
                        | "tif"
                        | "tiff"
                ),
                "sidecar destination collision: {}",
                peer.display()
            );
        }
    }
    let mut doc = crate::catalog::document(path)?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_millis()
        .min(i64::MAX as u128) as i64;
    doc.recipe
        .edit(EditMeta::user("CLI Basic edit", timestamp), |settings| {
            basic.apply(settings)
        })?;
    doc.recipe.validate()?;
    doc.record_write(&format!("cli-{}", std::process::id()), timestamp)?;
    Sidecar::write_recipe(Sidecar::paths(path).recipe, &doc)?;
    Ok(doc.recipe.settings)
}
