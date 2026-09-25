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
fn preview_hash(info: &ImageInfo) -> EngineResult<Option<u64>> {
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
fn root(parents: &mut [usize], mut n: usize) -> usize {
    while parents[n] != n {
        parents[n] = parents[parents[n]];
        n = parents[n];
    }
    n
}
fn join(parents: &mut [usize], a: usize, b: usize) {
    let a = root(parents, a);
    let b = root(parents, b);
    parents[a.max(b)] = a.min(b);
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
        self.scorer = scorer;
    }
    /// Connected components of burst and dHash edges. Missing times/hashes never
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
        let mut errors = Vec::new();
        if options.near_duplicates {
            let mut hashes: Vec<(usize, u64)> = Vec::new();
            for (n, info) in infos.iter().enumerate() {
                match preview_hash(info) {
                    Ok(Some(hash)) => {
                        for &(m, other) in &hashes {
                            if (hash ^ other).count_ones() <= 6 {
                                join(&mut parents, m, n);
                            }
                        }
                        hashes.push((n, hash));
                    }
                    Ok(None) => {}
                    Err(error) => errors.push((info.id, error)),
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
        self.preview_errors = errors;
        Ok(())
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
        let mut best = None;
        for id in &group.images {
            let score = self.scorer.score(&self.index.image_info(*id)?);
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
