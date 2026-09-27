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
    /// Output format. Original copies bytes + recipe XMP; DNG develops linear Rec.2020.
    #[arg(long, value_parser = ["jpeg", "png", "tiff", "avif", "jxl", "dng", "original"])]
    format: String,
    /// AVIF 8/10/12, TIFF/JPEG XL 8/16, DNG 32-bit float per channel.
    #[arg(long, default_value_t = 8)]
    bit_depth: u8,
    /// AVIF encoding speed, 1 (slow) through 10 (fast).
    #[arg(long, default_value_t = 6, value_parser = clap::value_parser!(u8).range(1..=10))]
    avif_speed: u8,
    #[arg(long, default_value_t = 90, value_parser = clap::value_parser!(u8).range(1..=100))]
    quality: u8,
    /// Maximum JPEG bytes including ICC and XMP (fails if unattainable).
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    max_file_bytes: Option<u64>,
    /// Watermark JSON file (text/font or PNG graphic, anchor, inset, opacity).
    #[arg(long)]
    watermark: Option<PathBuf>,
    #[arg(long, conflicts_with = "fit", value_parser = clap::value_parser!(u32).range(1..))]
    long_edge: Option<u32>,
    #[arg(long)]
    fit: Option<String>,
    #[arg(long, default_value = "srgb", value_parser = ["srgb", "p3", "rec2020", "prophoto"])]
    color_space: String,
    #[arg(long, value_parser = ["screen", "matte", "glossy"])]
    sharpen: Option<String>,
    /// Output sharpening strength (after resize, before watermark).
    #[arg(long, default_value = "standard", value_parser = ["low", "standard", "high"])]
    sharpen_amount: String,
    /// Output pixel density; paper sharpening uses 300 ppi when omitted.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..=9600))]
    ppi: Option<u32>,
    #[arg(long, default_value = "all", value_parser = ["all", "copyright", "copyright-and-contact", "all-except-camera", "none"])]
    metadata: String,
    /// Remove XMP face regions and their associated person keywords.
    #[arg(long)]
    remove_person_info: bool,
    /// Remove XMP GPS and IPTC location properties.
    #[arg(long)]
    remove_location: bool,
    /// Retain Lightroom keyword paths, or omit the hierarchy with false.
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    keywords_as_hierarchy: bool,
    #[arg(long, default_value = "{name}-{seq}")]
    name: String,
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    jobs: Option<u32>,
    /// Apply x2/x4 super-resolution before resize/sharpen (serial; ignores --jobs).
    #[arg(long, value_parser = ["2", "4"])]
    upscale: Option<String>,
    /// Reveal successful exports in Finder (macOS).
    #[arg(long)]
    reveal: bool,
    /// Open successful exports in this absolute app path (macOS).
    #[arg(long)]
    open_in_app: Option<PathBuf>,
    /// Execute this absolute script path with output paths as separate arguments.
    #[arg(long)]
    after_export_script: Option<PathBuf>,
    /// Timeout for each post-export child process.
    #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u32).range(1..=3600))]
    after_export_timeout: u32,
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
    if options.format == "original" {
        ensure!(
            options.metadata == "all"
                && !options.remove_person_info
                && !options.remove_location
                && options.keywords_as_hierarchy
                && options.long_edge.is_none()
                && options.fit.is_none()
                && options.sharpen.is_none()
                && options.watermark.is_none()
                && options.upscale.is_none()
                && options.max_file_bytes.is_none(),
            "original + XMP retains original metadata and pixels; privacy filters and output transforms are not supported"
        );
    }
    ensure!(
        options.format != "jxl" || options.color_space == "srgb",
        "lossless JPEG XL currently supports only sRGB"
    );
    ensure!(
        match options.format.as_str() {
            "avif" => matches!(options.bit_depth, 8 | 10 | 12),
            "dng" => options.bit_depth == 32,
            "tiff" | "jxl" => matches!(options.bit_depth, 8 | 16),
            _ => options.bit_depth == 8,
        },
        "unsupported bit depth for format"
    );
    ensure!(
        options.max_file_bytes.is_none() || options.format == "jpeg",
        "--max-file-bytes requires JPEG"
    );
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
            // Original is handled before decoding; this placeholder is never rendered.
            "dng" | "original" => Format::Dng,
            "jxl" => Format::JpegXl {
                bits: options.bit_depth,
            },
            "avif" => Format::Avif(export::AvifOptions {
                quality: options.quality,
                bits: options.bit_depth,
                speed: options.avif_speed,
            }),
            _ => Format::Tiff {
                bits: options.bit_depth,
            },
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
            "copyright-and-contact" => Metadata::CopyrightAndContact,
            "all-except-camera" => Metadata::AllExceptCamera,
            _ => Metadata::All,
        },
        remove_person_info: options.remove_person_info,
        remove_location: options.remove_location,
        keywords_as_hierarchy: options.keywords_as_hierarchy,
        resize,
        sharpen_for: match options.sharpen.as_deref() {
            Some("screen") => SharpenFor::Screen,
            Some("matte") => SharpenFor::Matte,
            Some("glossy") => SharpenFor::Glossy,
            _ => SharpenFor::None,
        },
        naming: options.name.clone(),
        sharpen_amount: match options.sharpen_amount.as_str() {
            "low" => export::SharpenAmount::Low,
            "high" => export::SharpenAmount::High,
            _ => export::SharpenAmount::Standard,
        },
        dpi: options.ppi,
        output_dir: options.out.clone(),
        max_file_bytes: options.max_file_bytes,
        watermark: options
            .watermark
            .as_ref()
            .map(|path| -> Result<export::Watermark> {
                let mark: export::Watermark = serde_json::from_slice(&std::fs::read(path)?)?;
                mark.validate()?;
                Ok(mark)
            })
            .transpose()?,
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
    let extension = settings.format.extension();
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
    let actions = export::AfterExportActions {
        reveal: options.reveal,
        open_in_app: options.open_in_app.clone(),
        run_script: options.after_export_script.clone(),
        timeout_seconds: options.after_export_timeout,
    };
    actions.validate()?;
    let settings = settings(options)?;
    let paths = paths(index, options)?;
    ensure!(!paths.is_empty(), "no images matched export input");
    let cancel = CancellationToken::new();
    let signal = cancel.clone();
    ctrlc::set_handler(move || signal.cancel()).context("install Ctrl-C handler")?;
    if options.format == "original" {
        return originals(&paths, options, &actions, &cancel);
    }
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
    let absolute_outputs = outputs
        .iter()
        .map(std::path::absolute)
        .collect::<std::io::Result<Vec<_>>>()?;
    let workflow_errors = export::run_after_export(&actions, &absolute_outputs, &cancel);
    Ok(
        json!({"total":paths.len(),"completed":completed,"outputs":outputs,"errors":errors,"workflow_errors":workflow_errors,"cancelled":cancel.is_cancelled()}),
    )
}

fn originals(
    paths: &[PathBuf],
    options: &Options,
    actions: &export::AfterExportActions,
    cancel: &CancellationToken,
) -> Result<Value> {
    let mut names = std::collections::HashSet::new();
    let mut plans = Vec::new();
    for (i, source) in paths.iter().enumerate() {
        cancel.check()?;
        let name = source
            .file_stem()
            .and_then(|s| s.to_str())
            .context("original filename must be UTF-8")?;
        let extension = source
            .extension()
            .and_then(|s| s.to_str())
            .context("original source requires an extension")?;
        let date = if options.name.contains("{date}")
            && !matches!(
                extension.to_ascii_lowercase().as_str(),
                "jpg" | "jpeg" | "png" | "tif" | "tiff"
            ) {
            let metadata = raw_decode::RawSource::open(source)?.metadata();
            chrono::DateTime::from_timestamp(metadata.capture_time, 0)
                .map(|time| time.format("%Y%m%d").to_string())
                .unwrap_or_default()
        } else {
            String::new()
        };
        let filename = export::filename(&options.name, name, i + 1, &date, extension)?;
        ensure!(
            names.insert(filename.to_lowercase()),
            "duplicate original output names"
        );
        let destination = options.out.join(filename);
        ensure!(
            destination.symlink_metadata().is_err()
                && Sidecar::paths(&destination).xmp.symlink_metadata().is_err(),
            "original output already exists"
        );
        plans.push(destination);
    }
    let mut outputs = Vec::new();
    for (source, destination) in paths.iter().zip(plans) {
        cancel.check()?;
        let recipe = crate::catalog::document(source)?.recipe;
        let packet = packet(source)?;
        outputs.push(export::export_original(
            source,
            &destination,
            (Sidecar::paths(source).recipe.try_exists()? || packet.is_some()).then_some(&recipe),
            packet.as_ref(),
            cancel,
        )?);
    }
    let absolute = outputs
        .iter()
        .map(std::path::absolute)
        .collect::<std::io::Result<Vec<_>>>()?;
    let workflow_errors = export::run_after_export(actions, &absolute, cancel);
    Ok(
        json!({"total":paths.len(),"completed":outputs.len(),"outputs":outputs,"errors":[],"workflow_errors":workflow_errors,"cancelled":cancel.is_cancelled()}),
    )
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
    fn original_format_rejects_lossy_settings() {
        for extras in [
            vec![],
            vec!["--remove-location"],
            vec!["--metadata", "none"],
            vec!["--long-edge", "100"],
            vec!["--sharpen", "screen"],
        ] {
            let mut args = vec![
                "tessera",
                "export",
                "input.nef",
                "--out",
                "out",
                "--format",
                "original",
            ];
            args.extend(&extras);
            let parsed = crate::Cli::try_parse_from(args).unwrap();
            let crate::Command::Export(options) = parsed.command else {
                panic!("export")
            };
            assert_eq!(super::settings(&options).is_ok(), extras.is_empty());
        }
    }

    #[test]
    fn sharpening_strength_and_density_flags() {
        let parsed = crate::Cli::try_parse_from([
            "tessera",
            "export",
            "input.png",
            "--out",
            "out",
            "--format",
            "png",
            "--sharpen",
            "matte",
            "--sharpen-amount",
            "high",
            "--ppi",
            "240",
        ])
        .unwrap();
        let crate::Command::Export(options) = parsed.command else {
            panic!("export")
        };
        let settings = super::settings(&options).unwrap();
        assert!(matches!(settings.sharpen_for, export::SharpenFor::Matte));
        assert_eq!(settings.sharpen_amount, export::SharpenAmount::High);
        assert_eq!(settings.dpi, Some(240));
    }

    #[test]
    fn dng_flags_select_float_linear_export() {
        let parsed = crate::Cli::try_parse_from([
            "tessera",
            "export",
            "input.dng",
            "--out",
            "out",
            "--format",
            "dng",
            "--bit-depth",
            "32",
        ])
        .unwrap();
        let crate::Command::Export(options) = parsed.command else {
            panic!("export")
        };
        assert!(matches!(
            super::settings(&options).unwrap().format,
            export::Format::Dng
        ));
    }

    #[test]
    fn metadata_flags_reach_export_settings() {
        for policy in ["copyright-and-contact", "all-except-camera"] {
            let parsed = crate::Cli::try_parse_from([
                "tessera",
                "export",
                "input.png",
                "--out",
                "out",
                "--format",
                "png",
                "--metadata",
                policy,
                "--remove-person-info",
                "--remove-location",
                "--keywords-as-hierarchy",
                "false",
            ])
            .unwrap();
            let crate::Command::Export(options) = parsed.command else {
                panic!("export")
            };
            let settings = super::settings(&options).unwrap();
            assert!(settings.remove_person_info);
            assert!(settings.remove_location);
            assert!(!settings.keywords_as_hierarchy);
            assert!(matches!(
                (policy, settings.metadata),
                (
                    "copyright-and-contact",
                    export::Metadata::CopyrightAndContact
                ) | ("all-except-camera", export::Metadata::AllExceptCamera)
            ));
        }
    }

    #[test]
    fn jpeg_xl_flags_select_lossless_16_bit() {
        let parsed = crate::Cli::try_parse_from([
            "tessera",
            "export",
            "input.png",
            "--out",
            "out",
            "--format",
            "jxl",
            "--bit-depth",
            "16",
        ])
        .unwrap();
        let crate::Command::Export(options) = parsed.command else {
            panic!("export")
        };
        assert!(matches!(
            super::settings(&options).unwrap().format,
            export::Format::JpegXl { bits: 16 }
        ));
    }

    #[test]
    fn avif_flags_reach_encoder_settings() {
        use clap::CommandFactory;
        let mut command = crate::Cli::command();
        let help = command
            .find_subcommand_mut("export")
            .unwrap()
            .render_long_help()
            .to_string();
        for flag in ["avif", "--avif-speed", "--bit-depth"] {
            assert!(help.contains(flag));
        }
        for bits in ["8", "10", "12", "16"] {
            let parsed = crate::Cli::try_parse_from([
                "tessera",
                "export",
                "input.png",
                "--out",
                "out",
                "--format",
                "avif",
                "--bit-depth",
                bits,
                "--avif-speed",
                "10",
                "--quality",
                "73",
            ])
            .unwrap();
            let crate::Command::Export(options) = parsed.command else {
                panic!("export")
            };
            let result = super::settings(&options);
            if bits == "16" {
                assert!(result.is_err());
            } else {
                let export::Format::Avif(options) = result.unwrap().format else {
                    panic!("avif")
                };
                assert_eq!(
                    options,
                    export::AvifOptions {
                        bits: bits.parse().unwrap(),
                        speed: 10,
                        quality: 73
                    }
                );
            }
        }
    }

    #[test]
    fn byte_limit_and_watermark_flags_are_exposed() {
        use clap::CommandFactory;
        let mut command = crate::Cli::command();
        let help = command
            .find_subcommand_mut("export")
            .unwrap()
            .render_long_help()
            .to_string();
        assert!(help.contains("--max-file-bytes"));
        assert!(help.contains("--watermark"));
        assert!(
            crate::Cli::try_parse_from([
                "tessera",
                "export",
                "input.png",
                "--out",
                "out",
                "--format",
                "jpeg",
                "--max-file-bytes",
                "4096",
                "--watermark",
                "mark.json"
            ])
            .is_ok()
        );
        assert!(
            crate::Cli::try_parse_from([
                "tessera",
                "export",
                "input.png",
                "--out",
                "out",
                "--format",
                "jpeg",
                "--max-file-bytes",
                "0"
            ])
            .is_err()
        );
    }

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
