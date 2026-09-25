//! Catalog/sidecar boundary. Expensive ML inference stays in existing Score jobs.
use crate::{Features, Profile, RecipeStore};
use engine_api::{
    id::ImageId,
    recipe::{history::Author, Recipe},
    EngineError, EngineResult,
};
use sidecar::{RecipeDocument, Sidecar};
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
};

pub(crate) fn document(path: &Path) -> EngineResult<RecipeDocument> {
    let paths = Sidecar::paths(path);
    if paths.recipe.try_exists()? {
        return Sidecar::read_recipe(paths.recipe);
    }
    let xmp = if paths.xmp.try_exists()? {
        paths.xmp
    } else {
        path.with_extension("xmp")
    };
    if xmp.try_exists()? {
        return Ok(RecipeDocument {
            recipe: Sidecar::read_xmp(xmp)?.to_recipe()?.recipe,
            ..RecipeDocument::default()
        });
    }
    Ok(RecipeDocument::default())
}
impl Profile {
    /// Enumerates every catalog image (no default query limit), loading actual
    /// recipe history. The provider supplies cached SigLIP vectors and unedited
    /// linear scene/face measurements; missing measurements are an explicit error.
    pub fn collect_library(
        &mut self,
        index: &index::Index,
        mut features: impl FnMut(ImageId, &Path) -> EngineResult<Features>,
    ) -> EngineResult<usize> {
        let mut rows = Vec::new();
        for id in index.search(&index::Query {
            limit: usize::MAX,
            ..Default::default()
        })? {
            let info = index.image_info(id)?;
            let recipe = document(&info.path)?.recipe;
            if recipe
                .history
                .lineage(recipe.history.head)?
                .iter()
                .any(|e| matches!(e.meta.author, Author::User))
            {
                rows.push((id, features(id, &info.path)?, recipe));
            }
        }
        self.collect(rows)
    }
}
/// File-backed store for hosts with a single recipe writer. Optimistic checks
/// catch intervening edits; the host must hold its write lock across a job.
/// Catalog recipe_hash has no public setter in this engine version: rescan or
/// invalidate host caches using committed review IDs after the job.
pub struct SidecarStore {
    paths: BTreeMap<ImageId, PathBuf>,
    machine: String,
}
impl SidecarStore {
    pub fn new(
        images: impl IntoIterator<Item = (ImageId, PathBuf)>,
        machine: &str,
    ) -> EngineResult<Self> {
        if machine.is_empty() {
            return Err(EngineError::invalid("machine", "empty identity"));
        }
        let mut paths = BTreeMap::new();
        let mut destinations = HashSet::new();
        for (id, path) in images {
            let path = path.canonicalize()?;
            if !path.is_file() {
                return Err(EngineError::invalid("image", "expected an image file"));
            }
            let destination = Sidecar::paths(&path).recipe;
            if paths.insert(id, path.clone()).is_some() || !destinations.insert(destination) {
                return Err(EngineError::invalid(
                    "sidecar",
                    "duplicate image or destination",
                ));
            }
            // Sidecar naming is stem-only; reject even unselected RAW/JPEG peers.
            for peer in std::fs::read_dir(path.parent().expect("canonical image parent"))? {
                let peer = peer?.path();
                if peer != path
                    && peer.is_file()
                    && peer.file_stem() == path.file_stem()
                    && peer.extension().and_then(|x| x.to_str()).is_some_and(|e| {
                        matches!(
                            e.to_ascii_lowercase().as_str(),
                            "jpg"
                                | "jpeg"
                                | "png"
                                | "tif"
                                | "tiff"
                                | "dng"
                                | "arw"
                                | "cr2"
                                | "cr3"
                                | "nef"
                                | "raf"
                        )
                    })
                {
                    return Err(EngineError::invalid("sidecar", "same-stem image collision"));
                }
            }
        }
        Ok(Self {
            paths,
            machine: machine.into(),
        })
    }
    fn path(&self, image: ImageId) -> EngineResult<&Path> {
        self.paths
            .get(&image)
            .map(PathBuf::as_path)
            .ok_or_else(|| EngineError::not_found("image", image))
    }
}
impl RecipeStore for SidecarStore {
    fn load(&mut self, image: ImageId) -> EngineResult<Recipe> {
        let recipe = document(self.path(image)?)?.recipe;
        if recipe.image_id.is_some_and(|id| id != image) {
            return Err(EngineError::invalid(
                "image",
                "sidecar belongs to another image",
            ));
        }
        Ok(recipe)
    }
    fn commit(
        &mut self,
        image: ImageId,
        expected: &Recipe,
        next: &Recipe,
        timestamp_ms: i64,
    ) -> EngineResult<()> {
        let path = self.path(image)?;
        let mut current = document(path)?;
        if &current.recipe != expected {
            return Err(EngineError::Conflict {
                message: "recipe changed during prediction".into(),
            });
        }
        if next.image_id != Some(image) {
            return Err(EngineError::invalid("image", "commit identity mismatch"));
        }
        current.recipe = next.clone();
        current.record_write(&self.machine, timestamp_ms)?;
        Sidecar::write_recipe(Sidecar::paths(path).recipe, &current)
    }
}
