//! Incremental queue updates (M2-28): new, removed and changed catalog images
//! reach an open session without reopening it, so the undo/redo history, the
//! cursor and unaffected groups survive a tethered frame or an import.
use crate::{CullSession, Group, ImageId, admit, grouping};
use engine_api::{EngineError, EngineResult};
use index::{ChangeBatch, ChangeFields, ChangeKind, Index};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    ops::Deref,
};

/// What an incremental update did to the review queue.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueueChange {
    /// New queue members, in queue order.
    pub inserted: Vec<ImageId>,
    /// Former members (deleted, moved out of the source, or asked for).
    pub removed: Vec<ImageId>,
    /// Remaining members whose catalog rows changed, with what changed.
    pub updated: Vec<(ImageId, ChangeFields)>,
    /// Group membership or order changed.
    pub regrouped: bool,
    /// The batch could not be applied (log trimmed or catalog replaced): reopen.
    pub reset: bool,
    /// Catalog change sequence applied (see `CullSession::change_sequence`).
    pub sequence: u64,
}
impl QueueChange {
    pub fn is_empty(&self) -> bool {
        self.inserted.is_empty()
            && self.removed.is_empty()
            && self.updated.is_empty()
            && !self.regrouped
            && !self.reset
    }
}

impl<I: Deref<Target = Index>> CullSession<I> {
    /// Catalog change sequence this queue reflects.
    pub fn change_sequence(&self) -> u64 {
        self.change_seq
    }

    /// Applies every catalog change committed since the last sync (or open).
    pub fn sync_catalog(&mut self) -> EngineResult<QueueChange> {
        let batch = self.index.changes_since(self.change_seq)?;
        self.apply_changes(&batch)
    }

    /// Applies a batch from `Index::changes_since`. `batch.from` must not be
    /// newer than this session's sequence (re-applying is harmless). New images
    /// join when the source admits them (pagination windows are not re-applied);
    /// selection changes never remove members ("changes do not remove images
    /// from the active query").
    pub fn apply_changes(&mut self, batch: &ChangeBatch) -> EngineResult<QueueChange> {
        if batch.from > self.change_seq {
            return Err(EngineError::invalid(
                "changes",
                "batch starts after this session's sequence",
            ));
        }
        let mut out = QueueChange {
            sequence: self.change_seq.max(batch.to),
            ..Default::default()
        };
        if batch.reset {
            out.reset = true;
            return Ok(out);
        }
        let members: HashSet<ImageId> = self.images.iter().copied().collect();
        let mut remove = Vec::new();
        let mut insert = Vec::new();
        let mut regroup = Vec::new();
        let mut updated = Vec::new();
        for change in &batch.changes {
            let member = members.contains(&change.id);
            match change.kind {
                ChangeKind::Removed if member => remove.push(change.id),
                ChangeKind::Removed => {}
                ChangeKind::Added if !member => insert.push(change.id),
                // Already a member (the batch overlaps the session's snapshot).
                ChangeKind::Added => updated.push((change.id, ChangeFields::ALL)),
                ChangeKind::Updated(fields) if member => {
                    if fields.contains(ChangeFields::FILE) && !self.still_admitted(change.id)? {
                        remove.push(change.id);
                        continue;
                    }
                    if fields.intersects(ChangeFields::FILE | ChangeFields::CAPTURE_TIME) {
                        regroup.push(change.id);
                    }
                    updated.push((change.id, fields));
                }
                // Moved into a folder source.
                ChangeKind::Updated(fields) if fields.contains(ChangeFields::FILE) => {
                    insert.push(change.id)
                }
                ChangeKind::Updated(_) => {}
            }
        }
        out.removed = self.remove_images(&remove)?;
        out.inserted = self.insert_images(&insert)?;
        for id in &regroup {
            self.refresh_grouping_inputs(*id)?;
            self.keys.remove(id);
        }
        out.regrouped = !out.removed.is_empty() || !out.inserted.is_empty();
        out.regrouped |= self.regroup_images(&regroup)?;
        out.updated = updated;
        self.change_seq = out.sequence;
        Ok(out)
    }

    /// Adds catalog images the source admits, at their queue-order position
    /// (capture time, then id), and groups them with their bursts and near
    /// duplicates. Returns the images added. Cursor and history keep pointing
    /// at the same images.
    pub fn insert_images(&mut self, ids: &[ImageId]) -> EngineResult<Vec<ImageId>> {
        let present: HashSet<ImageId> = self.images.iter().copied().collect();
        let mut fresh = Vec::new();
        for id in ids {
            if !present.contains(id) && !fresh.contains(id) {
                fresh.push(*id);
            }
        }
        if fresh.is_empty() {
            return Ok(Vec::new());
        }
        // A query source is checked against its own search; a folder by path.
        if self.folder.is_none() {
            let matching: HashSet<ImageId> = self
                .index
                .search(&index::Query {
                    decision: None,
                    grade: None,
                    mark: None,
                    predicate: None,
                    limit: i64::MAX as usize,
                    offset: 0,
                    ..self.query.clone()
                })?
                .into_iter()
                .collect();
            fresh.retain(|id| matching.contains(id));
        }
        // Unknown ids (already pruned again) are skipped rather than failing the batch.
        fresh.retain(|id| self.index.image_info(*id).is_ok());
        let fresh = admit(&self.index, &self.query, self.folder.as_deref(), fresh)?;
        if fresh.is_empty() {
            return Ok(Vec::new());
        }
        self.fill_keys()?;
        let mut new_keys = self.index.order_keys(&fresh)?;
        let mut order: Vec<usize> = (0..fresh.len()).collect();
        order.sort_by(|a, b| new_keys[*a].cmp(&new_keys[*b]));
        let mut images = self.images.clone();
        for n in order {
            let key = std::mem::take(&mut new_keys[n]);
            // First member that sorts after it (the queue is in key order unless reordered).
            let at = images
                .iter()
                .position(|id| self.keys.get(id).is_some_and(|k| *k > key))
                .unwrap_or(images.len());
            images.insert(at, fresh[n]);
            self.keys.insert(fresh[n], key);
        }
        self.replace_queue(images);
        for id in &fresh {
            self.refresh_grouping_inputs(*id)?;
            self.groups.push(Group { images: vec![*id] });
        }
        self.regroup_images(&fresh)?;
        Ok(self
            .images
            .iter()
            .copied()
            .filter(|id| fresh.contains(id))
            .collect())
    }

    /// Drops images from the queue (and from undo/redo steps, which could not
    /// write their sidecars any more). Groups that lose members are recomputed,
    /// since a burst can split. Returns the images that were members.
    pub fn remove_images(&mut self, ids: &[ImageId]) -> EngineResult<Vec<ImageId>> {
        let doomed: HashSet<ImageId> = ids
            .iter()
            .copied()
            .filter(|id| self.images.contains(id))
            .collect();
        if doomed.is_empty() {
            return Ok(Vec::new());
        }
        let removed: Vec<ImageId> = self
            .images
            .iter()
            .copied()
            .filter(|id| doomed.contains(id))
            .collect();
        for action in self.undo.iter_mut().chain(self.redo.iter_mut()) {
            action.changes.retain(|c| !doomed.contains(&c.id));
        }
        self.undo
            .retain(|a| !a.changes.is_empty() || a.basket.is_some());
        self.redo
            .retain(|a| !a.changes.is_empty() || a.basket.is_some());
        let images = self
            .images
            .iter()
            .copied()
            .filter(|id| !doomed.contains(id))
            .collect();
        self.replace_queue(images);
        let mut damaged = Vec::new();
        for group in &mut self.groups {
            let before = group.images.len();
            group.images.retain(|id| !doomed.contains(id));
            if group.images.len() != before {
                damaged.extend(group.images.iter().copied());
            }
        }
        self.groups.retain(|g| !g.images.is_empty());
        for id in &doomed {
            self.infos.remove(id);
            self.hashes.remove(id);
            self.keys.remove(id);
        }
        self.preview_errors.retain(|(id, _)| !doomed.contains(id));
        self.regroup_images(&damaged)?;
        Ok(removed)
    }

    /// Recomputes grouping around `ids` only: their groups, the groups of every
    /// image they relate to, and nothing else. Returns whether groups changed.
    pub fn regroup_images(&mut self, ids: &[ImageId]) -> EngineResult<bool> {
        let seeds: Vec<ImageId> = ids
            .iter()
            .copied()
            .filter(|id| self.infos.contains_key(id))
            .collect();
        if seeds.is_empty() {
            return Ok(false);
        }
        let position: HashMap<ImageId, usize> = self
            .images
            .iter()
            .enumerate()
            .map(|(n, id)| (*id, n))
            .collect();
        let mut group_of: HashMap<ImageId, usize> = HashMap::new();
        for (g, group) in self.groups.iter().enumerate() {
            for id in &group.images {
                group_of.insert(*id, g);
            }
        }
        let ordered = |a: ImageId, b: ImageId| {
            if position.get(&a) <= position.get(&b) {
                (a, b)
            } else {
                (b, a)
            }
        };
        let mut involved = BTreeSet::new();
        for seed in &seeds {
            involved.extend(group_of.get(seed).copied());
            for other in &self.images {
                if other != seed && !group_of.get(other).is_some_and(|g| involved.contains(g)) {
                    let (a, b) = ordered(*seed, *other);
                    if self.related(a, b) {
                        involved.extend(group_of.get(other).copied());
                    }
                }
            }
        }
        let mut members: Vec<ImageId> = involved
            .iter()
            .flat_map(|g| self.groups[*g].images.iter().copied())
            .chain(seeds.iter().copied())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        members.sort_by_key(|id| position.get(id).copied().unwrap_or(usize::MAX));
        let mut parents: Vec<usize> = (0..members.len()).collect();
        for n in 0..members.len() {
            for m in 0..n {
                if self.related(members[m], members[n]) {
                    grouping::join(&mut parents, m, n);
                }
            }
        }
        let mut components: std::collections::BTreeMap<usize, Group> = Default::default();
        for (n, id) in members.iter().enumerate() {
            components
                .entry(grouping::root(&mut parents, n))
                .or_insert_with(|| Group { images: Vec::new() })
                .images
                .push(*id);
        }
        let mut groups: Vec<Group> = self
            .groups
            .iter()
            .enumerate()
            .filter(|(g, _)| !involved.contains(g))
            .map(|(_, group)| group.clone())
            .chain(components.into_values())
            .collect();
        let first = |g: &Group| position.get(&g.images[0]).copied().unwrap_or(usize::MAX);
        groups.sort_by_key(first);
        let changed = groups != self.groups;
        self.groups = groups;
        Ok(changed)
    }

    /// Replaces the queue order, re-pointing the cursor and every undo/redo
    /// position at the same image (a removed image hands over to its successor).
    fn replace_queue(&mut self, images: Vec<ImageId>) {
        let index_of: HashMap<ImageId, usize> =
            images.iter().enumerate().map(|(n, id)| (*id, n)).collect();
        let old = std::mem::take(&mut self.images);
        let remap = |p: usize| -> usize {
            old.get(p..)
                .into_iter()
                .flatten()
                .find_map(|id| index_of.get(id))
                .or_else(|| {
                    old[..p.min(old.len())]
                        .iter()
                        .rev()
                        .find_map(|id| index_of.get(id))
                })
                .copied()
                .unwrap_or(0)
        };
        self.position = remap(self.position);
        for action in self.undo.iter_mut().chain(self.redo.iter_mut()) {
            action.before_position = remap(action.before_position);
            action.after_position = remap(action.after_position);
        }
        self.images = images;
    }

    fn still_admitted(&self, id: ImageId) -> EngineResult<bool> {
        let Ok(info) = self.index.image_info(id) else {
            return Ok(false);
        };
        Ok(info.path.exists()
            && self
                .folder
                .as_ref()
                .is_none_or(|f| info.path.starts_with(f)))
    }

    fn fill_keys(&mut self) -> EngineResult<()> {
        let missing: Vec<ImageId> = self
            .images
            .iter()
            .copied()
            .filter(|id| !self.keys.contains_key(id))
            .collect();
        if !missing.is_empty() {
            let keys = self.index.order_keys(&missing)?;
            self.keys.extend(missing.into_iter().zip(keys));
        }
        Ok(())
    }
}
