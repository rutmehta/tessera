//! Export, export presets, installed printer profiles and print renders over
//! UniFFI (WP M2-20; docs/01 §2.22, §2.26).
//!
//! Export settings travel as JSON ([`ExportOptions`]): the same document is a
//! preset file under `<app-dir>/ExportPresets/`, what the Export sheet edits,
//! and what `Engine::export_batch` runs. A batch streams: one source is decoded,
//! rendered, written and dropped before the next, so memory stays bounded by
//! a single image however many are exported.
use crate::{Engine, ImageQuery, Result, catalog, failure, parse_id};
use engine_api::{jobs::CancellationToken, recipe::Recipe};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

// ───────────────────────────── settings (JSON) ─────────────────────────────

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileFormat {
    #[default]
    Jpeg,
    Png,
    Tiff,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentSpace {
    #[default]
    Srgb,
    DisplayP3,
    Rec2020,
    Prophoto,
}
impl From<DocumentSpace> for export::ColorSpace {
    fn from(s: DocumentSpace) -> Self {
        match s {
            DocumentSpace::Srgb => Self::Srgb,
            DocumentSpace::DisplayP3 => Self::DisplayP3,
            DocumentSpace::Rec2020 => Self::Rec2020,
            DocumentSpace::Prophoto => Self::ProPhoto,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResizeMode {
    #[default]
    None,
    /// Long edge = `long_edge` units.
    LongEdge,
    /// Fit within `width` × `height` units (never crops).
    Fit,
    /// `percent` of the rendered size (100 = original).
    Percent,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SizeUnit {
    #[default]
    Px,
    In,
    Cm,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ResizeOptions {
    pub mode: ResizeMode,
    pub unit: SizeUnit,
    pub long_edge: f64,
    pub width: f64,
    pub height: f64,
    pub percent: f64,
}
impl Default for ResizeOptions {
    fn default() -> Self {
        Self {
            mode: ResizeMode::None,
            unit: SizeUnit::Px,
            long_edge: 2048.0,
            width: 2048.0,
            height: 2048.0,
            percent: 100.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputSharpening {
    #[default]
    None,
    Screen,
    Matte,
    Glossy,
}
impl From<OutputSharpening> for export::SharpenFor {
    fn from(s: OutputSharpening) -> Self {
        match s {
            OutputSharpening::None => Self::None,
            OutputSharpening::Screen => Self::Screen,
            OutputSharpening::Matte => Self::Matte,
            OutputSharpening::Glossy => Self::Glossy,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetadataPolicy {
    #[default]
    All,
    Copyright,
    None,
}

/// What happens when an output file already exists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnConflict {
    /// Append `-2`, `-3`, … to the name (never overwrites).
    #[default]
    Unique,
    /// Report the image as failed and leave the existing file alone.
    Skip,
}

/// One export configuration: a preset's settings, and what the sheet edits.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ExportOptions {
    pub format: FileFormat,
    /// JPEG quality 1–100.
    pub quality: u8,
    /// TIFF 8 or 16 (PNG and JPEG are 8-bit).
    pub bit_depth: u8,
    pub color_space: DocumentSpace,
    pub resize: ResizeOptions,
    /// Recorded in the file; converts inch/cm sizes to pixels.
    pub dpi: u32,
    pub sharpening: OutputSharpening,
    pub metadata: MetadataPolicy,
    /// Tokens: `{name}` file name without extension, `{seq}` 1-based position,
    /// `{date}` capture date `YYYY-MM-DD`.
    pub naming: String,
    /// 1 (off), 2 or 4: Real-ESRGAN before resize (downloads the model once).
    pub upscale: u8,
    /// Absolute folder; empty in presets that leave it to the sheet.
    pub destination: String,
    pub on_conflict: OnConflict,
    /// App hint: reveal the files in Finder afterwards.
    pub open_in_finder: bool,
}
impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            format: FileFormat::Jpeg,
            quality: 90,
            bit_depth: 8,
            color_space: DocumentSpace::Srgb,
            resize: ResizeOptions::default(),
            dpi: 72,
            sharpening: OutputSharpening::None,
            metadata: MetadataPolicy::All,
            naming: "{name}".into(),
            upscale: 1,
            destination: String::new(),
            on_conflict: OnConflict::Unique,
            open_in_finder: false,
        }
    }
}

impl ExportOptions {
    pub fn from_json(json: &str) -> Result<Self> {
        let options: Self =
            serde_json::from_str(json).map_err(|e| failure(format!("export settings: {e}")))?;
        options.validate()?;
        Ok(options)
    }
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("export options serialize")
    }
    fn extension(&self) -> &'static str {
        match self.format {
            FileFormat::Jpeg => "jpg",
            FileFormat::Png => "png",
            FileFormat::Tiff => "tif",
        }
    }
    /// Everything but the destination (checked when a batch runs).
    pub fn validate(&self) -> Result<()> {
        if !(1..=100).contains(&self.quality) {
            return Err(failure("JPEG quality must be 1–100"));
        }
        if !matches!(self.bit_depth, 8 | 16) {
            return Err(failure("bit depth must be 8 or 16"));
        }
        if self.bit_depth == 16 && self.format != FileFormat::Tiff {
            return Err(failure("16-bit output needs TIFF"));
        }
        if !(1..=9600).contains(&self.dpi) {
            return Err(failure("resolution must be 1–9600 dpi"));
        }
        if !matches!(self.upscale, 1 | 2 | 4) {
            return Err(failure("upscale must be 1, 2 or 4"));
        }
        self.pixel_resize()?;
        export::filename(&self.naming, "IMG_0001", 1, "2026-01-01", self.extension())
            .map_err(|e| failure(format!("file name template: {e}")))?;
        Ok(())
    }
    fn pixels(&self, value: f64) -> f64 {
        match self.resize.unit {
            SizeUnit::Px => value,
            SizeUnit::In => value * f64::from(self.dpi),
            SizeUnit::Cm => value / 2.54 * f64::from(self.dpi),
        }
    }
    fn pixel_resize(&self) -> Result<export::Resize> {
        let edge = |v: f64, what: &str| {
            let px = self.pixels(v).round();
            if px.is_finite() && (1.0..=65_535.0).contains(&px) {
                Ok(px as u32)
            } else {
                Err(failure(format!("{what} must be 1–65535 pixels")))
            }
        };
        Ok(match self.resize.mode {
            ResizeMode::None => export::Resize::None,
            ResizeMode::LongEdge => {
                export::Resize::LongEdge(edge(self.resize.long_edge, "long edge")?)
            }
            ResizeMode::Fit => export::Resize::Fit(
                edge(self.resize.width, "width")?,
                edge(self.resize.height, "height")?,
            ),
            ResizeMode::Percent => {
                let p = self.resize.percent;
                if !(p.is_finite() && (1.0..=400.0).contains(&p)) {
                    return Err(failure("percent must be 1–400"));
                }
                export::Resize::Percent(p)
            }
        })
    }
    fn settings(&self, output_dir: PathBuf) -> Result<export::ExportSettings> {
        Ok(export::ExportSettings {
            format: match self.format {
                FileFormat::Jpeg => export::Format::Jpeg {
                    quality: self.quality,
                },
                FileFormat::Png => export::Format::Png,
                FileFormat::Tiff => export::Format::Tiff {
                    bits: self.bit_depth,
                },
            },
            color_space: self.color_space.into(),
            metadata: match self.metadata {
                MetadataPolicy::All => export::Metadata::All,
                MetadataPolicy::Copyright => export::Metadata::CopyrightOnly,
                MetadataPolicy::None => export::Metadata::None,
            },
            resize: self.pixel_resize()?,
            sharpen_for: self.sharpening.into(),
            naming: self.naming.clone(),
            output_dir,
            dpi: Some(self.dpi),
            apply_orientation: true,
            render_scale: 1,
        })
    }
}

/// Validates export settings JSON and returns it normalized (defaults filled).
#[uniffi::export]
pub fn normalize_export_settings(json: String) -> Result<String> {
    Ok(ExportOptions::from_json(&json)?.to_json())
}

/// The file name a template gives (with extension), or the reason it is not
/// a safe name. The Export sheet's live example and the engine agree on this.
#[uniffi::export]
pub fn export_filename(
    template: String,
    name: String,
    sequence: u32,
    date: String,
    extension: String,
) -> Result<String> {
    Ok(export::filename(
        &template,
        &name,
        sequence as usize,
        &date,
        &extension,
    )?)
}

// ───────────────────────────────── presets ─────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ExportPreset {
    pub name: String,
    /// `ExportOptions` JSON.
    pub settings_json: String,
}

#[derive(Serialize, Deserialize)]
struct PresetFile {
    name: String,
    settings: ExportOptions,
}

/// Shipped presets, in picker order.
pub fn default_export_presets() -> Vec<(String, ExportOptions)> {
    let base = ExportOptions::default();
    vec![
        (
            "Web 2048 sRGB".into(),
            ExportOptions {
                quality: 85,
                resize: ResizeOptions {
                    mode: ResizeMode::LongEdge,
                    long_edge: 2048.0,
                    ..Default::default()
                },
                sharpening: OutputSharpening::Screen,
                naming: "{name}".into(),
                ..base.clone()
            },
        ),
        (
            "Full-size JPEG".into(),
            ExportOptions {
                quality: 92,
                dpi: 300,
                ..base.clone()
            },
        ),
        (
            "16-bit TIFF ProPhoto".into(),
            ExportOptions {
                format: FileFormat::Tiff,
                bit_depth: 16,
                color_space: DocumentSpace::Prophoto,
                dpi: 300,
                ..base.clone()
            },
        ),
        (
            "Print 300 dpi".into(),
            ExportOptions {
                quality: 95,
                dpi: 300,
                resize: ResizeOptions {
                    mode: ResizeMode::LongEdge,
                    unit: SizeUnit::In,
                    long_edge: 12.0,
                    ..Default::default()
                },
                sharpening: OutputSharpening::Glossy,
                ..base
            },
        ),
    ]
}

const PRESET_DIR: &str = "ExportPresets";
const SEEDED: &str = ".defaults-installed";

impl Engine {
    pub(crate) fn support_dir(&self) -> Result<&Path> {
        self.db
            .parent()
            .ok_or_else(|| failure("app support directory"))
    }
    fn preset_dir(&self) -> Result<PathBuf> {
        let dir = self.support_dir()?.join(PRESET_DIR);
        if !dir.join(SEEDED).exists() {
            std::fs::create_dir_all(&dir)?;
            for (name, settings) in default_export_presets() {
                if find_preset(&dir, &name)?.is_none() {
                    write_preset(&dir, &name, &settings)?;
                }
            }
            std::fs::write(dir.join(SEEDED), b"")?;
        }
        Ok(dir)
    }
}

fn preset_files(dir: &Path) -> Result<Vec<(PathBuf, PresetFile)>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        // A damaged or foreign file is skipped rather than hiding every preset.
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Ok(file) = serde_json::from_str::<PresetFile>(&text) {
            out.push((path, file));
        }
    }
    Ok(out)
}

fn find_preset(dir: &Path, name: &str) -> Result<Option<PathBuf>> {
    Ok(preset_files(dir)?
        .into_iter()
        .find(|(_, f)| f.name == name)
        .map(|(p, _)| p))
}

fn write_preset(dir: &Path, name: &str, settings: &ExportOptions) -> Result<()> {
    let path = match find_preset(dir, name)? {
        Some(path) => path,
        None => {
            let stem: String = name
                .chars()
                .map(|c| {
                    if c.is_alphanumeric() || " -_.".contains(c) {
                        c
                    } else {
                        '_'
                    }
                })
                .collect::<String>()
                .trim_start_matches('.')
                .chars()
                .take(64)
                .collect();
            let stem = if stem.trim().is_empty() {
                "preset".to_owned()
            } else {
                stem
            };
            let mut path = dir.join(format!("{stem}.json"));
            let mut n = 2;
            while path.exists() {
                path = dir.join(format!("{stem} {n}.json"));
                n += 1;
            }
            path
        }
    };
    let text = serde_json::to_vec_pretty(&PresetFile {
        name: name.into(),
        settings: settings.clone(),
    })
    .map_err(failure)?;
    let mut temp = tempfile::NamedTempFile::new_in(dir)?;
    std::io::Write::write_all(&mut temp, &text)?;
    temp.persist(&path).map_err(|e| failure(e.error))?;
    Ok(())
}

fn preset_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        return Err(failure("preset names are 1–80 characters"));
    }
    Ok(name.into())
}

#[uniffi::export]
impl Engine {
    /// Export presets: the shipped ones (installed into the app directory on
    /// first use, then editable like any other) in their order, then the
    /// user's alphabetically.
    pub fn export_presets(&self) -> Result<Vec<ExportPreset>> {
        let dir = self.preset_dir()?;
        let defaults: Vec<String> = default_export_presets()
            .into_iter()
            .map(|(n, _)| n)
            .collect();
        let mut files = preset_files(&dir)?;
        files.sort_by(|(_, a), (_, b)| {
            let rank = |n: &str| defaults.iter().position(|d| d == n).unwrap_or(usize::MAX);
            rank(&a.name)
                .cmp(&rank(&b.name))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(files
            .into_iter()
            .map(|(_, f)| ExportPreset {
                name: f.name,
                settings_json: f.settings.to_json(),
            })
            .collect())
    }
    /// Creates or replaces the preset called `name`.
    pub fn save_export_preset(&self, name: String, settings_json: String) -> Result<()> {
        let name = preset_name(&name)?;
        let settings = ExportOptions::from_json(&settings_json)?;
        write_preset(&self.preset_dir()?, &name, &settings)
    }
    pub fn delete_export_preset(&self, name: String) -> Result<()> {
        match find_preset(&self.preset_dir()?, &name)? {
            Some(path) => Ok(std::fs::remove_file(path)?),
            None => Err(failure(format!("no export preset named “{name}”"))),
        }
    }
    pub fn rename_export_preset(&self, name: String, new_name: String) -> Result<()> {
        let new_name = preset_name(&new_name)?;
        let dir = self.preset_dir()?;
        let path = find_preset(&dir, &name)?
            .ok_or_else(|| failure(format!("no export preset named “{name}”")))?;
        if new_name != name && find_preset(&dir, &new_name)?.is_some() {
            return Err(failure(format!("a preset named “{new_name}” exists")));
        }
        let file: PresetFile = serde_json::from_slice(&std::fs::read(&path)?).map_err(failure)?;
        std::fs::remove_file(&path)?;
        write_preset(&dir, &new_name, &file.settings)
    }
    /// Re-installs the shipped presets (replacing edits to them; others stay).
    pub fn restore_default_export_presets(&self) -> Result<()> {
        let dir = self.preset_dir()?;
        for (name, settings) in default_export_presets() {
            write_preset(&dir, &name, &settings)?;
        }
        Ok(())
    }
}

// ────────────────────────────────── batch ──────────────────────────────────

#[derive(Clone, Debug, uniffi::Enum)]
pub enum ExportTarget {
    Images {
        image_ids: Vec<String>,
    },
    /// A manual album of `library.json`, in album order.
    Album {
        library_path: String,
        album_id: i64,
    },
    Query {
        query: ImageQuery,
    },
}

/// Cancels a running `export_batch` (or print render) from another thread.
#[derive(Debug, Default, uniffi::Object)]
pub struct CancelFlag(CancellationToken);
#[uniffi::export]
impl CancelFlag {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }
    pub fn cancel(&self) {
        self.0.cancel();
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.is_cancelled()
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ExportProgress {
    /// Images finished (written or failed) so far.
    pub done: u32,
    pub total: u32,
    pub exported: u32,
    pub failed: u32,
    /// Source file name being rendered next (empty when finished).
    pub current: String,
}

/// Called on the exporting thread before each image and after the last.
#[uniffi::export(with_foreign)]
pub trait ExportProgressListener: Send + Sync {
    fn on_progress(&self, progress: ExportProgress);
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ExportItemResult {
    pub image_id: String,
    /// Source file name.
    pub name: String,
    pub output_path: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ExportReport {
    pub destination: String,
    /// Input order. Images not reached after a cancel have neither path nor error.
    pub items: Vec<ExportItemResult>,
    pub exported: u32,
    pub failed: u32,
    pub cancelled: bool,
    pub seconds: f64,
}

struct Pending {
    id: String,
    path: PathBuf,
    date: String,
    orientation: u16,
}

impl Engine {
    fn resolve_target(&self, target: ExportTarget) -> Result<Vec<String>> {
        let ids = match target {
            ExportTarget::Images { image_ids } => image_ids,
            ExportTarget::Album {
                library_path,
                album_id,
            } => {
                let library = library::Library::read(&library_path)?;
                let (_, album) = library
                    .album_by_id(album_id)
                    .ok_or_else(|| failure(format!("album not found: {album_id}")))?;
                album.images.iter().map(ToString::to_string).collect()
            }
            ExportTarget::Query { query } => {
                self.list_images(query)?.into_iter().map(|s| s.id).collect()
            }
        };
        // Keep the first occurrence: sequence numbers follow the given order.
        let mut seen = HashSet::new();
        Ok(ids
            .into_iter()
            .filter(|id| seen.insert(id.clone()))
            .collect())
    }

    fn pending(&self, ids: &[String]) -> Result<Vec<Pending>> {
        let c = self.lock()?;
        ids.iter()
            .map(|id| {
                parse_id(id)?;
                let (path, date, orientation) = c.reader.query_row(
                    "SELECT f.path, COALESCE(strftime('%Y-%m-%d', i.capture_time, 'auto'), ''), \
                     COALESCE((SELECT value FROM metadata WHERE image_id=i.id AND key='orientation'),'1') \
                     FROM image i JOIN file f ON f.id=i.file_id WHERE i.id=?",
                    [id],
                    |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)),
                )?;
                Ok(Pending {
                    id: id.clone(),
                    path: path.into(),
                    date,
                    orientation: orientation.parse().unwrap_or(1),
                })
            })
            .collect()
    }

    fn recipe_and_xmp(&self, item: &Pending) -> Result<(Recipe, Option<sidecar::XmpPacket>)> {
        // The recipe document is authoritative; read it under the catalog
        // lock so a concurrent develop save cannot tear it.
        let _c = self.lock()?;
        let doc = catalog::document(&item.path, parse_id(&item.id)?)?;
        let xmp = catalog::xmp_path(&item.path);
        let packet = if xmp.exists() {
            Some(sidecar::Sidecar::read_xmp(xmp)?)
        } else {
            None
        };
        Ok((doc.recipe, packet))
    }

    fn load_upscaler(&self, factor: usize) -> Result<ml_enhance::SuperResolution> {
        let dir = self.support_dir()?.join("models");
        std::fs::create_dir_all(&dir)?;
        let manifest = dir.join("models.toml");
        let text = include_str!("../../ml-runtime/models.toml");
        if std::fs::read_to_string(&manifest).ok().as_deref() != Some(text) {
            std::fs::write(&manifest, text)?;
        }
        let cache = std::env::var_os("TESSERA_ENHANCE_MODEL_CACHE")
            .map(PathBuf::from)
            .unwrap_or_else(|| dir.join("cache"));
        let registry = ml_runtime::ModelRegistry::open(&manifest, &cache).map_err(failure)?;
        ml_enhance::SuperResolution::load(&registry, factor, ml_runtime::SessionOptions::default())
            .map_err(|e| failure(format!("upscale model: {e}")))
    }
}

/// Decoded pixels for the renderer (RAW CFA or linear Rec.2020 from RGB files).
enum Source {
    Rgb(pipeline_cpu::Image),
    Raw(Box<(raw_decode::CfaImage, raw_decode::RawMetadata)>),
}
impl Source {
    fn open(path: &Path, _orientation: u16) -> Result<Self> {
        if !image_core::RgbSource::recognizes(path) {
            let mut raw = raw_decode::RawSource::open(path)?;
            let cfa = raw.decode_cfa()?;
            return Ok(Self::Raw(Box::new((cfa, raw.metadata()))));
        }
        Ok(Self::Rgb(image_core::RgbSource::open(path)?.into_pixels()))
    }
    fn render_source(&self) -> pipeline_cpu::RenderSource<'_> {
        match self {
            Self::Rgb(image) => pipeline_cpu::RenderSource::Rgb(image),
            Self::Raw(raw) => pipeline_cpu::RenderSource::Cfa {
                image: &raw.0,
                metadata: &raw.1,
            },
        }
    }
    /// Displayed size before crop (after orientation).
    fn display_size(&self) -> (u32, u32) {
        match self {
            Self::Rgb(image) => (image.width(), image.height()),
            Self::Raw(raw) => {
                let [_, _, w, h] = raw.1.default_crop;
                if raw.1.orientation >= 5 {
                    (h, w)
                } else {
                    (w, h)
                }
            }
        }
    }
}

fn stem(path: &Path) -> Result<String> {
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(str::to_owned)
        .ok_or_else(|| failure("file names must be UTF-8"))
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[uniffi::export]
impl Engine {
    /// Exports `target` with `settings_json` (`ExportOptions`) into its
    /// destination. Blocking: call off the main thread. Per-image failures
    /// (unreadable source, existing file with `on_conflict: skip`, …) are
    /// reported per item; settings, destination and naming problems fail the
    /// whole call before anything is written. After a cancel the report lists
    /// the images already written; nothing half-written is left behind.
    pub fn export_batch(
        &self,
        target: ExportTarget,
        settings_json: String,
        listener: Option<Arc<dyn ExportProgressListener>>,
        cancel: Option<Arc<CancelFlag>>,
    ) -> Result<ExportReport> {
        let started = Instant::now();
        let options = ExportOptions::from_json(&settings_json)?;
        let destination = PathBuf::from(options.destination.trim());
        if !destination.is_absolute() {
            return Err(failure("choose an export folder (an absolute path)"));
        }
        let ids = self.resolve_target(target)?;
        if ids.is_empty() {
            return Err(failure("nothing to export"));
        }
        let pending = self.pending(&ids)?;
        let settings = options.settings(destination.clone())?;
        std::fs::create_dir_all(&destination)
            .map_err(|e| failure(format!("{}: {e}", destination.display())))?;
        let cancel = cancel.map(|c| c.0.clone()).unwrap_or_default();

        // Plan every output name first: a naming template that maps two
        // images to one file fails here, before anything is written.
        let mut taken = HashSet::new();
        let mut plans = Vec::with_capacity(pending.len());
        for (i, item) in pending.iter().enumerate() {
            let name = stem(&item.path)?;
            let sequence = i + 1;
            let mut naming = options.naming.clone();
            let mut n = 1;
            let plan = loop {
                let file =
                    export::filename(&naming, &name, sequence, &item.date, options.extension())?;
                let key = file.to_lowercase();
                let path = destination.join(&file);
                let exists = path.exists() || sidecar::Sidecar::paths(&path).xmp.exists();
                let clash = taken.contains(&key);
                if !exists && !clash {
                    taken.insert(key);
                    break Ok(naming);
                }
                match options.on_conflict {
                    OnConflict::Skip if clash => {
                        return Err(failure(format!(
                            "the name template gives “{file}” for more than one photo; add {{seq}}"
                        )));
                    }
                    OnConflict::Skip => break Err(format!("{file} already exists")),
                    OnConflict::Unique => {
                        n += 1;
                        naming = format!("{}-{n}", options.naming);
                    }
                }
            };
            plans.push((name, plan));
        }

        let total = pending.len() as u32;
        let mut report = ExportReport {
            destination: destination.to_string_lossy().into_owned(),
            items: pending
                .iter()
                .map(|p| ExportItemResult {
                    image_id: p.id.clone(),
                    name: file_name(&p.path),
                    output_path: None,
                    error: None,
                })
                .collect(),
            exported: 0,
            failed: 0,
            cancelled: false,
            seconds: 0.0,
        };
        let notify = |report: &ExportReport, current: String| {
            if let Some(l) = &listener {
                l.on_progress(ExportProgress {
                    done: report.exported + report.failed,
                    total,
                    exported: report.exported,
                    failed: report.failed,
                    current,
                });
            }
        };
        let mut segmenter: Option<Box<dyn export::mask_ai::MaskSegmenter>> = None;
        let mut upscaler: Option<ml_enhance::SuperResolution> = None;
        // At most one owned output is encoding while the next source renders.
        // No queue of decoded RAWs, GPU transactions, or output frames grows
        // with the batch length. Always join before returning, including cancel.
        type Encoding = (
            usize,
            std::thread::JoinHandle<engine_api::EngineResult<PathBuf>>,
        );
        let mut encoding: Option<Encoding> = None;
        let complete =
            |report: &mut ExportReport, index: usize, result: Result<PathBuf>| match result {
                Ok(path) => {
                    report.exported += 1;
                    report.items[index].output_path = Some(path.to_string_lossy().into_owned());
                    if let Ok(c) = self.lock()
                        && let Ok(id) = parse_id(&pending[index].id)
                    {
                        let _ = c.index.record_export(id, &path.to_string_lossy(), false);
                    }
                }
                Err(_) if cancel.is_cancelled() => report.cancelled = true,
                Err(e) => {
                    report.failed += 1;
                    report.items[index].error = Some(e.to_string());
                }
            };
        let join = |handle: std::thread::JoinHandle<engine_api::EngineResult<PathBuf>>| -> Result<PathBuf> {
            handle.join().map_err(|_| failure("export encoder panicked"))?.map_err(Into::into)
        };
        for (i, (item, (name, plan))) in pending.iter().zip(plans).enumerate() {
            if cancel.is_cancelled() {
                report.cancelled = true;
                break;
            }
            notify(&report, file_name(&item.path));
            let result = plan.map_err(failure).and_then(|naming| {
                let (recipe, packet) = self.recipe_and_xmp(item)?;
                let source = Source::open(&item.path, item.orientation)?;
                let image = export::ExportImage {
                    source: source.render_source(),
                    name: &name,
                    sequence: i + 1,
                    date: &item.date,
                    metadata: packet.as_ref(),
                };
                if export::needs_segmenter(&recipe) && segmenter.is_none() {
                    segmenter = Some(
                        export::mask_ai::load_segmenter(self.support_dir()?)
                            .map_err(|e| failure(format!("AI masks: {e}")))?,
                    );
                }
                if options.upscale > 1 && upscaler.is_none() {
                    upscaler = Some(self.load_upscaler(usize::from(options.upscale))?);
                }
                let crop = recipe.settings.geometry.crop.rect;
                let settings = export::ExportSettings {
                    naming,
                    // Always develop at full resolution and resize afterwards: rendering at a
                    // reduced pyramid level fails the exactness gate (tone/detail differ when
                    // applied before the downsample; see M2-21c RESULTS). Opt back in with
                    // TESSERA_EXPORT_WEB_LEVEL=1 for speed at the cost of exactness.
                    render_scale: if options.upscale > 1
                        || std::env::var_os("TESSERA_EXPORT_WEB_LEVEL").is_none()
                    {
                        1
                    } else {
                        export_scale(
                            source.display_size(),
                            [crop.left, crop.top, crop.right, crop.bottom],
                            settings.resize,
                        )
                    },
                    ..settings.clone()
                };
                let rendered = export::render_one_cancellable(
                    &image,
                    &recipe,
                    &settings,
                    &cancel,
                    upscaler.as_mut(),
                    match segmenter.as_mut() {
                        Some(s) => Some(s.as_mut()),
                        None => None,
                    },
                )?;
                Ok(rendered)
            });
            if let Some((index, handle)) = encoding.take() {
                complete(&mut report, index, join(handle));
            }
            match result {
                Ok(rendered) => {
                    let token = cancel.clone();
                    encoding = Some((i, std::thread::spawn(move || rendered.finish(&token))));
                }
                Err(_) if cancel.is_cancelled() => {
                    report.cancelled = true;
                    break;
                }
                Err(e) => {
                    report.failed += 1;
                    report.items[i].error = Some(e.to_string());
                }
            }
        }
        if let Some((index, handle)) = encoding.take() {
            complete(&mut report, index, join(handle));
        }
        report.seconds = started.elapsed().as_secs_f64();
        notify(&report, String::new());
        Ok(report)
    }
}

// ─────────────────────────────── printing ───────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct PrinterProfile {
    pub name: String,
    pub path: String,
    /// "RGB", "CMYK" or "Gray".
    pub color_space: String,
}

/// Installed ICC output (printer/paper) profiles from the ColorSync folders.
#[uniffi::export]
pub fn printer_profiles() -> Vec<PrinterProfile> {
    color_mgmt::installed_output_profiles()
        .into_iter()
        .map(|p| PrinterProfile {
            name: p.name,
            path: p.path.to_string_lossy().into_owned(),
            color_space: p.color_space.into(),
        })
        .collect()
}

/// Describes one profile file (for "Other…"); fails unless it is an output profile.
#[uniffi::export]
pub fn describe_printer_profile(path: String) -> Result<PrinterProfile> {
    let bytes = std::fs::read(&path)?;
    let p = color_mgmt::describe_output(Path::new(&path), &bytes)
        .ok_or_else(|| failure("not an RGB, CMYK or gray output (printer) profile"))?;
    Ok(PrinterProfile {
        name: p.name,
        path,
        color_space: p.color_space.into(),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum RenderingIntent {
    Perceptual,
    RelativeColorimetric,
    Saturation,
    AbsoluteColorimetric,
}
impl From<RenderingIntent> for color_mgmt::Intent {
    fn from(i: RenderingIntent) -> Self {
        match i {
            RenderingIntent::Perceptual => Self::Perceptual,
            RenderingIntent::RelativeColorimetric => Self::RelativeColorimetric,
            RenderingIntent::Saturation => Self::Saturation,
            RenderingIntent::AbsoluteColorimetric => Self::AbsoluteColorimetric,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PrintSharpening {
    None,
    Matte,
    Glossy,
}

/// Application-managed colour: convert into the printer profile's device space.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct PrintProfile {
    pub path: String,
    pub intent: RenderingIntent,
    pub black_point_compensation: bool,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PrintRenderRequest {
    pub image_id: String,
    /// The picture is scaled to fit this box (pixels at the print resolution),
    /// keeping its aspect.
    pub max_width: u32,
    pub max_height: u32,
    pub sharpening: PrintSharpening,
    /// None: printer-managed colour (Display P3 pixels, matched by the driver).
    pub profile: Option<PrintProfile>,
}

/// Interleaved 8-bit device pixels plus the ICC profile that describes them.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PrintImage {
    pub width: u32,
    pub height: u32,
    /// 3 (RGB), 4 (CMYK) or 1 (gray).
    pub channels: u32,
    pub data: Vec<u8>,
    pub icc: Vec<u8>,
}

/// Largest power-of-two binning (≤ 8) that still leaves ≥ the requested box.
pub fn print_scale(display: (u32, u32), crop: [f32; 4], box_px: (u32, u32)) -> u32 {
    let [l, t, r, b] = crop;
    let fw = f64::from((r - l).clamp(0.01, 1.0));
    let fh = f64::from((b - t).clamp(0.01, 1.0));
    let (w, h) = (f64::from(display.0) * fw, f64::from(display.1) * fh);
    // Aspect fit: the limiting side decides how many pixels are needed.
    binning_for((f64::from(box_px.0) / w).min(f64::from(box_px.1) / h))
}

/// Binning for an export resize: the rendered size stays ≥ the output size
/// (Lanczos then only downsamples), so small outputs render 4–64× fewer pixels.
pub fn export_scale(display: (u32, u32), crop: [f32; 4], resize: export::Resize) -> u32 {
    match resize {
        export::Resize::None => 1,
        export::Resize::LongEdge(n) => print_scale(display, crop, (n, n)),
        export::Resize::Fit(w, h) => print_scale(display, crop, (w, h)),
        export::Resize::Percent(p) => binning_for(p / 100.0),
    }
}

fn binning_for(fit: f64) -> u32 {
    if !fit.is_finite() || fit <= 0.0 {
        return 1;
    }
    let mut scale = 1;
    while scale < 8 && fit * f64::from(scale * 2) <= 1.0 {
        scale *= 2;
    }
    scale
}

#[uniffi::export]
impl Engine {
    /// Renders one photo for printing: develop settings, orientation, fit to
    /// the box, print sharpening, then colour for the chosen handling.
    /// Blocking; `cancel` stops between stages.
    pub fn render_for_print(
        &self,
        request: PrintRenderRequest,
        cancel: Option<Arc<CancelFlag>>,
    ) -> Result<PrintImage> {
        if request.max_width == 0 || request.max_height == 0 {
            return Err(failure("print box must be at least 1 × 1 pixel"));
        }
        if u64::from(request.max_width) * u64::from(request.max_height) > 200_000_000 {
            return Err(failure("print box exceeds 200 megapixels"));
        }
        let cancel = cancel.map(|c| c.0.clone()).unwrap_or_default();
        let item = self
            .pending(std::slice::from_ref(&request.image_id))?
            .pop()
            .ok_or_else(|| failure("image not found"))?;
        let (recipe, _) = self.recipe_and_xmp(&item)?;
        let source = Source::open(&item.path, item.orientation)?;
        let crop = recipe.settings.geometry.crop.rect;
        let segmenter = if export::needs_segmenter(&recipe) {
            Some(
                export::mask_ai::load_segmenter(self.support_dir()?)
                    .map_err(|e| failure(format!("AI masks: {e}")))?,
            )
        } else {
            None
        };
        let scale = print_scale(
            source.display_size(),
            [crop.left, crop.top, crop.right, crop.bottom],
            (request.max_width, request.max_height),
        );
        let space = if request.profile.is_some() {
            export::ColorSpace::ProPhoto
        } else {
            export::ColorSpace::DisplayP3
        };
        let mut segmenter = segmenter;
        let rgb = export::render_pixels(
            &export::ExportImage {
                source: source.render_source(),
                name: "print",
                sequence: 1,
                date: "",
                metadata: None,
            },
            &recipe,
            &export::RenderRequest {
                color_space: space,
                // Fit the box exactly (enlarging when the file is smaller):
                // the page layout decides the physical size, not the file.
                resize: export::Resize::Fit(request.max_width, request.max_height),
                sharpen_for: match request.sharpening {
                    PrintSharpening::None => export::SharpenFor::None,
                    PrintSharpening::Matte => export::SharpenFor::Matte,
                    PrintSharpening::Glossy => export::SharpenFor::Glossy,
                },
                scale,
            },
            &cancel,
            match segmenter.as_mut() {
                Some(s) => Some(s.as_mut()),
                None => None,
            },
        )?;
        let (width, height) = rgb.dimensions();
        let pixels: Vec<[f32; 3]> = rgb
            .as_raw()
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| p.map(|v| v.clamp(0.0, 1.0)))
            .collect();
        cancel.check()?;
        match request.profile {
            Some(profile) => {
                let mut registry = color_mgmt::Registry::new();
                let source = registry
                    .builtin(color_mgmt::Builtin::ProPhoto)
                    .map_err(failure)?;
                let printer = registry
                    .load_file(&profile.path)
                    .map_err(|e| failure(format!("{}: {e}", profile.path)))?;
                let device = color_mgmt::convert_rgb(
                    &source,
                    &printer,
                    profile.intent.into(),
                    profile.black_point_compensation,
                    &pixels,
                )
                .map_err(failure)?;
                Ok(PrintImage {
                    width,
                    height,
                    channels: device.channels as u32,
                    data: device.data,
                    icc: printer.icc_bytes().to_vec(),
                })
            }
            None => Ok(PrintImage {
                width,
                height,
                channels: 3,
                data: pixels
                    .iter()
                    .flat_map(|p| p.map(|v| (v * 255.0).round() as u8))
                    .collect(),
                icc: export::color_space_icc(export::ColorSpace::DisplayP3)?,
            }),
        }
    }
}
