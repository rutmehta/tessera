//! `CullSession` over UniFFI: a fixed review queue with groups, a single global
//! undo stack, the basket and a review-only defect sweep. Only ids, selections
//! and small records cross the bridge; pixels never do.
use crate::{Decision, Engine, ImageQuery, Result, Selection, failure, parse_id};
use cull::{ImageId, Library, OwnedCullSession as Core};
use rusqlite::{Connection, OpenFlags};
use std::{
    path::Path,
    sync::{Arc, Mutex, MutexGuard},
};

/// One image of the review queue, in queue order.
#[derive(Clone, Debug, uniffi::Record)]
pub struct SessionImage {
    pub id: String,
    pub path: String,
    /// As stored by the index (see `ImageSummary::capture_time`).
    pub capture_time: Option<String>,
    pub orientation: u16,
    pub selection: Selection,
    pub in_basket: bool,
    /// Index into `CullSession::groups`.
    pub group: u32,
}

/// Burst / near-duplicate group. Members follow queue order.
#[derive(Clone, Debug, uniffi::Record)]
pub struct CullGroup {
    pub images: Vec<String>,
    /// Suggested best frame (scorer's pick). Suggestion only; never applied implicitly.
    pub best: String,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ImageState {
    pub image_id: String,
    pub selection: Selection,
    /// Member of the current basket target album.
    pub in_basket: bool,
}

/// Result of a mutation, undo or redo: every image whose selection or basket
/// membership may have changed, plus the cursor afterwards.
#[derive(Clone, Debug, uniffi::Record)]
pub struct CullUpdate {
    pub changed: Vec<ImageState>,
    /// library.json may have changed; refetch `albums()` for counts.
    pub albums_changed: bool,
    pub current: Option<String>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct GroupDecision {
    pub best: String,
    pub rejected: u32,
    pub update: CullUpdate,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ImageDecision {
    pub image_id: String,
    pub decision: Decision,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StatusPhase {
    Unedited,
    Edited,
    Exported,
    Published,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ImageStatus {
    pub image_id: String,
    pub phase: StatusPhase,
    pub in_album: Vec<String>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct AlbumInfo {
    pub name: String,
    pub images: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ThresholdDirection {
    /// Defect when the signal is strictly below the threshold (e.g. focus).
    Below,
    /// Defect when the signal is strictly above the threshold (e.g. closed eyes).
    Above,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct DefectThreshold {
    pub signal: String,
    pub value: f64,
    pub direction: ThresholdDirection,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct DefectReason {
    pub signal: String,
    pub value: f64,
    pub threshold: f64,
    pub direction: ThresholdDirection,
    pub model: String,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct DefectCandidate {
    pub image_id: String,
    pub reasons: Vec<DefectReason>,
}

pub(crate) struct Inner {
    pub(crate) core: Core,
    reader: Connection,
    pub(crate) assist: crate::assist::AssistState,
}

/// Owns its own index connection (WAL) so the engine's catalog lock is never
/// held across a culling pass. Calls are synchronous; dispatch off the main
/// thread for large queues.
#[derive(uniffi::Object)]
pub struct CullSession {
    inner: Mutex<Inner>,
}

fn ids(values: &[String]) -> Result<Vec<ImageId>> {
    values.iter().map(|id| parse_id(id)).collect()
}

impl Engine {
    fn open_session(&self, source: cull::Source) -> Result<Arc<CullSession>> {
        let folder = match &source {
            cull::Source::Folder(path) => Some(path.canonicalize()?),
            cull::Source::Query(_) => None,
        };
        let assist = crate::assist::AssistState::new(
            self.support_dir()?.to_path_buf(),
            crate::assist::library_key(folder.as_deref()),
        );
        let index = index::Index::open(&self.db)?;
        let reader = Connection::open_with_flags(&self.db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let mut core = Core::open_owned(index, source)?;
        // The host owns cursor movement so it can follow its display order.
        core.set_auto_advance(false);
        Ok(Arc::new(CullSession {
            inner: Mutex::new(Inner {
                core,
                reader,
                assist,
            }),
        }))
    }
}

#[uniffi::export]
impl Engine {
    /// Recursive folder queue over already-indexed images (call `index_folder`
    /// first). The library defaults to `<folder>/library.json`. Auto-advance is
    /// off; hosts move the cursor with `set_current`/navigation.
    pub fn open_cull_session(&self, folder: String) -> Result<Arc<CullSession>> {
        self.open_session(Path::new(&folder).to_path_buf().into())
    }
    /// Filtered queue. Zero limit means all matches. Call `set_library` before
    /// basket operations.
    pub fn open_cull_session_for_query(&self, query: ImageQuery) -> Result<Arc<CullSession>> {
        self.open_session(
            index::Query {
                folder: query.folder,
                text: query.text,
                decision: query.decision.map(Into::into),
                limit: query.limit as usize,
                offset: query.offset as usize,
                ..Default::default()
            }
            .into(),
        )
    }
    /// Stores the latest value of an AI signal (e.g. "focus", "closed_eyes").
    /// Producers are ML jobs; the app uses it only for synthetic test scores.
    pub fn set_score(
        &self,
        image_id: String,
        signal: String,
        value: f64,
        model: String,
    ) -> Result<()> {
        let id = parse_id(&image_id)?;
        self.lock()?.index.set_score(
            id,
            &index::Score {
                signal,
                value,
                model,
            },
        )?;
        Ok(())
    }
}

impl CullSession {
    pub(crate) fn lock(&self) -> Result<MutexGuard<'_, Inner>> {
        self.inner.lock().map_err(failure)
    }
}

impl Inner {
    fn basket_members(&self) -> Result<Vec<ImageId>> {
        let Some(target) = self.core.basket_target() else {
            return Ok(Vec::new());
        };
        Ok(self
            .core
            .library()?
            .albums
            .get(target)
            .map(|a| a.images.clone())
            .unwrap_or_default())
    }
    /// Every mutation reports through here, which also retires a stale review plan.
    pub(crate) fn update(
        &mut self,
        mut changed: Vec<ImageId>,
        albums_changed: bool,
    ) -> Result<CullUpdate> {
        self.assist.invalidate();
        changed.dedup();
        let basket = self.basket_members()?;
        let mut seen = Vec::with_capacity(changed.len());
        let mut states = Vec::with_capacity(changed.len());
        for id in changed {
            if seen.contains(&id) {
                continue;
            }
            seen.push(id);
            states.push(ImageState {
                image_id: id.to_string(),
                selection: self.core.selection(id)?.into(),
                in_basket: basket.contains(&id),
            });
        }
        Ok(CullUpdate {
            changed: states,
            albums_changed,
            current: self.core.current().map(|id| id.to_string()),
        })
    }
    fn current_update(&mut self) -> Result<CullUpdate> {
        self.update(self.core.current().into_iter().collect(), false)
    }
    fn current(&self) -> Option<String> {
        self.core.current().map(|id| id.to_string())
    }
}

#[uniffi::export]
impl CullSession {
    pub fn images(&self) -> Result<Vec<SessionImage>> {
        let s = self.lock()?;
        let basket = s.basket_members()?;
        let mut group_of = std::collections::HashMap::new();
        for (n, group) in s.core.groups().iter().enumerate() {
            for id in &group.images {
                group_of.insert(*id, n as u32);
            }
        }
        let mut stmt = s.reader.prepare_cached(
            "SELECT f.path,i.capture_time,COALESCE((SELECT value FROM metadata WHERE image_id=i.id AND key='orientation'),'1') FROM image i JOIN file f ON f.id=i.file_id WHERE i.id=?",
        )?;
        let mut out = Vec::with_capacity(s.core.images().len());
        for id in s.core.images() {
            let key = id.to_string();
            let (path, capture_time, orientation) = stmt.query_row([&key], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?;
            out.push(SessionImage {
                id: key,
                path,
                capture_time,
                orientation: orientation.parse().unwrap_or(1),
                // Reconciled from sidecars when the session opened.
                selection: s.core.index().selection(*id)?.unwrap_or_default().into(),
                in_basket: basket.contains(id),
                group: group_of.get(id).copied().unwrap_or(0),
            });
        }
        Ok(out)
    }
    pub fn groups(&self) -> Result<Vec<CullGroup>> {
        let s = self.lock()?;
        (0..s.core.groups().len())
            .map(|n| {
                Ok(CullGroup {
                    images: s.core.groups()[n]
                        .images
                        .iter()
                        .map(ToString::to_string)
                        .collect(),
                    best: s.core.best_in_group(n)?.to_string(),
                })
            })
            .collect()
    }
    /// Images whose embedded preview could not be read for near-duplicate
    /// grouping, with the reason. They remain reviewable.
    pub fn preview_errors(&self) -> Result<Vec<String>> {
        Ok(self
            .lock()?
            .core
            .preview_errors()
            .iter()
            .map(|(id, e)| format!("{id}: {e}"))
            .collect())
    }
    pub fn selection(&self, image_id: String) -> Result<Selection> {
        Ok(self.lock()?.core.selection(parse_id(&image_id)?)?.into())
    }

    // Cursor and navigation. Navigation stops at boundaries and returns the
    // (possibly unchanged) current image.
    pub fn position(&self) -> Result<Option<u32>> {
        Ok(self.lock()?.core.position().map(|p| p as u32))
    }
    pub fn current(&self) -> Result<Option<String>> {
        Ok(self.lock()?.current())
    }
    pub fn set_position(&self, position: u32) -> Result<()> {
        Ok(self.lock()?.core.set_position(position as usize)?)
    }
    pub fn set_current(&self, image_id: String) -> Result<()> {
        Ok(self.lock()?.core.set_current(parse_id(&image_id)?)?)
    }
    pub fn set_auto_advance(&self, enabled: bool) -> Result<()> {
        self.lock()?.core.set_auto_advance(enabled);
        Ok(())
    }
    pub fn auto_advance(&self) -> Result<bool> {
        Ok(self.lock()?.core.auto_advance())
    }
    pub fn current_group(&self) -> Result<Option<u32>> {
        Ok(self.lock()?.core.current_group().map(|g| g as u32))
    }
    pub fn next_group(&self) -> Result<Option<String>> {
        let mut s = self.lock()?;
        s.core.next_group();
        Ok(s.current())
    }
    pub fn prev_group(&self) -> Result<Option<String>> {
        let mut s = self.lock()?;
        s.core.prev_group();
        Ok(s.current())
    }
    pub fn next_in_group(&self) -> Result<Option<String>> {
        let mut s = self.lock()?;
        s.core.next_in_group();
        Ok(s.current())
    }
    pub fn prev_in_group(&self) -> Result<Option<String>> {
        let mut s = self.lock()?;
        s.core.prev_in_group();
        Ok(s.current())
    }
    pub fn best_in_group(&self, group: u32) -> Result<String> {
        Ok(self.lock()?.core.best_in_group(group as usize)?.to_string())
    }

    // Decisions on the current image. Writes recipe + XMP before returning.
    /// With assistance on (`set_assist_mode`), a Keep/Reject also teaches the
    /// library's learner.
    pub fn decide(&self, decision: Decision) -> Result<CullUpdate> {
        let mut s = self.lock()?;
        let id = s.core.current();
        s.decide_learning(decision.into())?;
        s.update(id.into_iter().collect(), false)
    }
    pub fn grade(&self, grade: u8) -> Result<CullUpdate> {
        let mut s = self.lock()?;
        s.core.grade(grade)?;
        s.current_update()
    }
    /// Empty name clears the mark.
    pub fn mark(&self, name: String) -> Result<CullUpdate> {
        let mut s = self.lock()?;
        s.core.mark(name)?;
        s.current_update()
    }

    // Batches: one undo step each, cursor unchanged.
    pub fn decide_images(&self, image_ids: Vec<String>, decision: Decision) -> Result<CullUpdate> {
        let ids = ids(&image_ids)?;
        let mut s = self.lock()?;
        s.core.decide_images(&ids, decision.into())?;
        s.update(ids, false)
    }
    /// A different decision per image as one undo step ("choose this" in
    /// compare keeps one frame and rejects the other).
    pub fn decide_each(&self, decisions: Vec<ImageDecision>) -> Result<CullUpdate> {
        let pairs = decisions
            .iter()
            .map(|d| Ok((parse_id(&d.image_id)?, d.decision.into())))
            .collect::<Result<Vec<_>>>()?;
        let mut s = self.lock()?;
        s.core.decide_each(&pairs)?;
        s.update(pairs.iter().map(|(id, _)| *id).collect(), false)
    }
    pub fn grade_images(&self, image_ids: Vec<String>, grade: u8) -> Result<CullUpdate> {
        let ids = ids(&image_ids)?;
        let mut s = self.lock()?;
        s.core.grade_images(&ids, grade)?;
        s.update(ids, false)
    }
    pub fn mark_images(&self, image_ids: Vec<String>, name: String) -> Result<CullUpdate> {
        let ids = ids(&image_ids)?;
        let mut s = self.lock()?;
        s.core.mark_images(&ids, name)?;
        s.update(ids, false)
    }
    /// Keeps the group's suggested best and rejects the rest as one undo step.
    pub fn keep_best_reject_rest(&self, group: u32) -> Result<GroupDecision> {
        let mut s = self.lock()?;
        let members = s
            .core
            .groups()
            .get(group as usize)
            .ok_or_else(|| failure("group outside session"))?
            .images
            .clone();
        let best = s.core.keep_best_reject_rest(group as usize)?;
        Ok(GroupDecision {
            best: best.to_string(),
            rejected: members.len().saturating_sub(1) as u32,
            update: s.update(members, false)?,
        })
    }

    /// Returns None when there is nothing to undo.
    pub fn undo(&self) -> Result<Option<CullUpdate>> {
        let mut s = self.lock()?;
        let touched = s.core.undo_images();
        if !s.core.undo()? {
            return Ok(None);
        }
        s.update(touched, true).map(Some)
    }
    pub fn redo(&self) -> Result<Option<CullUpdate>> {
        let mut s = self.lock()?;
        let touched = s.core.redo_images();
        if !s.core.redo()? {
            return Ok(None);
        }
        s.update(touched, true).map(Some)
    }
    pub fn can_undo(&self) -> Result<bool> {
        Ok(self.lock()?.core.can_undo())
    }
    pub fn can_redo(&self) -> Result<bool> {
        Ok(self.lock()?.core.can_redo())
    }

    // Library, basket and albums (library.json; never per-image sidecars).
    pub fn set_library(&self, path: String) -> Result<()> {
        Ok(self.lock()?.core.set_library(path)?)
    }
    pub fn library_path(&self) -> Result<Option<String>> {
        Ok(self
            .lock()?
            .core
            .library_path()
            .map(|p| p.to_string_lossy().into_owned()))
    }
    pub fn basket_target(&self) -> Result<Option<String>> {
        Ok(self.lock()?.core.basket_target().map(str::to_owned))
    }
    /// Membership flags in later updates refer to the new target.
    pub fn set_basket_target(&self, name: String) -> Result<()> {
        Ok(self.lock()?.core.set_basket_target(name)?)
    }
    pub fn toggle_basket(&self) -> Result<CullUpdate> {
        let mut s = self.lock()?;
        s.core.toggle_basket()?;
        let current = s.core.current().into_iter().collect();
        s.update(current, true)
    }
    pub fn set_basket(&self, image_ids: Vec<String>, add: bool) -> Result<CullUpdate> {
        let ids = ids(&image_ids)?;
        let mut s = self.lock()?;
        s.core.set_basket(&ids, add)?;
        s.update(ids, true)
    }
    /// Safe delete inside an album: membership only, files untouched. Undoable.
    pub fn remove_from_album(&self, album: String, image_ids: Vec<String>) -> Result<CullUpdate> {
        let ids = ids(&image_ids)?;
        let mut s = self.lock()?;
        s.core.remove_from_album(&album, &ids)?;
        s.update(ids, true)
    }
    pub fn albums(&self) -> Result<Vec<AlbumInfo>> {
        let library: Library = self.lock()?.core.library()?;
        Ok(library
            .albums
            .into_iter()
            .map(|(name, album)| AlbumInfo {
                name,
                images: album.images.iter().map(ToString::to_string).collect(),
            })
            .collect())
    }

    /// Derived, read-only status per image (library.json read once).
    pub fn derived_statuses(&self, image_ids: Vec<String>) -> Result<Vec<ImageStatus>> {
        let ids = ids(&image_ids)?;
        let s = self.lock()?;
        Ok(s.core
            .derived_statuses(&ids)?
            .into_iter()
            .zip(image_ids)
            .map(|(status, image_id)| ImageStatus {
                image_id,
                phase: match status.status {
                    cull::Status::Unedited => StatusPhase::Unedited,
                    cull::Status::Edited => StatusPhase::Edited,
                    cull::Status::Exported => StatusPhase::Exported,
                    cull::Status::Published => StatusPhase::Published,
                },
                in_album: status.in_album,
            })
            .collect())
    }

    /// Review list only: applies nothing and records no history. Apply the
    /// user's confirmed subset with `decide_images(.., Reject)`.
    pub fn defect_sweep(&self, thresholds: Vec<DefectThreshold>) -> Result<Vec<DefectCandidate>> {
        let thresholds: Vec<_> = thresholds
            .into_iter()
            .map(|t| match t.direction {
                ThresholdDirection::Below => cull::Threshold::below(t.signal, t.value),
                ThresholdDirection::Above => cull::Threshold::above(t.signal, t.value),
            })
            .collect();
        let found = self.lock()?.core.defect_sweep(&thresholds)?;
        Ok(found
            .into_iter()
            .map(|(id, reasons)| DefectCandidate {
                image_id: id.to_string(),
                reasons: reasons
                    .into_iter()
                    .map(|r| DefectReason {
                        signal: r.signal,
                        value: r.value,
                        threshold: r.threshold,
                        direction: match r.direction {
                            cull::Direction::Below => ThresholdDirection::Below,
                            cull::Direction::Above => ThresholdDirection::Above,
                        },
                        model: r.model,
                    })
                    .collect(),
            })
            .collect())
    }
}
