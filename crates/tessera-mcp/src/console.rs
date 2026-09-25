use crate::{catalog, pixels, unsupported};
use engine_api::{
    EngineError, EngineResult,
    id::{HistoryEntryId, ImageId},
    recipe::{
        Author, EditMeta,
        history::{HistoryEntry, HistoryGroup},
    },
    tools::*,
};
use index::{Index, Query};
use sidecar::{RecipeDocument, Sidecar};
use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// Serialized, synchronous tool execution. Recipe documents remain the source of truth.
pub struct Console {
    pub(crate) index: Index,
    pub(crate) app: PathBuf,
}
impl Console {
    pub fn open(app_dir: impl AsRef<Path>) -> EngineResult<Self> {
        let app = app_dir.as_ref().to_owned();
        std::fs::create_dir_all(&app)?;
        Ok(Self {
            index: Index::open(app.join("index.sqlite"))?,
            app,
        })
    }
    pub fn execute(&mut self, request: ToolRequest) -> ToolResponse {
        self.run(&request).into()
    }
    pub fn open_image(&mut self, path: impl AsRef<Path>) -> EngineResult<ImageId> {
        let path = path.as_ref().canonicalize()?;
        if !path.is_file() {
            return Err(EngineError::invalid("path", "expected an image file"));
        }
        // The catalog's public scanner indexes directories. It owns stable IDs and metadata.
        self.index.scan(
            path.parent()
                .ok_or_else(|| EngineError::invalid("path", "missing parent"))?,
            &catalog::Reader,
            &catalog::Reader,
        )?;
        for id in self.index.search(&Query {
            limit: usize::MAX,
            ..Default::default()
        })? {
            if self.index.image_info(id)?.path == path {
                return Ok(id);
            }
        }
        Err(unsupported("image format is not indexable"))
    }
    pub(crate) fn document(&self, id: ImageId) -> EngineResult<(PathBuf, RecipeDocument)> {
        let path = self.index.image_info(id)?.path;
        let mut doc = catalog::document(&path)?;
        catalog::identify(&mut doc, id)?;
        Ok((path, doc))
    }
    pub fn render_preview(&self, id: ImageId, max_px: u32) -> EngineResult<image::RgbImage> {
        let (path, doc) = self.document(id)?;
        pixels::render(&path, &doc.recipe.settings, max_px)
    }
    fn run(&mut self, request: &ToolRequest) -> EngineResult<ToolOutput> {
        match &request.call {
            ToolCall::SetTone { image, update } => {
                let (path, mut doc) = self.document(*image)?;
                self.check(&doc, request)?;
                validate_tone(update)?;
                let meta = meta(request);
                let entry=doc.recipe.edit(meta.clone(),|s| {
                    macro_rules! set { ($target:expr, $($field:ident),*) => { $(if let Some(v)=update.$field { $target.$field=v; })* }; }
                    set!(s.tone,exposure,contrast,highlights,shadows,whites,blacks,texture,clarity,dehaze);
                    set!(s.color,vibrance,saturation);
                    set!(s.white_balance,temperature,tint);
                    if update.temperature.is_some()||update.tint.is_some() {s.white_balance.mode=engine_api::recipe::settings::WhiteBalanceMode::Custom;}
                })?;
                let entry = entry.unwrap_or_else(|| record_empty(&mut doc, meta));
                self.save(&path, &mut doc)?;
                Ok(ToolOutput::Edited {
                    image: *image,
                    entry: Some(entry),
                    recipe: doc.recipe.recipe_hash(),
                })
            }
            ToolCall::GetHistogram { image, space, bins } => {
                let histogram = match space {
                    HistogramSpace::Display => {
                        pixels::histogram(&self.render_preview(*image, 1024)?, *bins)?
                    }
                    HistogramSpace::SceneLinear => {
                        let (path, doc) = self.document(*image)?;
                        pixels::linear_histogram(&path, &doc.recipe.settings, *bins)?
                    }
                };
                Ok(ToolOutput::Histogram {
                    image: *image,
                    histogram,
                })
            }
            ToolCall::RemoveObject { .. } => Err(unsupported(
                "remove_object requires non-generative inpainting, which is not implemented",
            )),
            ToolCall::RetouchSkin { .. } => Err(unsupported(
                "retouch_skin is not implemented; no pixels were generated",
            )),
            ToolCall::CreateMask { .. }
            | ToolCall::AdjustMask { .. }
            | ToolCall::Crop { .. }
            | ToolCall::ApplyStyle { .. } => self.edit(request),
            ToolCall::SetSelection { .. } => self.selection(request),
            ToolCall::Compare {
                image_a,
                image_b,
                metric,
            } => {
                let comparison = self.compare_images(*image_a, *image_b, *metric)?;
                Ok(ToolOutput::Comparison {
                    metric: *metric,
                    distance: comparison.distance,
                    summary: serde_json::to_string(&comparison.metrics)?,
                })
            }
            ToolCall::GetScores { image } => Ok(ToolOutput::Scores {
                image: *image,
                scores: self.scores(*image)?,
            }),
            ToolCall::IndexFolder { path, recursive } => {
                self.index_folder(path, *recursive, request)
            }
            ToolCall::Export { images, settings } => self.export_images(images, settings, request),
        }
    }
    pub(crate) fn check(&self, doc: &RecipeDocument, request: &ToolRequest) -> EngineResult<()> {
        if request
            .expect_recipe
            .is_some_and(|hash| hash != doc.recipe.recipe_hash())
        {
            return Err(EngineError::Conflict {
                message: "recipe changed since the request was prepared".into(),
            });
        }
        Ok(())
    }
    pub(crate) fn save(&self, path: &Path, doc: &mut RecipeDocument) -> EngineResult<()> {
        catalog::writable(path)?;
        doc.recipe.validate()?;
        for entry in &doc.recipe.history.entries {
            if let Some(id) = entry.meta.group
                && !doc.recipe.history.groups.iter().any(|g| g.id == id)
            {
                doc.recipe.history.groups.push(HistoryGroup {
                    id,
                    name: "Agent base edit".into(),
                });
            }
        }
        doc.record_write("tessera-mcp", now())?;
        Sidecar::write_recipe(Sidecar::paths(path).recipe, doc)
    }
}
pub(crate) fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}
pub(crate) fn meta(request: &ToolRequest) -> EditMeta {
    EditMeta {
        label: request.call.name().into(),
        author: Author::Agent {
            name: "tessera-mcp".into(),
        },
        timestamp_ms: now(),
        rationale: request.rationale.clone(),
        group: request.group,
    }
}
pub(crate) fn record_empty(doc: &mut RecipeDocument, meta: EditMeta) -> HistoryEntryId {
    let history = &mut doc.recipe.history;
    let id = HistoryEntryId(history.entries.len() as u64 + 1);
    history.entries.push(HistoryEntry {
        id,
        parent: history.head,
        meta,
        changes: vec![],
    });
    history.head = Some(id);
    id
}
fn validate_tone(update: &ToneUpdate) -> EngineResult<()> {
    for (name, value, min, max) in [
        ("exposure", update.exposure, -10., 10.),
        ("contrast", update.contrast, -100., 100.),
        ("highlights", update.highlights, -100., 100.),
        ("shadows", update.shadows, -100., 100.),
        ("whites", update.whites, -100., 100.),
        ("blacks", update.blacks, -100., 100.),
        ("texture", update.texture, -100., 100.),
        ("clarity", update.clarity, -100., 100.),
        ("dehaze", update.dehaze, -100., 100.),
        ("vibrance", update.vibrance, -100., 100.),
        ("saturation", update.saturation, -100., 100.),
        ("temperature", update.temperature, 2000., 50000.),
        ("tint", update.tint, -150., 150.),
    ] {
        if value.is_some_and(|v| !v.is_finite() || !(min..=max).contains(&v)) {
            return Err(EngineError::invalid(
                name,
                format!("must be finite and within {min}..={max}"),
            ));
        }
    }
    Ok(())
}
