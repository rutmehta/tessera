use crate::{Error, Frame, Result, naming};
use engine_api::{
    EngineResult,
    jobs::{Job, JobContext, Priority},
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
};

pub(crate) enum Faces {
    Pending(Option<PathBuf>),
    Ready(Box<ml_faces::FaceModels>),
    Unavailable(String),
}
impl Faces {
    fn analyze(
        &mut self,
        index: &index::Index,
        id: engine_api::id::ImageId,
        rgb: &image::RgbImage,
    ) -> Option<String> {
        if let Self::Pending(dir) = self {
            let loaded = dir
                .as_ref()
                .ok_or_else(|| "face models not configured".to_string())
                .and_then(|dir| {
                    ml_runtime::ModelRegistry::open(dir.join("models.toml"), dir.join("models"))
                        .and_then(|r| ml_faces::FaceModels::load(&r, Default::default()))
                        .map_err(|e| e.to_string())
                });
            *self = match loaded {
                Ok(m) => Self::Ready(Box::new(m)),
                Err(e) => Self::Unavailable(e),
            };
        }
        match self {
            Self::Ready(m) => m
                .analyze_and_store(index, id, rgb)
                .err()
                .map(|e| e.to_string()),
            Self::Unavailable(e) => Some(e.clone()),
            Self::Pending(_) => unreachable!(),
        }
    }
}
pub(crate) struct Ingest {
    pub source: PathBuf,
    pub staging: PathBuf,
    pub folder: PathBuf,
    pub naming: String,
    pub db: PathBuf,
    pub sequence: u64,
    pub faces: Arc<Mutex<Faces>>,
    pub tx: mpsc::Sender<Frame>,
}
impl Ingest {
    fn ingest(&self, event: &mut Frame, ctx: &JobContext) -> Result<()> {
        ctx.check_cancelled()?;
        let source = self.source.canonicalize()?;
        if !source.starts_with(self.staging.canonicalize()?)
            || !fs::symlink_metadata(&self.source)?.is_file()
        {
            return Err(Error::Message(
                "backend file is outside staging or not a regular file".into(),
            ));
        }
        let name = naming::render(&self.naming, &source, self.sequence)?;
        let mut destination = self.folder.join(&name);
        // Copy to a hidden temp file first, then publish atomically, never overwrite.
        let mut temp = tempfile::NamedTempFile::new_in(&self.folder)?;
        std::io::copy(&mut fs::File::open(&source)?, &mut temp)?;
        temp.as_file().sync_all()?;
        let (stem, ext) = name.rsplit_once('.').expect("validated extension");
        let mut collision = 0u64;
        loop {
            match temp.persist_noclobber(&destination) {
                Ok(_) => break,
                Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {
                    ctx.check_cancelled()?;
                    collision += 1;
                    temp = e.file;
                    destination = self.folder.join(format!("{stem}-{collision}.{ext}"));
                }
                Err(e) => return Err(Error::Io(e.error)),
            }
        }
        event.path = destination.clone();
        fs::remove_file(source)?;
        ctx.report_progress(0.2, Some("downloaded"));
        let mut index = index::Index::open(&self.db)?;
        index::Scanner::new(&Sidecars, &index::NoopMetadataProvider)
            .scan(&mut index, &self.folder)?;
        let id = index
            .search(&index::Query {
                limit: i64::MAX as usize,
                ..Default::default()
            })?
            .into_iter()
            .find(|id| index.image_info(*id).is_ok_and(|i| i.path == destination))
            .ok_or_else(|| Error::Message("downloaded file format is not indexed".into()))?;
        event.image_id = Some(id);
        ctx.check_cancelled()?;
        ctx.report_progress(0.4, Some("indexed"));
        let ext = destination
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        let bytes = if matches!(ext.as_str(), "jpg" | "jpeg") {
            fs::read(&destination)?
        } else {
            raw_decode::RawSource::open(&destination)?
                .embedded_preview()
                .ok_or_else(|| Error::Message("no embedded JPEG preview".into()))?
        };
        let rgb = image::load_from_memory_with_format(&bytes, image::ImageFormat::Jpeg)
            .map_err(|e| Error::Message(e.to_string()))?
            .thumbnail(2048, 2048)
            .to_rgb8();
        let previews = self
            .db
            .parent()
            .unwrap_or(Path::new("."))
            .join("tether-previews");
        fs::create_dir_all(&previews)?;
        let preview = previews.join(format!("{id}.preview"));
        let mut temp = tempfile::NamedTempFile::new_in(&previews)?;
        image::codecs::jpeg::JpegEncoder::new(&mut temp)
            .encode_image(&rgb)
            .map_err(|e| Error::Message(e.to_string()))?;
        temp.persist(&preview).map_err(|e| Error::Io(e.error))?;
        event.preview = Some(preview);
        ctx.report_progress(0.6, Some("preview extracted"));
        ml_quality::analyze_and_store(&index, id, &rgb)?;
        ctx.check_cancelled()?;
        event.face_warning = self
            .faces
            .lock()
            .map_err(|e| Error::Message(e.to_string()))?
            .analyze(&index, id, &rgb);
        ctx.report_progress(1.0, Some("scored"));
        Ok(())
    }
}
impl Job for Ingest {
    fn label(&self) -> &str {
        "tether ingest and score"
    }
    fn priority(&self) -> Priority {
        Priority::Score
    }
    fn run(self: Box<Self>, ctx: &JobContext) -> EngineResult<()> {
        let mut event = Frame {
            sequence: self.sequence,
            path: self.source.clone(),
            image_id: None,
            preview: None,
            face_warning: None,
            error: None,
        };
        let result = self.ingest(&mut event, ctx);
        if let Err(e) = &result {
            event.error = Some(e.to_string());
        }
        let _ = self.tx.send(event);
        result.map_err(|e| engine_api::EngineError::invalid("tether ingest", e.to_string()))
    }
}
pub(crate) struct Barrier(pub mpsc::Sender<()>);
impl Job for Barrier {
    fn label(&self) -> &str {
        "tether drain"
    }
    fn priority(&self) -> Priority {
        Priority::Score
    }
    fn run(self: Box<Self>, _: &JobContext) -> EngineResult<()> {
        let _ = self.0.send(());
        Ok(())
    }
}

// Preserve existing decisions/metadata when the incremental scanner revisits a
// session. NoopSidecarReader would erase changed sidecar values.
struct Sidecars;
impl index::SidecarReader for Sidecars {
    fn read(&self, path: &Path) -> EngineResult<index::SidecarData> {
        let paths = sidecar::Sidecar::paths(path);
        let mut data = index::SidecarData::default();
        let xmp = if paths.xmp.exists() {
            paths.xmp
        } else {
            path.with_extension("xmp")
        };
        if xmp.exists() {
            let packet = sidecar::Sidecar::read_xmp(xmp)?;
            let meta = packet.metadata()?;
            data.caption = Some(meta.description);
            data.keywords = meta.keywords;
            data.selection = Some(packet.selection()?);
        }
        if paths.recipe.exists() {
            let doc = sidecar::Sidecar::read_recipe(paths.recipe)?;
            data.selection = Some(doc.recipe.selection.clone());
            data.recipe_hash = Some(doc.recipe.recipe_hash().0);
        }
        Ok(data)
    }
}
