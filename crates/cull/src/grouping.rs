use crate::{CullSession, Decision, ImageId};
use engine_api::{EngineError, EngineResult};
use image::{RgbImage, imageops::FilterType};
use index::{ImageInfo, Index};
use previews::{Codec, Jpeg};
use std::collections::{BTreeMap, HashMap, HashSet};
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
/// Incremental default-policy regroup after edits, inserts and removals.
///
/// Starts from the published groups: groups without a damaged member stay
/// joined (their edges are unchanged). Damaged components are recomputed from
/// sorted burst edges, equal-hash joins, distinct-hash pairs inside the damaged
/// set, and pairs between each image whose hash appeared or changed and every
/// distinct queue hash: O(|damaged|^2 + |changed| * N) pair checks in the worst
/// case, at most `PAIRS_PER_POLL` per poll, with O(N) retained state. The
/// published groups stay the last complete result until the job finishes.
pub(crate) struct DefaultRebuild {
    /// Queue snapshot the positions below refer to. A changed queue restarts.
    images: Vec<ImageId>,
    position: HashMap<ImageId, usize>,
    /// Images whose components are recomputed, and images whose hash changed.
    damaged: HashSet<ImageId>,
    changed: HashSet<ImageId>,
    parents: Vec<usize>,
    /// One representative per distinct hash inside the damaged set.
    internal: Vec<(usize, u64)>,
    /// Changed images compared with every distinct queue hash.
    sources: Vec<(usize, u64)>,
    targets: Vec<(usize, u64)>,
    target_values: HashSet<u64>,
    i: usize,
    j: usize,
    s: usize,
    t: usize,
}
impl DefaultRebuild {
    /// About a millisecond of pair checks: a single edit in a 20k-image
    /// library settles within one or two polls.
    pub(crate) const PAIRS_PER_POLL: usize = 1 << 16;
    fn check(&mut self, a: (usize, u64), b: (usize, u64)) {
        if root(&mut self.parents, a.0) != root(&mut self.parents, b.0) && near_duplicate(a.1, b.1)
        {
            join(&mut self.parents, a.0, b.0);
        }
    }
    fn advance(&mut self, checks: &mut u64) -> bool {
        let mut budget = Self::PAIRS_PER_POLL;
        while self.i < self.internal.len() {
            while self.j < self.i {
                if budget == 0 {
                    return false;
                }
                budget -= 1;
                *checks += 1;
                self.check(self.internal[self.j], self.internal[self.i]);
                self.j += 1;
            }
            self.i += 1;
            self.j = 0;
        }
        while self.s < self.sources.len() {
            while self.t < self.targets.len() {
                if budget == 0 {
                    return false;
                }
                budget -= 1;
                *checks += 1;
                self.check(self.sources[self.s], self.targets[self.t]);
                self.t += 1;
            }
            self.s += 1;
            self.t = 0;
        }
        true
    }
    /// A hash appeared for `id` while the job runs: it only adds edges. Later
    /// sources see it as a target; it is compared with every earlier target.
    fn add_source(&mut self, id: ImageId, hash: u64) {
        self.changed.insert(id);
        if let Some(&n) = self.position.get(&id) {
            self.sources.push((n, hash));
            if self.target_values.insert(hash) {
                self.targets.push((n, hash));
            }
        }
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
    /// Refreshes the grouping inputs of `id` (new, moved, edited or re-timed
    /// image). A known hash stays in use while the image is re-hashed: grouping
    /// changes only if the new hash differs. Returns whether the capture time
    /// (the burst input) changed.
    pub(crate) fn refresh_grouping_inputs(&mut self, id: ImageId) -> EngineResult<bool> {
        let info = self.index.image_info(id)?;
        self.previews.remove(id);
        if self.options.near_duplicates && self.declared.is_none() {
            self.previews.enqueue(info.clone());
        }
        let retimed = self
            .infos
            .get(&id)
            .is_none_or(|old| old.capture_seconds != info.capture_seconds);
        self.infos.insert(id, info);
        Ok(retimed)
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
        self.apply_hash_results(results)
    }
    /// Applies completed hash replies (already version-checked) to grouping.
    pub(crate) fn apply_hash_results(
        &mut self,
        results: Vec<(ImageId, EngineResult<Option<u64>>)>,
    ) -> EngineResult<bool> {
        // Appeared hashes only add edges; changed or lost hashes can split.
        let mut gained = Vec::new();
        let mut damaged = Vec::new();
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
            match self.hashes.insert(id, hash).flatten() {
                old if old == hash => {}
                Some(_) => damaged.push(id),
                None => gained.push(id),
            }
        }
        if self.grouping_strategy.is_some() {
            self.custom_hashes_dirty |= !gained.is_empty() || !damaged.is_empty();
            return if !self.previews.pending() && self.custom_hashes_dirty {
                self.custom_hashes_dirty = false;
                self.regroup_images(&self.images.clone())
            } else {
                Ok(false)
            };
        }
        let mut regrouped = false;
        if !gained.is_empty() {
            if let Some(job) = &mut self.rebuild {
                for id in &gained {
                    if let Some(Some(hash)) = self.hashes.get(id) {
                        job.add_source(*id, *hash);
                    }
                }
            }
            regrouped |= self.join_new_hashes(&gained);
        }
        if !damaged.is_empty() {
            regrouped |= self.schedule_default(&damaged, &damaged)?;
        } else if self.rebuild.is_some() {
            regrouped |= self.advance_rebuild()?;
        }
        Ok(regrouped)
    }
    /// New hashes only add edges. Join existing components and compare each
    /// newly hashed image once against the queue: O(batch * N), without an
    /// all-pairs reconstruction. While a regroup job runs this updates the
    /// published groups too; the job also receives the hashes as sources.
    fn join_new_hashes(&mut self, gained: &[ImageId]) -> bool {
        let positions: HashMap<_, _> = self
            .images
            .iter()
            .enumerate()
            .map(|(n, id)| (*id, n))
            .collect();
        let mut parents: Vec<_> = (0..self.images.len()).collect();
        for group in &self.groups {
            for pair in group.images.windows(2) {
                if let (Some(a), Some(b)) = (positions.get(&pair[0]), positions.get(&pair[1])) {
                    join(&mut parents, *a, *b);
                }
            }
        }
        for id in gained {
            let Some(&n) = positions.get(id) else {
                continue;
            };
            for (m, other) in self.images.iter().enumerate() {
                if m != n && root(&mut parents, m) != root(&mut parents, n) {
                    let (a, b) = if m < n { (*other, *id) } else { (*id, *other) };
                    self.pair_checks += 1;
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
        changed
    }
    /// Schedules (or extends) the incremental default regroup: `seeds` damage
    /// their published components; `changed` images are compared with every
    /// distinct queue hash. Runs the first chunk now and publishes only a
    /// complete result.
    pub(crate) fn schedule_default(
        &mut self,
        seeds: &[ImageId],
        changed: &[ImageId],
    ) -> EngineResult<bool> {
        let (mut damaged, mut changed_set) = match self.rebuild.take() {
            Some(job) => (job.damaged, job.changed),
            None => Default::default(),
        };
        damaged.extend(seeds.iter().copied());
        changed_set.extend(changed.iter().copied());
        let position: HashMap<ImageId, usize> = self
            .images
            .iter()
            .enumerate()
            .map(|(n, id)| (*id, n))
            .collect();
        damaged.retain(|id| position.contains_key(id));
        changed_set.retain(|id| position.contains_key(id));
        // Inserted singletons stay published in queue order while the job runs.
        self.groups.sort_by_key(|g| {
            g.images
                .first()
                .and_then(|id| position.get(id))
                .copied()
                .unwrap_or(usize::MAX)
        });
        // Close over published components: a damaged member damages its group.
        let mut parents: Vec<_> = (0..self.images.len()).collect();
        let mut closed = damaged.clone();
        for group in &self.groups {
            if group.images.iter().any(|id| damaged.contains(id)) {
                closed.extend(group.images.iter().filter(|id| position.contains_key(id)));
            } else {
                for pair in group.images.windows(2) {
                    if let (Some(a), Some(b)) = (position.get(&pair[0]), position.get(&pair[1])) {
                        join(&mut parents, *a, *b);
                    }
                }
            }
        }
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
        let mut targets = Vec::new();
        let mut target_values = HashSet::new();
        let mut internal = Vec::new();
        let mut sources = Vec::new();
        if self.options.near_duplicates && self.declared.is_none() {
            let mut exact: HashMap<u64, usize> = HashMap::new();
            let mut internal_values = HashSet::new();
            for (n, id) in self.images.iter().enumerate() {
                let Some(hash) = self.hashes.get(id).copied().flatten() else {
                    continue;
                };
                match exact.get(&hash) {
                    Some(&other) => join(&mut parents, other, n),
                    None => {
                        exact.insert(hash, n);
                        target_values.insert(hash);
                        targets.push((n, hash));
                    }
                }
                if closed.contains(id) && internal_values.insert(hash) {
                    internal.push((n, hash));
                }
                if changed_set.contains(id) {
                    sources.push((n, hash));
                }
            }
        }
        self.rebuild = Some(DefaultRebuild {
            images: self.images.clone(),
            position,
            damaged: closed,
            changed: changed_set,
            parents,
            internal,
            sources,
            targets,
            target_values,
            i: 1,
            j: 0,
            s: 0,
            t: 0,
        });
        self.advance_rebuild()
    }
    fn advance_rebuild(&mut self) -> EngineResult<bool> {
        // Queue removal, insertion and reordering can happen between polls.
        // Never interpret positions from a previous queue: restart from the
        // published groups, keeping the damaged and changed images.
        if self
            .rebuild
            .as_ref()
            .is_some_and(|r| r.images != self.images)
        {
            return self.schedule_default(&[], &[]);
        }
        let Some(rebuild) = &mut self.rebuild else {
            return Ok(false);
        };
        if !rebuild.advance(&mut self.pair_checks) {
            self.previews.wake(&self.preview_notify)?;
            return Ok(false);
        }
        let groups = rebuild.groups(&self.images);
        self.rebuild = None;
        let changed = self.groups != groups;
        self.groups = groups;
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
