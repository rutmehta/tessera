//! Sidecar-backed, keyboard-oriented culling. No AI signal applies a decision.
mod background;
mod defects;
mod grouping;
mod hash_cache;
mod incremental;
pub mod learning;
mod library;
pub mod people;
mod persistence;
pub use background::PreviewShutdown;
pub use defects::{DefectReason, Direction, Threshold};
use engine_api::{EngineError, EngineResult};
pub use engine_api::{
    id::ImageId,
    recipe::{Decision, Grade, Mark, Selection},
};
pub use grouping::{
    Group, GroupingOptions, GroupingStrategy, LargestFile, Scorer, dhash, dhash_jpeg, preview_hash,
};
pub use hash_cache::HashCachePolicy;
pub use incremental::QueueChange;
use index::{ImageInfo, Index, Query};
pub use library::{Album, DerivedStatus, Library, Status};
use std::{
    collections::{HashMap, HashSet},
    ops::Deref,
    path::{Path, PathBuf},
};

/// A folder (recursive) or an index query, including its explicit pagination.
// Keep the public Source::Query(Query) construction API as Query gains facets.
#[allow(clippy::large_enum_variant)]
pub enum Source {
    Folder(PathBuf),
    Query(Query),
}
impl From<Query> for Source {
    fn from(query: Query) -> Self {
        Self::Query(query)
    }
}
impl From<&Path> for Source {
    fn from(path: &Path) -> Self {
        Self::Folder(path.into())
    }
}
impl From<PathBuf> for Source {
    fn from(path: PathBuf) -> Self {
        Self::Folder(path)
    }
}

#[derive(Clone)]
struct Change {
    id: ImageId,
    before: Selection,
    after: Selection,
}
#[derive(Clone)]
struct Action {
    changes: Vec<Change>,
    before_position: usize,
    after_position: usize,
    basket: Option<library::BasketChange>,
}
impl Action {
    /// Images whose selection or album membership this action changes.
    fn image_ids(&self) -> Vec<ImageId> {
        let mut ids: Vec<_> = self.changes.iter().map(|c| c.id).collect();
        if let Some(basket) = &self.basket {
            ids.extend(basket.image_ids());
        }
        ids
    }
    fn write(&self, index: &Index, forward: bool) -> EngineResult<()> {
        if let Some(basket) = &self.basket {
            basket.write(forward)
        } else {
            persistence::write_changes(index, &self.changes, forward)
        }
    }
}

/// Which images an edit affects: the cursor image, or an explicit batch.
enum Targets<'t> {
    Current,
    Images(&'t [ImageId]),
}

type PreviewProvider =
    std::sync::Arc<dyn Fn(&ImageInfo) -> EngineResult<Option<u64>> + Send + Sync>;

/// A fixed review queue. Changes do not remove images from the active query.
/// `I` is how the session holds its index: borrowed (`&Index`, see `open`) or
/// owned (`Box<Index>`, see `open_owned`) for hosts that cannot keep a borrow
/// alive across calls, such as the UniFFI bridge. The owned form is `Send`.
pub struct CullSession<I> {
    index: I,
    images: Vec<ImageId>,
    position: usize,
    auto_advance: bool,
    undo: Vec<Action>,
    redo: Vec<Action>,
    groups: Vec<Group>,
    preview_errors: Vec<(ImageId, EngineError)>,
    scorer: Option<Box<dyn Scorer>>,
    grouping_strategy: Option<Box<dyn GroupingStrategy>>,
    preview_hash: PreviewProvider,
    previews: background::BackgroundPreviews,
    preview_notify: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
    /// Hash snapshot has changes not yet applied to a custom policy.
    custom_hashes_dirty: bool,
    rebuild: Option<grouping::DefaultRebuild>,
    /// Grouping pair checks performed (a work counter for regression tests).
    pair_checks: u64,
    library: Option<PathBuf>,
    basket_target: Option<String>,
    /// The source, kept so incremental inserts apply the same membership rules.
    folder: Option<PathBuf>,
    query: Query,
    /// Options of the last `regroup`; incremental regrouping reuses them.
    options: GroupingOptions,
    /// Grouping inputs per queue image (incremental regrouping compares against them).
    infos: HashMap<ImageId, ImageInfo>,
    /// dHash of the embedded preview; absent when unreadable or near-duplicates are off.
    hashes: HashMap<ImageId, Option<u64>>,
    /// Queue sort keys (capture time as stored, id), filled on first insert.
    keys: HashMap<ImageId, (Option<String>, String)>,
    /// Catalog change sequence applied so far (see `sync_catalog`).
    change_seq: u64,
    /// Host-declared local assets; this explicit queue never reads original files.
    declared: Option<HashSet<ImageId>>,
}
/// Session that owns its own index connection.
pub type OwnedCullSession = CullSession<Box<Index>>;

impl<'a> CullSession<&'a Index> {
    pub fn open(index: &'a Index, source: impl Into<Source>) -> EngineResult<Self> {
        Self::open_with(index, source.into())
    }
}
impl OwnedCullSession {
    /// Catalog-only, read-only queue for host-declared local assets. The host
    /// validates declarations; membership is a snapshot refreshed by reopening.
    pub fn open_owned_declared(
        index: Index,
        folder: PathBuf,
        ids: HashSet<ImageId>,
    ) -> EngineResult<Self> {
        if !folder.is_absolute()
            || folder
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(EngineError::invalid(
                "folder",
                "expected absolute catalog folder without ..",
            ));
        }
        Self::open_with_policy(
            Box::new(index),
            Source::Folder(folder),
            Some(ids),
            std::sync::Arc::new(preview_hash),
        )
    }
    /// Open a second connection to the same SQLite file for this; WAL lets it
    /// coexist with the host's own connection.
    /// Host rendering policy for displayed-image grouping, including imported proxies.
    pub fn open_owned_with_previews(
        index: Index,
        source: impl Into<Source>,
        preview: impl Fn(&ImageInfo) -> EngineResult<Option<u64>> + Send + Sync + 'static,
    ) -> EngineResult<Self> {
        Self::open_with_policy(
            Box::new(index),
            source.into(),
            None,
            std::sync::Arc::new(preview),
        )
    }
    /// Opt in to persistent hashes using an approved host cache root and stable
    /// provider/pixel-policy identity. Other constructors never persist hashes.
    pub fn open_owned_with_cached_previews(
        index: Index,
        source: impl Into<Source>,
        policy: HashCachePolicy,
        preview: impl Fn(&ImageInfo) -> EngineResult<Option<u64>> + Send + Sync + 'static,
    ) -> EngineResult<Self> {
        Self::open_with_policy(
            Box::new(index),
            source.into(),
            None,
            hash_cache::persistent(Some(policy), std::sync::Arc::new(preview)),
        )
    }
    pub fn open_owned(index: Index, source: impl Into<Source>) -> EngineResult<Self> {
        Self::open_with(Box::new(index), source.into())
    }
}
impl<I: Deref<Target = Index>> CullSession<I> {
    fn open_with(index: I, source: Source) -> EngineResult<Self> {
        Self::open_with_policy(index, source, None, std::sync::Arc::new(preview_hash))
    }
    fn open_with_policy(
        index: I,
        source: Source,
        declared: Option<HashSet<ImageId>>,
        preview_hash: PreviewProvider,
    ) -> EngineResult<Self> {
        // Read first: changes committed while the queue is built are re-applied (idempotently).
        let change_seq = index.change_head()?;
        let (query, folder) = match source {
            Source::Query(q) => (q, None),
            Source::Folder(p) if declared.is_some() => (Query::default(), Some(p)),
            Source::Folder(p) => (
                Query::default(),
                Some(p.canonicalize().map_err(|e| EngineError::io_at(&p, &e))?),
            ),
        };
        // Reconcile sidecars before applying selection filters or pagination.
        // Index::search defaults to 100 rows; a culling queue defaults to all.
        let candidates = Query {
            decision: None,
            grade: None,
            mark: None,
            // Boolean saved searches can contain selection terms under OR/NOT.
            // Reconcile first; filtering stale selection would lose candidates.
            predicate: None,
            limit: i64::MAX as usize,
            offset: 0,
            ..query.clone()
        };
        let images = admit(
            &index,
            &query,
            folder.as_deref(),
            index.search(&candidates)?,
            declared.as_ref(),
        )?;
        let images = images
            .into_iter()
            .skip(query.offset)
            .take(if query.limit == 0 {
                usize::MAX
            } else {
                query.limit
            })
            .collect();
        let mut session = Self {
            index,
            images,
            position: 0,
            auto_advance: true,
            undo: Vec::new(),
            redo: Vec::new(),
            groups: Vec::new(),
            preview_errors: Vec::new(),
            scorer: None,
            grouping_strategy: None,
            preview_hash,
            previews: Default::default(),
            preview_notify: None,
            custom_hashes_dirty: false,
            rebuild: None,
            pair_checks: 0,
            library: if declared.is_none() {
                folder.as_ref().map(|p| p.join("library.json"))
            } else {
                None
            },
            basket_target: None,
            folder,
            query,
            options: GroupingOptions::default(),
            infos: HashMap::new(),
            hashes: HashMap::new(),
            keys: HashMap::new(),
            change_seq,
            declared,
        };
        session.regroup(GroupingOptions::default())?;
        Ok(session)
    }
    /// Whether this is a catalog-only declaration snapshot.
    pub fn is_declared_read_only(&self) -> bool {
        self.declared.is_some()
    }
    pub fn require_writable(&self) -> EngineResult<()> {
        if self.is_declared_read_only() {
            return Err(EngineError::invalid(
                "session",
                "Smart Preview library session is read-only",
            ));
        }
        Ok(())
    }
    /// The index this session reads and writes through.
    pub fn index(&self) -> &Index {
        &self.index
    }
    pub fn images(&self) -> &[ImageId] {
        &self.images
    }
    pub fn position(&self) -> Option<usize> {
        self.current().map(|_| self.position)
    }
    pub fn current(&self) -> Option<ImageId> {
        self.images.get(self.position).copied()
    }
    pub fn set_position(&mut self, position: usize) -> EngineResult<()> {
        if position >= self.images.len() {
            return Err(EngineError::invalid("position", "outside review queue"));
        }
        self.position = position;
        Ok(())
    }
    /// Moves the cursor to `id`, which must be in the review queue.
    pub fn set_current(&mut self, id: ImageId) -> EngineResult<()> {
        let position = self
            .images
            .iter()
            .position(|image| *image == id)
            .ok_or_else(|| EngineError::invalid("image", "not in review queue"))?;
        self.position = position;
        Ok(())
    }
    /// Authoritative (sidecar-first) selection for any indexed image.
    pub fn selection(&self, id: ImageId) -> EngineResult<Selection> {
        if self.declared.is_some() {
            return Ok(self.index.selection(id)?.unwrap_or_default());
        }
        Ok(persistence::load(&self.index, id)?.recipe.selection)
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    /// Images the next `undo` would touch (selection or album membership).
    pub fn undo_images(&self) -> Vec<ImageId> {
        self.undo.last().map(Action::image_ids).unwrap_or_default()
    }
    /// Images the next `redo` would touch.
    pub fn redo_images(&self) -> Vec<ImageId> {
        self.redo.last().map(Action::image_ids).unwrap_or_default()
    }
    pub fn set_auto_advance(&mut self, enabled: bool) {
        self.auto_advance = enabled;
    }
    pub fn auto_advance(&self) -> bool {
        self.auto_advance
    }
    fn require_current(&self) -> EngineResult<ImageId> {
        self.current()
            .ok_or_else(|| EngineError::invalid("session", "empty review queue"))
    }
    fn change(&mut self, targets: Targets, change: impl Fn(&mut Selection)) -> EngineResult<()> {
        self.require_writable()?;
        let ids = match targets {
            Targets::Current => vec![self.require_current()?],
            Targets::Images(ids) => {
                let mut unique = Vec::with_capacity(ids.len());
                for id in ids {
                    if !self.images.contains(id) {
                        return Err(EngineError::invalid("image", "not in review queue"));
                    }
                    if !unique.contains(id) {
                        unique.push(*id);
                    }
                }
                unique
            }
        };
        let mut changes = Vec::with_capacity(ids.len());
        for id in ids {
            let before = persistence::load(&self.index, id)?.recipe.selection;
            let mut after = before.clone();
            change(&mut after);
            changes.push(Change { id, before, after });
        }
        self.apply(changes)
    }
    fn apply(&mut self, changes: Vec<Change>) -> EngineResult<()> {
        let changes: Vec<_> = changes
            .into_iter()
            .filter(|c| c.before != c.after)
            .collect();
        if changes.is_empty() {
            return Ok(());
        }
        persistence::write_changes(&self.index, &changes, true)?;
        self.undo.push(Action {
            changes,
            before_position: self.position,
            after_position: self.position,
            basket: None,
        });
        self.redo.clear();
        Ok(())
    }
    /// Only decisions auto-advance; grading and marks stay on the current image.
    pub fn decide(&mut self, decision: Decision) -> EngineResult<()> {
        let history_len = self.undo.len();
        self.change(Targets::Current, |s| s.set_decision(decision))?;
        if self.auto_advance && self.position + 1 < self.images.len() {
            self.position += 1;
        }
        if self.undo.len() > history_len {
            self.undo.last_mut().unwrap().after_position = self.position;
        }
        Ok(())
    }
    pub fn grade(&mut self, grade: u8) -> EngineResult<()> {
        let grade = Grade::try_from(grade)?;
        self.change(Targets::Current, |s| s.set_grade(Some(grade)))
    }
    /// Empty name clears the mark.
    pub fn mark(&mut self, name: impl Into<String>) -> EngineResult<()> {
        let mark = mark_from(name.into());
        self.change(Targets::Current, |s| s.mark = mark.clone())
    }
    /// One undo step for a batch decision (e.g. an applied defect sweep or a
    /// multi-selection). Never moves the cursor. Unknown images are rejected.
    pub fn decide_images(&mut self, ids: &[ImageId], decision: Decision) -> EngineResult<()> {
        self.change(Targets::Images(ids), |s| s.set_decision(decision))
    }
    /// One undo step with a decision per image, e.g. "choose this" in compare
    /// (keep one, reject the other). Never moves the cursor.
    pub fn decide_each(&mut self, decisions: &[(ImageId, Decision)]) -> EngineResult<()> {
        self.require_writable()?;
        let mut changes: Vec<Change> = Vec::with_capacity(decisions.len());
        for (id, decision) in decisions {
            if !self.images.contains(id) {
                return Err(EngineError::invalid("image", "not in review queue"));
            }
            if changes.iter().any(|c| c.id == *id) {
                return Err(EngineError::invalid("image", "listed twice"));
            }
            let before = persistence::load(&self.index, *id)?.recipe.selection;
            let mut after = before.clone();
            after.set_decision(*decision);
            changes.push(Change {
                id: *id,
                before,
                after,
            });
        }
        self.apply(changes)
    }
    /// One undo step; grades imply Keep. Never moves the cursor.
    pub fn grade_images(&mut self, ids: &[ImageId], grade: u8) -> EngineResult<()> {
        let grade = Grade::try_from(grade)?;
        self.change(Targets::Images(ids), |s| s.set_grade(Some(grade)))
    }
    /// One undo step; an empty name clears the mark. Never moves the cursor.
    pub fn mark_images(&mut self, ids: &[ImageId], name: impl Into<String>) -> EngineResult<()> {
        let mark = mark_from(name.into());
        self.change(Targets::Images(ids), |s| s.mark = mark.clone())
    }
    pub fn undo(&mut self) -> EngineResult<bool> {
        self.require_writable()?;
        let Some(action) = self.undo.last() else {
            return Ok(false);
        };
        action.write(&self.index, false)?;
        let action = self.undo.pop().unwrap();
        self.position = action.before_position;
        self.redo.push(action);
        Ok(true)
    }
    pub fn redo(&mut self) -> EngineResult<bool> {
        self.require_writable()?;
        let Some(action) = self.redo.last() else {
            return Ok(false);
        };
        action.write(&self.index, true)?;
        let action = self.redo.pop().unwrap();
        self.position = action.after_position;
        self.undo.push(action);
        Ok(true)
    }
}

/// The reviewable subset of `ids` for a source, in the given order. Reconciles
/// each candidate's selection from its sidecars before applying selection filters.
fn admit(
    index: &Index,
    query: &Query,
    folder: Option<&Path>,
    ids: Vec<ImageId>,
    declared: Option<&HashSet<ImageId>>,
) -> EngineResult<Vec<ImageId>> {
    // Folder matching uses Path components rather than SQLite glob metacharacters.
    let imported = folder
        .and_then(|p| Library::read(p.join("library.json")).ok())
        .and_then(|library| library.unknown.get("lightroom_proxy_members").cloned());
    let mut images = Vec::new();
    for id in ids {
        let info = index.image_info(id)?;
        let external_member = imported
            .as_ref()
            .is_some_and(|v| v.get(id.to_string()).is_some());
        if folder.is_some_and(|p| !info.path.starts_with(p)) && !external_member {
            continue;
        }
        if let Some(declared) = declared {
            if declared.contains(&id) {
                images.push(id);
            }
            continue;
        }
        // Deleted or moved since indexing: not reviewable, never recreated.
        if !info.path.exists() {
            continue;
        }
        let selection = persistence::load(index, id)?.recipe.selection;
        index.set_selection(id, &selection)?;
        if query.decision.is_some_and(|d| d != selection.decision)
            || query.grade.is_some_and(|g| Some(g) != selection.grade)
            || query
                .mark
                .as_ref()
                .is_some_and(|m| selection.mark.as_ref().map(|mark| &mark.0) != Some(m))
        {
            continue;
        }
        images.push(id);
    }
    if query.predicate.is_some() {
        let matching: std::collections::HashSet<_> = index
            .search(&Query {
                predicate: query.predicate.clone(),
                limit: i64::MAX as usize,
                ..Default::default()
            })?
            .into_iter()
            .collect();
        images.retain(|id| matching.contains(id));
    }
    Ok(images)
}

fn mark_from(name: String) -> Option<Mark> {
    if name.is_empty() {
        None
    } else {
        Some(Mark::new(name))
    }
}
