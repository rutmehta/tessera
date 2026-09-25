//! Sidecar-backed, keyboard-oriented culling. No AI signal applies a decision.
mod defects;
mod grouping;
mod library;
mod persistence;
pub use defects::{DefectReason, Direction, Threshold};
use engine_api::{EngineError, EngineResult};
pub use engine_api::{
    id::ImageId,
    recipe::{Decision, Grade, Mark, Selection},
};
pub use grouping::{Group, GroupingOptions, LargestFile, Scorer, dhash, dhash_jpeg};
use index::{Index, Query};
pub use library::{Album, DerivedStatus, Library, Status};
use std::{
    ops::Deref,
    path::{Path, PathBuf},
};

/// A folder (recursive) or an index query, including its explicit pagination.
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
    library: Option<PathBuf>,
    basket_target: Option<String>,
}
/// Session that owns its own index connection.
pub type OwnedCullSession = CullSession<Box<Index>>;

impl<'a> CullSession<&'a Index> {
    pub fn open(index: &'a Index, source: impl Into<Source>) -> EngineResult<Self> {
        Self::open_with(index, source.into())
    }
}
impl OwnedCullSession {
    /// Open a second connection to the same SQLite file for this; WAL lets it
    /// coexist with the host's own connection.
    pub fn open_owned(index: Index, source: impl Into<Source>) -> EngineResult<Self> {
        Self::open_with(Box::new(index), source.into())
    }
}
impl<I: Deref<Target = Index>> CullSession<I> {
    fn open_with(index: I, source: Source) -> EngineResult<Self> {
        let (query, folder) = match source {
            Source::Query(q) => (q, None),
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
            limit: i64::MAX as usize,
            offset: 0,
            ..query.clone()
        };
        // Folder matching uses Path components rather than SQLite glob metacharacters.
        let mut images = Vec::new();
        for id in index.search(&candidates)? {
            let info = index.image_info(id)?;
            if folder.as_ref().is_some_and(|p| !info.path.starts_with(p)) {
                continue;
            }
            // Deleted or moved since indexing: not reviewable, never recreated.
            if !info.path.exists() {
                continue;
            }
            let selection = persistence::load(&index, id)?.recipe.selection;
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
            library: folder.map(|p| p.join("library.json")),
            basket_target: None,
        };
        session.regroup(GroupingOptions::default())?;
        Ok(session)
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

fn mark_from(name: String) -> Option<Mark> {
    if name.is_empty() {
        None
    } else {
        Some(Mark::new(name))
    }
}
