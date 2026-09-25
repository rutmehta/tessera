//! Contract-facing catalog API. SQLite errors remain private.
use super::*;
use engine_api::error::{EngineError, EngineResult};

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
