//! Contract-facing catalog API. SQLite errors remain private.
use super::*;
use engine_api::error::{EngineError, EngineResult};

/// Minimal catalog facts needed for culling. Unknown capture times remain absent.
#[derive(Debug, Clone)]
pub struct ImageInfo {
    pub id: ImageId,
    pub path: std::path::PathBuf,
    pub size: u64,
    pub capture_seconds: Option<f64>,
}

/// Latest value for a named signal; model includes the producing model version.
#[derive(Debug, Clone, PartialEq)]
pub struct Score {
    pub signal: String,
    pub value: f64,
    pub model: String,
}

fn sql_error(error: rusqlite::Error) -> EngineError {
    IndexError::Sql(error).into()
}

impl From<IndexError> for EngineError {
    fn from(error: IndexError) -> Self {
        match error {
            IndexError::Engine(error) => error,
            IndexError::Io(error) => error.into(),
            IndexError::Metadata(message) => Self::Decode {
                format: "index".into(),
                message,
            },
            error => Self::Io {
                path: None,
                message: error.to_string(),
            },
        }
    }
}

/// Rebuildable SQLite catalog. Each instance owns one connection.
#[derive(Debug)]
pub struct Index(Core);
impl Index {
    pub fn open(path: impl AsRef<Path>) -> EngineResult<Self> {
        Core::open(path).map(Self).map_err(Into::into)
    }
    pub fn scan(
        &mut self,
        root: impl AsRef<Path>,
        sidecars: &dyn SidecarReader,
        metadata: &dyn MetadataProvider,
    ) -> EngineResult<usize> {
        self.0.scan(root, sidecars, metadata).map_err(Into::into)
    }
    pub fn search(&self, query: &Query) -> EngineResult<Vec<ImageId>> {
        self.0.search(query).map_err(Into::into)
    }
    pub fn facets(&self, query: &Query) -> EngineResult<Facets> {
        self.0.facets(query).map_err(Into::into)
    }
    pub fn set_selection(&self, id: ImageId, selection: &Selection) -> EngineResult<()> {
        self.0.set_selection(id, selection).map_err(Into::into)
    }
    pub fn selection(&self, id: ImageId) -> EngineResult<Option<Selection>> {
        self.0.selection(id).map_err(Into::into)
    }
    /// `capture_seconds` accepts ISO date-times and numeric Unix seconds (RAW
    /// scanners store the latter; 'auto' keeps them from parsing as Julian days).
    pub fn image_info(&self, id: ImageId) -> EngineResult<ImageInfo> {
        self.0.conn.query_row(
            "SELECT f.path,f.size,unixepoch(i.capture_time,'auto','subsec') FROM image i JOIN file f ON f.id=i.file_id WHERE i.id=?",
            [id.to_string()],
            |r| Ok(ImageInfo { id, path: r.get::<_, String>(0)?.into(), size: r.get::<_, i64>(1)?.max(0) as u64, capture_seconds: r.get(2)? }),
        ).optional().map_err(sql_error)?.ok_or_else(|| EngineError::not_found("image", id))
    }
    pub fn set_score(&self, id: ImageId, score: &Score) -> EngineResult<()> {
        if !score.value.is_finite() || score.signal.is_empty() || score.model.is_empty() {
            return Err(EngineError::invalid(
                "score",
                "requires finite value, signal and model",
            ));
        }
        self.0.conn.execute(
            "INSERT INTO score(image_id,signal,value,model) VALUES(?,?,?,?) ON CONFLICT(image_id,signal) DO UPDATE SET value=excluded.value,model=excluded.model",
            params![id.to_string(),score.signal,score.value,score.model],
        ).map_err(sql_error)?;
        Ok(())
    }
    pub fn scores(&self, id: ImageId) -> EngineResult<Vec<Score>> {
        let mut stmt = self
            .0
            .conn
            .prepare("SELECT signal,value,model FROM score WHERE image_id=? ORDER BY signal")
            .map_err(sql_error)?;
        stmt.query_map([id.to_string()], |r| {
            Ok(Score {
                signal: r.get(0)?,
                value: r.get(1)?,
                model: r.get(2)?,
            })
        })
        .map_err(sql_error)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(sql_error)
    }
    pub fn record_export(
        &self,
        id: ImageId,
        destination: &str,
        published: bool,
    ) -> EngineResult<()> {
        self.0
            .conn
            .execute(
                "INSERT INTO export_log(image_id,destination,published) VALUES(?,?,?)",
                params![id.to_string(), destination, published],
            )
            .map_err(sql_error)?;
        Ok(())
    }
    /// (ever exported, ever published). These are historical, not freshness flags.
    pub fn export_status(&self, id: ImageId) -> EngineResult<(bool, bool)> {
        self.0
            .conn
            .query_row(
                "SELECT count(*)>0,coalesce(max(published),0) FROM export_log WHERE image_id=?",
                [id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(sql_error)
    }
    pub fn add_keyword(&self, name: &str, parent: Option<&str>) -> EngineResult<i64> {
        self.0.add_keyword(name, parent).map_err(Into::into)
    }
    pub fn tag(&self, id: ImageId, keyword: &str) -> EngineResult<()> {
        self.0.tag(id, keyword).map_err(Into::into)
    }
    pub fn images_with_keyword(&self, name: &str) -> EngineResult<Vec<ImageId>> {
        self.0.images_with_keyword(name).map_err(Into::into)
    }
}

/// Scanner hooks receive image paths, not sidecar paths. XMP (both naming
/// conventions) and `.edits/<stem>.json` are tracked for invalidation.
pub struct Scanner<'a> {
    pub sidecars: &'a dyn SidecarReader,
    pub metadata: &'a dyn MetadataProvider,
}
impl<'a> Scanner<'a> {
    pub fn new(sidecars: &'a dyn SidecarReader, metadata: &'a dyn MetadataProvider) -> Self {
        Self { sidecars, metadata }
    }
    /// Returns the number of inserted or refreshed images.
    pub fn scan(&self, index: &mut Index, root: impl AsRef<Path>) -> EngineResult<usize> {
        index.scan(root, self.sidecars, self.metadata)
    }
}
