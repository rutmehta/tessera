use crate::{Action, CullSession, ImageId, persistence};
use engine_api::{EngineError, EngineResult};
use index::Index;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    ops::Deref,
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
    /// Images whose membership differs between the two album states.
    pub(crate) fn image_ids(&self) -> Vec<ImageId> {
        let members =
            |album: &Option<Album>| album.as_ref().map(|a| a.images.clone()).unwrap_or_default();
        let (before, after) = (members(&self.before), members(&self.after));
        let mut ids: Vec<_> = before
            .iter()
            .filter(|id| !after.contains(id))
            .copied()
            .collect();
        ids.extend(after.iter().filter(|id| !before.contains(id)));
        ids
    }
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
impl<I: Deref<Target = Index>> CullSession<I> {
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
    pub fn library_path(&self) -> Option<&Path> {
        self.library.as_deref()
    }
    /// All albums in library.json (empty when no library is configured).
    pub fn library(&self) -> EngineResult<Library> {
        self.library
            .as_ref()
            .map(Library::read)
            .transpose()
            .map(Option::unwrap_or_default)
    }
    /// Returns true when added. Membership lives only in library.json, never a
    /// per-image sidecar. The operation shares the selection undo/redo stack.
    pub fn toggle_basket(&mut self) -> EngineResult<bool> {
        let id = self.require_current()?;
        let album = self.require_target()?;
        let mut added = false;
        self.change_album(album, |images| {
            added = !images.contains(&id);
            if added {
                images.push(id);
            } else {
                images.retain(|image| *image != id);
            }
        })?;
        Ok(added)
    }
    /// Adds (in order, skipping members) or removes a batch from the basket
    /// target as one undo step. A no-op batch records no history.
    pub fn set_basket(&mut self, ids: &[ImageId], add: bool) -> EngineResult<()> {
        self.require_queued(ids)?;
        let album = self.require_target()?;
        self.change_album(album, |images| {
            for id in ids {
                if add && !images.contains(id) {
                    images.push(*id);
                } else if !add {
                    images.retain(|image| image != id);
                }
            }
        })
    }
    /// Safe delete inside an album (docs/06 §4.2): removes membership only.
    /// Files, sidecars and decisions are untouched. One undo step.
    pub fn remove_from_album(&mut self, album: &str, ids: &[ImageId]) -> EngineResult<()> {
        if !self.library()?.albums.contains_key(album) {
            return Err(EngineError::not_found("album", album));
        }
        self.change_album(album.to_owned(), |images| {
            images.retain(|image| !ids.contains(image))
        })
    }
    fn require_target(&self) -> EngineResult<String> {
        self.basket_target
            .clone()
            .ok_or_else(|| EngineError::invalid("basket", "select a target album"))
    }
    fn require_queued(&self, ids: &[ImageId]) -> EngineResult<()> {
        if ids.iter().any(|id| !self.images.contains(id)) {
            return Err(EngineError::invalid("image", "not in review queue"));
        }
        Ok(())
    }
    fn change_album(
        &mut self,
        album: String,
        change: impl FnOnce(&mut Vec<ImageId>),
    ) -> EngineResult<()> {
        let path = self.library.clone().ok_or_else(|| {
            EngineError::invalid("library", "set library.json for a query session")
        })?;
        let library = Library::read(&path)?;
        let before = library.albums.get(&album).cloned();
        let mut after = before.clone().unwrap_or_default();
        change(&mut after.images);
        if before.as_ref().map(|a| &a.images) == Some(&after.images)
            || (before.is_none() && after.images.is_empty())
        {
            return Ok(());
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
        Ok(())
    }
    /// Batch form of `derived_status` that reads library.json once.
    pub fn derived_statuses(&self, ids: &[ImageId]) -> EngineResult<Vec<DerivedStatus>> {
        let library = self.library()?;
        ids.iter()
            .map(|id| self.status_with(&library, *id))
            .collect()
    }
    pub fn derived_status(&self, id: ImageId) -> EngineResult<DerivedStatus> {
        self.status_with(&self.library()?, id)
    }
    fn status_with(&self, library: &Library, id: ImageId) -> EngineResult<DerivedStatus> {
        let document = persistence::load(&self.index, id)?;
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
        let in_album = library
            .albums
            .iter()
            .filter(|(_, album)| album.images.contains(&id))
            .map(|(name, _)| name.clone())
            .collect();
        Ok(DerivedStatus { status, in_album })
    }
}
