use anyhow::{Context, Result, ensure};
use clap::Args;
use index::{Index, Understanding};
use ml_caption::{Calibration, Florence, KeywordModel, WritePolicy};
use ml_runtime::{ModelRegistry, SessionOptions};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

#[derive(Args)]
pub struct Options {
    pub image: PathBuf,
    /// Model registry. Defaults to APP_DIR/models.toml.
    #[arg(long)]
    pub manifest: Option<PathBuf>,
    /// SHA-addressed model directory, including the separately pinned tokenizer.
    #[arg(long)]
    pub cache: Option<PathBuf>,
    /// Store model output in the index. Image must already be indexed.
    #[arg(long)]
    pub store: bool,
    /// Opt into CoreML. CPU is default for this dynamic autoregressive export.
    #[arg(long)]
    pub coreml: bool,
    /// Include actual executed node/provider counts.
    #[arg(long)]
    pub partition_report: bool,
}
#[derive(Args)]
pub struct KeywordOptions {
    #[command(flatten)]
    pub common: Options,
    #[arg(long,default_value_t=20,value_parser=clap::value_parser!(u32).range(1..=10000))]
    pub top: u32,
    #[arg(long, default_value_t = 0.05)]
    pub temperature: f32,
    #[arg(long, default_value_t = 0.2)]
    pub midpoint: f32,
    /// Optional newline-delimited vocabulary instead of bundled photographic concepts.
    #[arg(long)]
    pub vocabulary: Option<PathBuf>,
    /// Explicitly accept the returned top suggestions into the keyword tree/index.
    #[arg(long)]
    pub accept: bool,
    /// Also write accepted keywords to XMP. Originals are never written.
    #[arg(long, requires = "accept")]
    pub write_xmp: bool,
}
pub fn run(app: &Path, index: &Index, command: &crate::Ml) -> Result<Value> {
    let (options, keyword_options) = match command {
        crate::Ml::Keywords(o) => (&o.common, Some(o)),
        crate::Ml::Caption(o) | crate::Ml::Ocr(o) => (o, None),
        _ => unreachable!(),
    };
    let id = if options.store || keyword_options.is_some_and(|o| o.accept) {
        Some(crate::catalog::resolve(index, &options.image)?.0)
    } else {
        None
    };
    let path = options
        .image
        .canonicalize()
        .context("image is unavailable")?;
    let manifest = options
        .manifest
        .clone()
        .unwrap_or_else(|| app.join("models.toml"));
    let cache = options.cache.clone().unwrap_or_else(|| app.join("models"));
    let registry = ModelRegistry::open(&manifest, &cache).context(
        "install crates/ml-runtime/models.toml as APP_DIR/models.toml, or use --manifest",
    )?;
    let prefix = if keyword_options.is_some() {
        "siglip/"
    } else {
        "caption/florence-"
    };
    let specs: Vec<_> = registry
        .models()
        .iter()
        .filter(|s| s.id.starts_with(prefix))
        .collect();
    ensure!(!specs.is_empty(), "required models absent from manifest");
    for spec in specs {
        ensure!(
            cache.join(format!("{}.onnx", spec.sha256)).is_file(),
            "missing model {}; run tools/fetch_siglip.py or tools/fetch_florence.py --cache {}",
            spec.id,
            cache.display()
        );
    }
    let image = ml_caption::decode_image(&path, &app.join("previews"))?;
    let session = if options.coreml {
        SessionOptions::default()
    } else {
        SessionOptions::cpu()
    };
    let mut value = if let Some(id) = id {
        index.understanding(id)?.unwrap_or_else(empty)
    } else {
        empty()
    };
    let mut output;
    if let Some(o) = keyword_options {
        let labels = match &o.vocabulary {
            Some(p) => std::fs::read_to_string(p)?
                .lines()
                .map(str::trim)
                .filter(|s| !s.is_empty() && !s.starts_with('#'))
                .map(str::to_owned)
                .collect(),
            None => ml_caption::vocabulary(),
        };
        let model = ml_embed::Siglip::load(&registry, &cache.join("tokenizer.json"), session)?;
        let mut model = KeywordModel::new(
            model,
            labels,
            Calibration {
                temperature: o.temperature,
                midpoint: o.midpoint,
            },
        )?;
        let keywords = model.suggest_keywords(&image)?;
        value.keywords = keywords
            .into_iter()
            .take(o.top as usize)
            .map(|(keyword, confidence)| index::KeywordSuggestion {
                keyword,
                confidence,
            })
            .collect();
        set_version(&mut value, "keywords", &model.model_version());
        let mut library = library::Library::read(app.join("library.json"))?;
        let mapped = value
            .keywords
            .iter()
            .map(|s| ml_caption::map_keyword(&library, &s.keyword))
            .collect::<Result<Vec<_>>>()?;
        output =
            json!({"keywords":value.keywords,"mapping":mapped,"model_version":value.model_version});
        if o.accept {
            let names = value
                .keywords
                .iter()
                .map(|s| s.keyword.clone())
                .collect::<Vec<_>>();
            ml_caption::accept_keywords(
                index,
                &mut library,
                &[id.context("missing indexed image")?],
                &names,
                if o.write_xmp {
                    WritePolicy::Xmp
                } else {
                    WritePolicy::IndexOnly
                },
            )?;
            library.write(app.join("library.json"))?;
            output["accepted"] = json!(names);
        }
        if options.partition_report {
            let (vision, text) = model.partition_reports()?;
            output["partitions"] = reports(vec![("vision".into(), vision), ("text".into(), text)]);
        }
    } else {
        let mut model = Florence::load(&registry, &cache.join("florence-tokenizer.json"), session)?;
        if matches!(command, crate::Ml::Caption(_)) {
            let caption = model.caption(&image)?;
            value.caption = caption.caption;
            value.alt_text = caption.alt_text;
            set_version(&mut value, "caption", ml_caption::FLORENCE_VERSION);
            output = json!({"caption":value.caption,"alt_text":value.alt_text,"model_version":value.model_version});
        } else {
            value.ocr = model.ocr(&image)?;
            set_version(&mut value, "ocr", ml_caption::FLORENCE_VERSION);
            output = json!({"ocr":value.ocr,"model_version":value.model_version});
        }
        if options.partition_report {
            output["partitions"] = reports(model.partition_reports()?);
        }
    }
    if options.store {
        index.set_understanding(id.context("missing indexed image")?, &value)?;
    }
    Ok(output)
}
fn empty() -> Understanding {
    Understanding {
        model_version: String::new(),
        keywords: vec![],
        caption: String::new(),
        alt_text: String::new(),
        ocr: vec![],
    }
}
// Retain provenance for untouched fields when updating only one CLI task.
fn set_version(value: &mut Understanding, task: &str, version: &str) {
    let mut versions: serde_json::Map<String, Value> =
        serde_json::from_str(&value.model_version).unwrap_or_default();
    if versions.is_empty() && !value.model_version.is_empty() {
        versions.insert("previous".into(), json!(value.model_version));
    }
    versions.insert(task.into(), json!(version));
    value.model_version = Value::Object(versions).to_string();
}
fn reports(reports: Vec<(String, ml_runtime::PartitionReport)>) -> Value {
    let mut result = serde_json::Map::new();
    for (name, report) in reports {
        let mut counts = std::collections::BTreeMap::new();
        for node in report.nodes {
            *counts.entry(node.provider).or_insert(0usize) += 1;
        }
        result.insert(name, json!(counts));
    }
    Value::Object(result)
}
