use crate::SavedSearch;
use engine_api::{EngineError, EngineResult, id::ImageId};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
};

/// Canonical cross-image document. Map keys are UI/basket handles, while IDs
/// survive renaming. Unknown fields survive culling and future schema additions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Library {
    pub schema_version: u32,
    pub roots: Vec<PathBuf>,
    pub albums: BTreeMap<String, Album>,
    pub album_groups: Vec<AlbumGroup>,
    pub smart_albums: Vec<SmartAlbum>,
    pub people: Vec<String>,
    pub keywords: Vec<Keyword>,
    pub marks_preset: BTreeMap<String, String>,
    pub publish_state: BTreeMap<String, serde_json::Value>,
    #[serde(flatten)]
    pub unknown: BTreeMap<String, serde_json::Value>,
}
impl Default for Library {
    fn default() -> Self {
        Self {
            schema_version: 1,
            roots: vec![],
            albums: BTreeMap::new(),
            album_groups: vec![],
            smart_albums: vec![],
            people: vec![],
            keywords: vec![],
            marks_preset: BTreeMap::new(),
            publish_state: BTreeMap::new(),
            unknown: BTreeMap::new(),
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Album {
    pub id: i64,
    pub name: String,
    pub parent: Option<i64>,
    pub images: Vec<ImageId>,
    #[serde(flatten)]
    pub unknown: BTreeMap<String, serde_json::Value>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlbumGroup {
    pub id: i64,
    pub name: String,
    pub parent: Option<i64>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SmartAlbum {
    pub id: i64,
    pub name: String,
    /// When set, only manual albums in this group and its descendants are searched.
    pub parent: Option<i64>,
    pub search: SavedSearch,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Keyword {
    pub id: i64,
    pub name: String,
    pub synonyms: Vec<String>,
    pub children: Vec<Keyword>,
}
impl Library {
    pub fn read(path: impl AsRef<Path>) -> EngineResult<Self> {
        let path = path.as_ref();
        match std::fs::read(path) {
            Ok(bytes) => {
                let document: Self = serde_json::from_slice(&bytes)?;
                if document.schema_version != 1 {
                    return Err(EngineError::SchemaVersion {
                        document: "library".into(),
                        found: document.schema_version,
                        supported: 1,
                    });
                }
                Ok(document)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(EngineError::io_at(path, &e)),
        }
    }
    /// Same-directory write, fsync and atomic rename. Original photos are never opened.
    pub fn write(&self, path: impl AsRef<Path>) -> EngineResult<()> {
        let path = path.as_ref();
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        temp.write_all(&serde_json::to_vec_pretty(self)?)?;
        temp.as_file().sync_all()?;
        temp.persist(path)
            .map_err(|e| EngineError::io_at(path, &e.error))?;
        std::fs::File::open(parent)?.sync_all()?;
        Ok(())
    }
    /// Membership hook for derived status. Returns basket handles, not duplicated state.
    pub fn in_album(&self, image: ImageId) -> Vec<String> {
        self.albums
            .iter()
            .filter(|(_, a)| a.images.contains(&image))
            .map(|(key, _)| key.clone())
            .collect()
    }
    /// Safe Delete: membership only. Disk deletion deliberately has no API here.
    pub fn remove_from_album(&mut self, id: i64, images: &[ImageId]) -> EngineResult<()> {
        let album = self
            .albums
            .values_mut()
            .find(|a| a.id == id)
            .ok_or_else(|| EngineError::not_found("album", id.to_string()))?;
        album.images.retain(|image| !images.contains(image));
        Ok(())
    }
    pub fn delete_album(&mut self, id: i64) -> EngineResult<()> {
        let key = self
            .albums
            .iter()
            .find(|(_, a)| a.id == id)
            .map(|(k, _)| k.clone())
            .ok_or_else(|| EngineError::not_found("album", id.to_string()))?;
        self.albums.remove(&key);
        Ok(())
    }
}
