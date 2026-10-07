use crate::{CullSession, Decision, ImageId};
use engine_api::{EngineError, EngineResult};
use image::{RgbImage, imageops::FilterType};
use index::{ImageInfo, Index};
use previews::{Codec, Jpeg};
use std::collections::BTreeMap;
use std::ops::Deref;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub images: Vec<ImageId>,
}
#[derive(Debug, Clone, Copy)]
pub struct GroupingOptions {
    pub burst_gap_seconds: f64,
    pub near_duplicates: bool,
}
impl Default for GroupingOptions {
    fn default() -> Self {
        Self {
            burst_gap_seconds: 2.0,
            near_duplicates: true,
        }
    }
}
/// Supplies undirected edges for connected-component grouping in place of the
/// default burst OR dHash policy. Implementations should be symmetric.
/// Each unordered pair is evaluated once, in review-queue order. Hashes are
/// absent when previews are unavailable or `near_duplicates` is disabled.
pub trait GroupingStrategy: Send + Sync {
    fn related(
        &self,
        a: &ImageInfo,
        b: &ImageInfo,
        hash_a: Option<u64>,
        hash_b: Option<u64>,
        options: GroupingOptions,
    ) -> bool;
}

/// Higher is better. Ties pick the first image in review order.
pub trait Scorer: Send + Sync {
    fn score(&self, image: &ImageInfo) -> f64;
}
pub struct LargestFile;
impl Scorer for LargestFile {
    fn score(&self, image: &ImageInfo) -> f64 {
        image.size as f64
    }
}

/// 64 horizontal comparisons of a 9×8 luminance thumbnail.
pub fn dhash(image: &RgbImage) -> u64 {
    let gray = image::DynamicImage::ImageRgb8(image.clone()).into_luma8();
    let small = image::imageops::resize(&gray, 9, 8, FilterType::Triangle);
    let mut hash = 0;
    for y in 0..8 {
        for x in 0..8 {
            if small.get_pixel(x, y)[0] > small.get_pixel(x + 1, y)[0] {
                hash |= 1 << (y * 8 + x);
            }
        }
    }
    hash
}
pub fn dhash_jpeg(bytes: &[u8]) -> EngineResult<u64> {
    Jpeg.decode(bytes)
        .map(|image| dhash(&image))
        .map_err(|e| EngineError::Decode {
            format: "embedded JPEG".into(),
            message: e.to_string(),
        })
}
pub fn preview_hash(info: &ImageInfo) -> EngineResult<Option<u64>> {
    let ext = info
        .path
        .extension()
        .and_then(|x| x.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let bytes = if matches!(ext.as_str(), "jpg" | "jpeg") {
        Some(std::fs::read(&info.path).map_err(|e| EngineError::io_at(&info.path, &e))?)
    } else {
        raw_decode::RawSource::open(&info.path)?.embedded_preview()
    };
    bytes.as_deref().map(dhash_jpeg).transpose()
}
fn near_duplicate(a: u64, b: u64) -> bool {
    (a ^ b).count_ones() <= 6
}
pub(crate) fn root(parents: &mut [usize], mut n: usize) -> usize {
    while parents[n] != n {
        parents[n] = parents[parents[n]];
        n = parents[n];
    }
    n
}
pub(crate) fn join(parents: &mut [usize], a: usize, b: usize) {
    let a = root(parents, a);
    let b = root(parents, b);
    parents[a.max(b)] = a.min(b);
}
/// A snapshot rebuild has O(N) retained state and visits at most 4096 distinct
/// hash pairs per poll. Equal hashes collapse before comparisons; burst edges
/// use sorted adjacent times. No component-size squared loop holds a session lock.
pub(crate) struct DefaultRebuild {
    images: Vec<ImageId>,
    parents: Vec<usize>,
    hashes: Vec<(usize, u64)>,
    n: usize,
    m: usize,
}
impl DefaultRebuild {
    const PAIRS_PER_POLL: usize = 4096;
    fn advance(&mut self) -> bool {
        for _ in 0..Self::PAIRS_PER_POLL {
            if self.n >= self.hashes.len() {
                return true;
            }
            let (a, ha) = self.hashes[self.m];
            let (b, hb) = self.hashes[self.n];
            if root(&mut self.parents, a) != root(&mut self.parents, b) && near_duplicate(ha, hb) {
                join(&mut self.parents, a, b);
            }
            self.m += 1;
            if self.m == self.n {
                self.n += 1;
                self.m = 0;
            }
        }
        self.n >= self.hashes.len()
    }
    fn groups(&mut self, images: &[ImageId]) -> Vec<Group> {
        let mut groups: BTreeMap<usize, Group> = BTreeMap::new();
        for (n, id) in images.iter().enumerate() {
            groups
                .entry(root(&mut self.parents, n))
                .or_insert_with(|| Group { images: Vec::new() })
                .images
                .push(*id);
        }
        groups.into_values().collect()
    }
}
impl<I: Deref<Target = Index>> CullSession<I> {
    pub fn groups(&self) -> &[Group] {
        &self.groups
    }
    /// Unreadable previews do not prevent manual review or burst grouping.
    pub fn preview_errors(&self) -> &[(ImageId, EngineError)] {
        &self.preview_errors
    }
    pub fn set_scorer(&mut self, scorer: Box<dyn Scorer>) {
        self.scorer = Some(scorer);
    }
    /// Install a replacement edge policy; takes effect on the next `regroup`.
    pub fn set_grouping_strategy(&mut self, strategy: Box<dyn GroupingStrategy>) {
        self.grouping_strategy = Some(strategy);
    }
    /// Connected components of strategy edges, or default burst and dHash edges.
    /// Missing times/hashes in the default policy never
    /// match each other. Group and member order follow the original review queue.
    pub fn regroup(&mut self, options: GroupingOptions) -> EngineResult<()> {
        if !options.burst_gap_seconds.is_finite() || options.burst_gap_seconds < 0. {
            return Err(EngineError::invalid(
                "burst_gap_seconds",
                "must be finite and nonnegative",
            ));
        }
        let infos = self
            .images
            .iter()
            .map(|id| self.index.image_info(*id))
            .collect::<EngineResult<Vec<_>>>()?;
        let mut parents: Vec<_> = (0..infos.len()).collect();
        // Explicit regroup also revalidates source fingerprints, on the worker.
        // No cached hash is trusted on the open/regroup path itself.
        self.previews.reset();
        self.hashes.clear();
        self.custom_hashes_dirty = false;
        self.rebuild = None;
        self.preview_errors.clear();
        if options.near_duplicates && self.declared.is_none() {
            for info in &infos {
                self.previews.enqueue(info.clone());
            }
        }
        if let Some(strategy) = &self.grouping_strategy {
            // Downstream policies see a complete hash snapshot, not arbitrary
            // partial completion order. With hashes disabled they run at once.
            for n in 0..if self.previews.pending() {
                0
            } else {
                infos.len()
            } {
                for m in 0..n {
                    if strategy.related(&infos[m], &infos[n], None, None, options) {
                        join(&mut parents, m, n);
                    }
                }
            }
        } else {
            let mut timed: Vec<_> = infos
                .iter()
                .enumerate()
                .filter_map(|(n, i)| i.capture_seconds.map(|t| (n, t)))
                .collect();
            timed.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
            for pair in timed.windows(2) {
                if pair[1].1 - pair[0].1 <= options.burst_gap_seconds {
                    join(&mut parents, pair[0].0, pair[1].0);
                }
            }
        }
        let mut groups: BTreeMap<usize, Group> = BTreeMap::new();
        for (n, id) in self.images.iter().enumerate() {
            groups
                .entry(root(&mut parents, n))
                .or_insert_with(|| Group { images: Vec::new() })
                .images
                .push(*id);
        }
        self.groups = groups.into_values().collect();
        self.options = options;
        self.infos = infos.into_iter().map(|i| (i.id, i)).collect();
        Ok(())
    }
    /// Whether two queue images belong together under the session's policy
    /// (`a` before `b` in queue order). The pairwise form of `regroup`: a
    /// burst chain within the gap is a chain of pairs within the gap.
    pub(crate) fn related(&self, a: ImageId, b: ImageId) -> bool {
        let (Some(ia), Some(ib)) = (self.infos.get(&a), self.infos.get(&b)) else {
            return false;
        };
        let options = self.options;
        let hash = |id| {
            if options.near_duplicates && self.declared.is_none() {
                self.hashes.get(&id).copied().flatten()
            } else {
                None
            }
        };
        if let Some(strategy) = &self.grouping_strategy {
            return strategy.related(ia, ib, hash(a), hash(b), options);
        }
        if let (Some(ta), Some(tb)) = (ia.capture_seconds, ib.capture_seconds)
            && (ta - tb).abs() <= options.burst_gap_seconds
        {
            return true;
        }
        matches!((hash(a), hash(b)), (Some(x), Some(y)) if near_duplicate(x, y))
    }
    /// Refreshes the grouping inputs of `id` (new, moved or re-timed image).
    pub(crate) fn refresh_grouping_inputs(&mut self, id: ImageId) -> EngineResult<()> {
        let info = self.index.image_info(id)?;
        self.preview_errors.retain(|(e, _)| *e != id);
        self.hashes.remove(&id);
        self.previews.remove(id);
        if self.options.near_duplicates && self.declared.is_none() {
            self.previews.enqueue(info.clone());
        }
        self.infos.insert(id, info);
        Ok(())
    }
    /// Wake the host after a bounded result batch. The callback runs outside
    /// all session locks and may request another poll. It must not own the session.
    pub fn set_preview_notifier(&mut self, notify: impl Fn() + Send + Sync + 'static) {
        self.preview_notify = Some(std::sync::Arc::new(notify));
    }
    /// Cancel/disconnect work immediately; wait on the returned barrier off the
    /// main thread to guarantee worker retirement and notification completion.
    pub fn retire_previews(&mut self) -> crate::PreviewShutdown {
        self.rebuild = None;
        self.custom_hashes_dirty = false;
        self.previews.retire()
    }
    /// True while deferred hashes remain. Hosts poll after displaying the queue.
    pub fn previews_pending(&self) -> bool {
        self.previews.pending() || self.rebuild.is_some()
    }
    /// Starts/advances background hashing, applying at most 16 ready results.
    /// Never decodes pixels on the caller. Drop cancels queued work and transfers
    /// joining to the lifecycle owner; `retire_previews().wait()` is the explicit
    /// completion barrier and must run off the main thread.
    pub fn poll_previews(&mut self) -> EngineResult<bool> {
        let results = self
            .previews
            .poll(&self.preview_hash, &self.preview_notify)?;
        let mut changed = Vec::new();
        for (id, result) in results {
            if !self.infos.contains_key(&id) {
                continue;
            }
            self.preview_errors.retain(|(image, _)| *image != id);
            let hash = match result {
                Ok(hash) => hash,
                Err(error) => {
                    self.preview_errors.push((id, error));
                    None
                }
            };
            self.hashes.insert(id, hash);
            changed.push(id);
        }
        if self.grouping_strategy.is_some() {
            self.custom_hashes_dirty |= !changed.is_empty();
            return if !self.previews.pending() && self.custom_hashes_dirty {
                self.custom_hashes_dirty = false;
                self.regroup_images(&self.images.clone())
            } else {
                Ok(false)
            };
        }
        if self.rebuild.is_some() {
            // New hashes invalidate the reconstruction snapshot. Rebuild from
            // all accepted inputs, never mix parents from different revisions.
            if !changed.is_empty() {
                return self.rebuild_default();
            }
            return self.advance_rebuild();
        }
        if changed.is_empty() {
            return Ok(false);
        }
        // New hashes only add edges. Join existing components and compare each
        // newly hashed image once against the queue: O(batch * N), not repeated
        // all-pairs reconstruction of an ever-growing near-duplicate component.
        let positions: std::collections::HashMap<_, _> = self
            .images
            .iter()
            .enumerate()
            .map(|(n, id)| (*id, n))
            .collect();
        let mut parents: Vec<_> = (0..self.images.len()).collect();
        for group in &self.groups {
            for pair in group.images.windows(2) {
                join(&mut parents, positions[&pair[0]], positions[&pair[1]]);
            }
        }
        for id in changed {
            let n = positions[&id];
            for (m, other) in self.images.iter().enumerate() {
                if m != n && root(&mut parents, m) != root(&mut parents, n) {
                    let (a, b) = if m < n { (*other, id) } else { (id, *other) };
                    if self.related(a, b) {
                        join(&mut parents, m, n);
                    }
                }
            }
        }
        let mut groups: BTreeMap<usize, Group> = BTreeMap::new();
        for (n, id) in self.images.iter().enumerate() {
            groups
                .entry(root(&mut parents, n))
                .or_insert_with(|| Group { images: Vec::new() })
                .images
                .push(*id);
        }
        let groups: Vec<_> = groups.into_values().collect();
        let changed = groups != self.groups;
        self.groups = groups;
        Ok(changed)
    }
    /// Reconstruct the default graph without an unbounded pair loop. Exact
    /// hash and burst edges are linear/sorted; remaining edges advance in polls.
    pub(crate) fn rebuild_default(&mut self) -> EngineResult<bool> {
        let mut parents: Vec<_> = (0..self.images.len()).collect();
        let mut timed: Vec<_> = self
            .images
            .iter()
            .enumerate()
            .filter_map(|(n, id)| self.infos.get(id)?.capture_seconds.map(|t| (n, t)))
            .collect();
        timed.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        for pair in timed.windows(2) {
            if pair[1].1 - pair[0].1 <= self.options.burst_gap_seconds {
                join(&mut parents, pair[0].0, pair[1].0);
            }
        }
        let mut exact = std::collections::HashMap::new();
        let mut hashes = Vec::new();
        if self.options.near_duplicates && self.declared.is_none() {
            for (n, id) in self.images.iter().enumerate() {
                if let Some(hash) = self.hashes.get(id).copied().flatten() {
                    if let Some(other) = exact.get(&hash) {
                        join(&mut parents, *other, n);
                    } else {
                        exact.insert(hash, n);
                        hashes.push((n, hash));
                    }
                }
            }
        }
        self.rebuild = Some(DefaultRebuild {
            images: self.images.clone(),
            parents,
            hashes,
            n: 1,
            m: 0,
        });
        self.advance_rebuild()
    }
    fn advance_rebuild(&mut self) -> EngineResult<bool> {
        // Queue removal and navigation reordering can happen between polls.
        // Never interpret parents from a previous queue as current positions.
        if self
            .rebuild
            .as_ref()
            .is_some_and(|r| r.images != self.images)
        {
            return self.rebuild_default();
        }
        let Some(rebuild) = &mut self.rebuild else {
            return Ok(false);
        };
        let done = rebuild.advance();
        let groups = rebuild.groups(&self.images);
        let changed = self.groups != groups;
        self.groups = groups;
        if done {
            self.rebuild = None;
        } else {
            self.previews.wake(&self.preview_notify)?;
        }
        Ok(changed)
    }
    pub fn current_group(&self) -> Option<usize> {
        let id = self.current()?;
        self.groups.iter().position(|g| g.images.contains(&id))
    }
    fn move_to(&mut self, id: ImageId) {
        if let Some(n) = self.images.iter().position(|i| *i == id) {
            self.position = n;
        }
    }
    pub fn next_group(&mut self) {
        if let Some(n) = self.current_group()
            && let Some(group) = self.groups.get(n + 1)
        {
            self.move_to(group.images[0]);
        }
    }
    pub fn prev_group(&mut self) {
        if let Some(n) = self.current_group().and_then(|n| n.checked_sub(1)) {
            self.move_to(self.groups[n].images[0]);
        }
    }
    pub fn next_in_group(&mut self) {
        if let Some(n) = self.current_group() {
            let ids = &self.groups[n].images;
            if let Some(pos) = ids.iter().position(|id| Some(*id) == self.current())
                && let Some(id) = ids.get(pos + 1)
            {
                self.move_to(*id);
            }
        }
    }
    pub fn prev_in_group(&mut self) {
        if let Some(n) = self.current_group() {
            let ids = &self.groups[n].images;
            if let Some(pos) = ids.iter().position(|id| Some(*id) == self.current())
                && let Some(id) = pos.checked_sub(1).map(|p| ids[p])
            {
                self.move_to(id);
            }
        }
    }
    /// Group containing `id`, if it is in the review queue.
    pub fn group_of(&self, id: ImageId) -> Option<usize> {
        self.groups.iter().position(|g| g.images.contains(&id))
    }
    pub fn best_in_group(&self, group: usize) -> EngineResult<ImageId> {
        let group = self
            .groups
            .get(group)
            .ok_or_else(|| EngineError::invalid("group", "outside session"))?;
        // Read current persisted signals, including scores computed after opening
        // the session. Never compare byte counts against normalized quality.
        let mut catalog = BTreeMap::new();
        if self.scorer.is_none() {
            for &id in &group.images {
                let scores = self.index.scores(id)?;
                let get =
                    |signal: &str| scores.iter().find(|s| s.signal == signal).map(|s| s.value);
                let value = match (get("quality"), get("face_sharpness")) {
                    (Some(q), Some(f)) => Some(q * (0.5 + 0.5 * f)),
                    (q, f) => q.or(f),
                };
                if let Some(value) = value {
                    catalog.insert(id, value);
                }
            }
        }
        let mut best = None;
        for id in &group.images {
            let info = self.index.image_info(*id)?;
            let score = if let Some(scorer) = &self.scorer {
                scorer.score(&info)
            } else if catalog.is_empty() {
                LargestFile.score(&info)
            } else {
                catalog.get(id).copied().unwrap_or(-1.)
            };
            if !score.is_finite() {
                return Err(EngineError::invalid(
                    "score",
                    "scorer returned non-finite value",
                ));
            }
            if best.is_none_or(|(_, previous)| score > previous) {
                best = Some((*id, score));
            }
        }
        best.map(|(id, _)| id)
            .ok_or_else(|| EngineError::invalid("group", "empty"))
    }
    pub fn keep_best_reject_rest(&mut self, group: usize) -> EngineResult<ImageId> {
        let best = self.best_in_group(group)?;
        let decisions: Vec<_> = self.groups[group]
            .images
            .iter()
            .map(|id| {
                let decision = if *id == best {
                    Decision::Keep
                } else {
                    Decision::Reject
                };
                (*id, decision)
            })
            .collect();
        self.decide_each(&decisions)?;
        Ok(best)
    }
}
