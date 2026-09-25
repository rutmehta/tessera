use anyhow::{Context, Result, bail, ensure};
use clap::Args;
use engine_api::{jobs::CancellationToken, recipe::Recipe};
use export::{
    ColorSpace, ExportImage, ExportItem, ExportSettings, Format, Metadata, Resize, SharpenFor,
};
use index::{Index, Query};
use pipeline_cpu::{Image, RenderSource};
use serde_json::{Value, json};
use sidecar::{Sidecar, XmpPacket};
use std::path::{Path, PathBuf};

#[derive(Args)]
pub struct Options {
    #[arg(conflicts_with = "query", required_unless_present = "query")]
    input: Option<PathBuf>,
    #[arg(long, required_unless_present = "input")]
    query: Option<String>,
    #[arg(long)]
    out: PathBuf,
    #[arg(long, value_parser = ["jpeg", "png", "tiff"])]
    format: String,
    #[arg(long, default_value_t = 90, value_parser = clap::value_parser!(u8).range(1..=100))]
    quality: u8,
    #[arg(long, conflicts_with = "fit", value_parser = clap::value_parser!(u32).range(1..))]
    long_edge: Option<u32>,
    #[arg(long)]
    fit: Option<String>,
    #[arg(long, default_value = "srgb", value_parser = ["srgb", "p3", "rec2020", "prophoto"])]
    color_space: String,
    #[arg(long, value_parser = ["screen", "matte", "glossy"])]
    sharpen: Option<String>,
    #[arg(long, default_value = "all", value_parser = ["all", "copyright", "none"])]
    metadata: String,
    #[arg(long, default_value = "{name}-{seq}")]
    name: String,
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    jobs: Option<u32>,
    /// Apply x2/x4 super-resolution before resize/sharpen (serial; ignores --jobs).
    #[arg(long, value_parser = ["2", "4"])]
    upscale: Option<String>,
}

fn paths(index: &Index, options: &Options) -> Result<Vec<PathBuf>> {
    if let Some(query) = &options.query {
        let mut paths = index
            .search(&Query {
                text: Some(query.clone()),
                ..super::all()
            })?
            .into_iter()
            .map(|id| index.image_info(id).map(|info| info.path))
            .collect::<engine_api::EngineResult<Vec<_>>>()?;
        paths.sort();
        return Ok(paths);
    }
    let input = options
        .input
        .as_ref()
        .context("image, directory or --query required")?;
    if input.is_file() {
        return Ok(vec![input.canonicalize()?]);
    }
    ensure!(
        input.is_dir(),
        "not an image or directory: {}",
        input.display()
    );
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(input)? {
        let path = entry?.path();
        if path.is_file() && is_image(&path) {
            paths.push(path.canonicalize()?);
        }
    }
    paths.sort();
    Ok(paths)
}

fn is_image(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "jpg" | "jpeg" | "png" | "tif" | "tiff" | "cr3" | "cr2" | "nef" | "arw" | "raf" | "dng"
    )
}

fn settings(options: &Options) -> Result<ExportSettings> {
    let resize = if let Some(edge) = options.long_edge {
        Resize::LongEdge(edge)
    } else if let Some(fit) = &options.fit {
        let (w, h) = fit.split_once(['x', 'X']).context("--fit must be WxH")?;
        let (w, h): (u32, u32) = (w.parse()?, h.parse()?);
        ensure!(w > 0 && h > 0, "--fit dimensions must be positive");
        Resize::Fit(w, h)
    } else {
        Resize::None
    };
    Ok(ExportSettings {
        format: match options.format.as_str() {
            "jpeg" => Format::Jpeg {
                quality: options.quality,
            },
            "png" => Format::Png,
            _ => Format::Tiff { bits: 8 },
        },
        color_space: match options.color_space.as_str() {
            "p3" => ColorSpace::DisplayP3,
            "rec2020" => ColorSpace::Rec2020,
            "prophoto" => ColorSpace::ProPhoto,
            _ => ColorSpace::Srgb,
        },
        metadata: match options.metadata.as_str() {
            "none" => Metadata::None,
            "copyright" => Metadata::CopyrightOnly,
            _ => Metadata::All,
        },
        resize,
        sharpen_for: match options.sharpen.as_deref() {
            Some("screen") => SharpenFor::Screen,
            Some("matte") => SharpenFor::Matte,
            Some("glossy") => SharpenFor::Glossy,
            _ => SharpenFor::None,
        },
        naming: options.name.clone(),
        output_dir: options.out.clone(),
        ..ExportSettings::default()
    })
}

// Export's RGB source contract is linear Rec.2020; decode ordinary images into it.
fn decode_rgb(path: &Path) -> Result<Image> {
    let rgb = image::open(path)
        .with_context(|| format!("decode {}", path.display()))?
        .into_rgb8();
    let (w, h) = rgb.dimensions();
    let mut planes = vec![vec![0.0; (w as usize) * (h as usize)]; 3];
    for (i, pixel) in rgb.pixels().enumerate() {
        let linear = pixel.0.map(|sample| {
            let v = f32::from(sample) / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        });
        // Linear sRGB (D65) to linear Rec.2020 (D65).
        for (c, row) in [
            [0.6274, 0.3293, 0.0433],
            [0.0691, 0.9195, 0.0114],
            [0.0164, 0.0880, 0.8956],
        ]
        .iter()
        .enumerate()
        {
            planes[c][i] = row.iter().zip(linear).map(|(a, b)| a * b).sum();
        }
    }
    Ok(Image::new(w, h, planes)?)
}

fn packet(path: &Path) -> Result<Option<XmpPacket>> {
    let appended = Sidecar::paths(path).xmp;
    let legacy = path.with_extension("xmp");
    let file = if appended.is_file() { appended } else { legacy };
    if file.is_file() {
        Ok(Some(Sidecar::read_xmp(file)?))
    } else {
        Ok(None)
    }
}

// Check the entire SR selection before loading weights or publishing any wave.
// Only naming metadata is read here; decoded pixels remain one-image-at-a-time.
fn preflight_upscale(
    paths: &[PathBuf],
    settings: &ExportSettings,
    cancel: &CancellationToken,
) -> Result<()> {
    let mut names = std::collections::HashSet::new();
    let extension = match settings.format {
        Format::Jpeg { .. } => "jpg",
        Format::Png => "png",
        Format::Tiff { .. } => "tif",
    };
    for (index, path) in paths.iter().enumerate() {
        cancel.check()?;
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .context("image filename is not UTF-8")?;
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let date = if settings.naming.contains("{date}")
            && !matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "tif" | "tiff")
        {
            let source = raw_decode::RawSource::open(path)?;
            chrono::DateTime::from_timestamp(source.metadata().capture_time, 0)
                .map(|time| time.format("%Y%m%d").to_string())
                .unwrap_or_default()
        } else {
            String::new()
        };
        let name = export::filename(&settings.naming, name, index + 1, &date, extension)?;
        ensure!(names.insert(name.to_lowercase()), "duplicate output names");
    }
    Ok(())
}

pub fn run(index: &Index, app_dir: &Path, options: &Options) -> Result<Value> {
    let settings = settings(options)?;
    let paths = paths(index, options)?;
    ensure!(!paths.is_empty(), "no images matched export input");
    let cancel = CancellationToken::new();
    let signal = cancel.clone();
    ctrlc::set_handler(move || signal.cancel()).context("install Ctrl-C handler")?;
    let mut upscale = if let Some(factor) = &options.upscale {
        preflight_upscale(&paths, &settings, &cancel)?;
        let manifest = app_dir.join("models.toml");
        let registry = ml_runtime::ModelRegistry::open(&manifest, app_dir.join("models"))
            .with_context(|| format!("open model registry {}", manifest.display()))?;
        cancel.check()?;
        let model = ml_enhance::SuperResolution::load(
            &registry,
            factor.parse()?,
            // Dynamic SR shapes can trigger CoreML native diagnostics on stdout,
            // corrupting the CLI JSON protocol even when CPU fallback succeeds.
            ml_runtime::SessionOptions::cpu(),
        )
        .with_context(|| format!("load x{factor} super-resolution model"))?;
        cancel.check()?;
        Some(model)
    } else {
        None
    };
    let mut completed = 0;
    let mut outputs = Vec::new();
    let mut errors = Vec::new();
    // Decode only one wave at a time; the export crate bounds render/encode workers.
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    let jobs = if upscale.is_some() {
        1
    } else {
        options.jobs.map_or(cores, |n| (n as usize).min(cores))
    };
    for (wave, chunk) in paths.chunks(jobs).enumerate() {
        if cancel.is_cancelled() {
            break;
        }
        let mut loaded = Vec::new();
        for (offset, path) in chunk.iter().enumerate() {
            if cancel.is_cancelled() {
                break;
            }
            match load(path) {
                Ok(image) => loaded.push((wave * jobs + offset + 1, image)),
                Err(error) => {
                    errors.push(json!({"input":path,"error":format!("{error:#}")}));
                    eprintln!("failed to decode {}: {error:#}", path.display());
                }
            }
        }
        let items: Vec<_> = loaded
            .iter()
            .map(|(seq, image)| ExportItem {
                image: ExportImage {
                    source: match &image.pixels {
                        Pixels::Rgb(rgb) => RenderSource::Rgb(rgb),
                        Pixels::Cfa(cfa, metadata) => RenderSource::Cfa {
                            image: cfa,
                            metadata,
                        },
                    },
                    name: &image.name,
                    sequence: *seq,
                    date: &image.date,
                    metadata: image.metadata.as_ref(),
                },
                recipe: &image.recipe,
            })
            .collect();
        let progress = |progress: export::Progress| {
            let path = &chunk[loaded[progress.index].0 - wave * jobs - 1];
            eprintln!(
                "{}/{} exported: {}",
                completed + progress.completed,
                paths.len(),
                path.display()
            );
        };
        let report = if let Some(model) = upscale.as_mut() {
            export::export_batch_upscaled(&items, &settings, progress, &cancel, model)?
        } else {
            export::export_batch_with_jobs(&items, &settings, progress, &cancel, jobs)?
        };
        for (result, (seq, _)) in report.results.into_iter().zip(&loaded) {
            match result {
                Ok(output) => {
                    completed += 1;
                    outputs.push(output);
                }
                Err(engine_api::EngineError::Cancelled) if cancel.is_cancelled() => {}
                Err(error) => {
                    errors.push(json!({"input":paths[seq - 1],"error":error.to_string()}))
                }
            }
        }
    }
    if cancel.is_cancelled() {
        bail!("export cancelled after {completed}/{} images", paths.len());
    }
    if !errors.is_empty() {
        bail!("export failed: {}", errors[0]["error"]);
    }
    Ok(json!({"total":paths.len(),"completed":completed,"outputs":outputs,"errors":errors}))
}

enum Pixels {
    Rgb(Image),
    Cfa(raw_decode::CfaImage, Box<raw_decode::RawMetadata>),
}

struct Loaded {
    name: String,
    date: String,
    recipe: Recipe,
    metadata: Option<XmpPacket>,
    pixels: Pixels,
}

fn load(path: &Path) -> Result<Loaded> {
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .context("image filename is not UTF-8")?
        .to_owned();
    let doc = crate::catalog::document(path)?;
    let metadata = packet(path)?;
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let (pixels, date) = if matches!(extension.as_str(), "jpg" | "jpeg" | "png" | "tif" | "tiff") {
        (Pixels::Rgb(decode_rgb(path)?), String::new())
    } else {
        let mut source = raw_decode::RawSource::open(path)?;
        let cfa = source.decode_cfa()?;
        let raw_metadata = source.metadata();
        let date = chrono::DateTime::from_timestamp(raw_metadata.capture_time, 0)
            .map(|time| time.format("%Y%m%d").to_string())
            .unwrap_or_default();
        (Pixels::Cfa(cfa, Box::new(raw_metadata)), date)
    };
    Ok(Loaded {
        name,
        date,
        recipe: doc.recipe,
        metadata,
        pixels,
    })
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    #[test]
    fn upscale_parser_accepts_only_two_or_four() {
        for (factor, valid) in [
            ("2", true),
            ("4", true),
            ("0", false),
            ("1", false),
            ("3", false),
            ("8", false),
        ] {
            assert_eq!(
                crate::Cli::try_parse_from([
                    "tessera",
                    "export",
                    "input.png",
                    "--out",
                    "out",
                    "--format",
                    "png",
                    "--upscale",
                    factor,
                ])
                .is_ok(),
                valid,
                "factor {factor}"
            );
        }
    }
}
