use crate::{Action, CullSession, ImageId, persistence};
use engine_api::{EngineError, EngineResult};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// Minimal library.json schema. Album keys are names; member order is manual.
/// Unrecognized library/album fields survive basket writes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Library {
    pub albums: BTreeMap<String, Album>,
    #[serde(flatten)]
    pub unknown: BTreeMap<String, serde_json::Value>,
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Album {
    pub images: Vec<ImageId>,
    #[serde(flatten)]
    pub unknown: BTreeMap<String, serde_json::Value>,
}
impl Library {
    pub fn read(path: impl AsRef<Path>) -> EngineResult<Self> {
        match persistence::optional_bytes(path.as_ref())? {
            Some(bytes) => Ok(serde_json::from_slice(&bytes)?),
            None => Ok(Self::default()),
        }
    }
    pub fn write(&self, path: impl AsRef<Path>) -> EngineResult<()> {
        persistence::atomic_write(path.as_ref(), &serde_json::to_vec_pretty(self)?)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Unedited,
    Edited,
    Exported,
    Published,
}
/// Album membership is orthogonal: a published image can belong to many albums.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DerivedStatus {
    pub status: Status,
    pub in_album: Vec<String>,
}
#[derive(Clone)]
pub(crate) struct BasketChange {
    path: PathBuf,
    album: String,
    before: Option<Album>,
    after: Option<Album>,
}
impl BasketChange {
    pub(crate) fn write(&self, forward: bool) -> EngineResult<()> {
        let mut library = Library::read(&self.path)?;
        let (expected, replacement) = if forward {
            (&self.before, &self.after)
        } else {
            (&self.after, &self.before)
        };
        if library.albums.get(&self.album) != expected.as_ref() {
            return Err(EngineError::invalid(
                "basket",
                "album changed outside this undo stack",
            ));
        }
        if let Some(album) = replacement {
            library.albums.insert(self.album.clone(), album.clone());
        } else {
            library.albums.remove(&self.album);
        }
        library.write(&self.path)
    }
}
impl CullSession<'_> {
    pub fn set_library(&mut self, path: impl AsRef<Path>) -> EngineResult<()> {
        Library::read(&path)?;
        self.library = Some(path.as_ref().to_path_buf());
        Ok(())
    }
    /// Select or create a named target album on the next toggle.
    pub fn set_basket_target(&mut self, name: impl Into<String>) -> EngineResult<()> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(EngineError::invalid("album", "empty name"));
        }
        self.basket_target = Some(name);
        Ok(())
    }
    pub fn basket_target(&self) -> Option<&str> {
        self.basket_target.as_deref()
    }
    /// Returns true when added. Membership lives only in library.json, never a
    /// per-image sidecar. The operation shares the selection undo/redo stack.
    pub fn toggle_basket(&mut self) -> EngineResult<bool> {
        let id = self.require_current()?;
        let path = self.library.clone().ok_or_else(|| {
            EngineError::invalid("library", "set library.json for a query session")
        })?;
        let album = self
            .basket_target
            .clone()
            .ok_or_else(|| EngineError::invalid("basket", "select a target album"))?;
        let library = Library::read(&path)?;
        let before = library.albums.get(&album).cloned();
        let mut after = before.clone().unwrap_or_default();
        let added = !after.images.contains(&id);
        if added {
            after.images.push(id);
        } else {
            after.images.retain(|image| *image != id);
        }
        let basket = BasketChange {
            path,
            album,
            before,
            after: Some(after),
        };
        basket.write(true)?;
        self.undo.push(Action {
            changes: Vec::new(),
            before_position: self.position,
            after_position: self.position,
            basket: Some(basket),
        });
        self.redo.clear();
        Ok(added)
    }
    pub fn derived_status(&self, id: ImageId) -> EngineResult<DerivedStatus> {
        let document = persistence::load(self.index, id)?;
        let (exported, published) = self.index.export_status(id)?;
        let status = if published {
            Status::Published
        } else if exported {
            Status::Exported
        } else if !document.recipe.history.entries.is_empty() {
            Status::Edited
        } else {
            Status::Unedited
        };
        let library = self
            .library
            .as_ref()
            .map(Library::read)
            .transpose()?
            .unwrap_or_default();
        let in_album = library
            .albums
            .into_iter()
            .filter_map(|(name, album)| album.images.contains(&id).then_some(name))
            .collect();
        Ok(DerivedStatus { status, in_album })
    }
}
