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
use std::path::{Path, PathBuf};

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
    fn write(&self, index: &Index, forward: bool) -> EngineResult<()> {
        if let Some(basket) = &self.basket {
            basket.write(forward)
        } else {
            persistence::write_changes(index, &self.changes, forward)
        }
    }
}

/// A fixed review queue. Changes do not remove images from the active query.
pub struct CullSession<'a> {
    index: &'a Index,
    images: Vec<ImageId>,
    position: usize,
    auto_advance: bool,
    undo: Vec<Action>,
    redo: Vec<Action>,
    groups: Vec<Group>,
    preview_errors: Vec<(ImageId, EngineError)>,
    scorer: Box<dyn Scorer>,
    library: Option<PathBuf>,
    basket_target: Option<String>,
}
impl<'a> CullSession<'a> {
    pub fn open(index: &'a Index, source: impl Into<Source>) -> EngineResult<Self> {
        let source = source.into();
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
            scorer: Box::new(LargestFile),
            library: folder.map(|p| p.join("library.json")),
            basket_target: None,
        };
        session.regroup(GroupingOptions::default())?;
        Ok(session)
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
    fn change_current(&mut self, change: impl FnOnce(&mut Selection)) -> EngineResult<()> {
        let id = self.require_current()?;
        let before = persistence::load(self.index, id)?.recipe.selection;
        let mut after = before.clone();
        change(&mut after);
        self.apply(vec![Change { id, before, after }])
    }
    fn apply(&mut self, changes: Vec<Change>) -> EngineResult<()> {
        let changes: Vec<_> = changes
            .into_iter()
            .filter(|c| c.before != c.after)
            .collect();
        if changes.is_empty() {
            return Ok(());
        }
        persistence::write_changes(self.index, &changes, true)?;
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
        self.change_current(|s| s.set_decision(decision))?;
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
        self.change_current(|s| s.set_grade(Some(grade)))
    }
    /// Empty name clears the mark.
    pub fn mark(&mut self, name: impl Into<String>) -> EngineResult<()> {
        let name = name.into();
        self.change_current(|s| {
            s.mark = if name.is_empty() {
                None
            } else {
                Some(Mark::new(name))
            }
        })
    }
    pub fn undo(&mut self) -> EngineResult<bool> {
        let Some(action) = self.undo.last() else {
            return Ok(false);
        };
        action.write(self.index, false)?;
        let action = self.undo.pop().unwrap();
        self.position = action.before_position;
        self.redo.push(action);
        Ok(true)
    }
    pub fn redo(&mut self) -> EngineResult<bool> {
        let Some(action) = self.redo.last() else {
            return Ok(false);
        };
        action.write(self.index, true)?;
        let action = self.redo.pop().unwrap();
        self.position = action.after_position;
        self.undo.push(action);
        Ok(true)
    }
}
