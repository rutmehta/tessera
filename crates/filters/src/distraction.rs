//! Suggestion-only distraction hook. The CPU implementation is a deterministic
//! geometric stub, NOT semantic wire/person segmentation. Callers supply selected
//! ml-faces detections in canvas pixels, review the masks, then explicitly remove.
use crate::{
    Buffer, caf, checkpoint,
    remove::{Remove, RemoveParams, RemoveResult},
};
use compositor::Raster;
use engine_api::{EngineError, EngineResult};
use ml_faces::Face;
use std::sync::atomic::AtomicBool;

pub struct DistractionMasks {
    /// Row-major canvas coverage, 1 = remove.
    pub wires: Vec<f32>,
    /// Dilated face-box proxy, not a full-body person mask.
    pub people: Vec<f32>,
}
impl DistractionMasks {
    pub fn union(&self, area: usize) -> EngineResult<Vec<f32>> {
        caf::validate_mask(&self.wires, area)?;
        caf::validate_mask(&self.people, area)?;
        Ok(self
            .wires
            .iter()
            .zip(&self.people)
            .map(|(&a, &b)| a.max(b))
            .collect())
    }
}

pub trait DistractionDetector {
    fn detect(
        &mut self,
        input: &Raster,
        faces: &[Face],
        cancel: &AtomicBool,
    ) -> EngineResult<DistractionMasks>;

    /// Explicit opt-in convenience path through the same Remove interface used
    /// by hand-painted masks. It does not fetch faces or inpainting weights.
    fn remove(
        &mut self,
        input: &Raster,
        faces: &[Face],
        remover: &mut dyn Remove,
        params: &RemoveParams,
        cancel: &AtomicBool,
    ) -> EngineResult<RemoveResult> {
        let masks = self.detect(input, faces, cancel)?;
        checkpoint(cancel)?;
        remover.apply(
            input,
            &masks.union(input.extent().area() as usize)?,
            params,
            cancel,
        )
    }
}

#[derive(Default)]
pub struct CpuDistractionDetector;
impl DistractionDetector for CpuDistractionDetector {
    fn detect(
        &mut self,
        input: &Raster,
        faces: &[Face],
        cancel: &AtomicBool,
    ) -> EngineResult<DistractionMasks> {
        checkpoint(cancel)?;
        let src = Buffer::read(input, cancel)?;
        let mut people = vec![0.0; src.pixels.len()];
        for face in faces {
            checkpoint(cancel)?;
            let [x, y, w, h] = face.bbox;
            if face.bbox.iter().any(|v| !v.is_finite())
                || w <= 0.0
                || h <= 0.0
                || !face.score.is_finite()
                || !(0.0..=1.0).contains(&face.score)
            {
                return Err(EngineError::invalid(
                    "distraction.faces",
                    "finite positive pixel boxes and scores in [0,1] required",
                ));
            }
            // 50% on each side, clipped before integer conversion. Entirely
            // off-canvas boxes are harmless empty suggestions.
            let left = (x - 0.5 * w).floor().clamp(0.0, src.w as f32) as usize;
            let right = (x + 1.5 * w).ceil().clamp(0.0, src.w as f32) as usize;
            let top = (y - 0.5 * h).floor().clamp(0.0, src.h as f32) as usize;
            let bottom = (y + 1.5 * h).ceil().clamp(0.0, src.h as f32) as usize;
            for row in top..bottom {
                checkpoint(cancel)?;
                people[row * src.w + left..row * src.w + right].fill(1.0);
            }
        }
        // Two-sided contrast rejects ordinary step edges. Probe four normals
        // to find horizontal, vertical and diagonal narrow ridges/valleys.
        let luma: Vec<f32> = src
            .pixels
            .iter()
            .map(|p| 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2])
            .collect();
        let mut candidates = vec![false; luma.len()];
        for y in 0..src.h {
            checkpoint(cancel)?;
            for x in 0..src.w {
                let i = y * src.w + x;
                for (dx, dy) in [(2, 0), (0, 2), (2, 2), (2, -2)] {
                    let (ax, ay) = (x as i64 + dx, y as i64 + dy);
                    let (bx, by) = (x as i64 - dx, y as i64 - dy);
                    if ax < 0
                        || ay < 0
                        || bx < 0
                        || by < 0
                        || ax >= src.w as i64
                        || bx >= src.w as i64
                        || ay >= src.h as i64
                        || by >= src.h as i64
                    {
                        continue;
                    }
                    let a = luma[i] - luma[ay as usize * src.w + ax as usize];
                    let b = luma[i] - luma[by as usize * src.w + bx as usize];
                    if a * b > 0.0 && a.abs().min(b.abs()) >= 0.15 {
                        candidates[i] = true;
                    }
                }
            }
        }
        let mut wires = vec![0.0; luma.len()];
        let mut component = Vec::new();
        for seed in 0..candidates.len() {
            if seed % src.w == 0 {
                checkpoint(cancel)?;
            }
            if !candidates[seed] {
                continue;
            }
            component.clear();
            component.push(seed);
            candidates[seed] = false;
            let mut next = 0;
            while next < component.len() {
                if next % 4096 == 0 {
                    checkpoint(cancel)?;
                }
                let i = component[next];
                next += 1;
                let (x, y) = (i % src.w, i / src.w);
                for yy in y.saturating_sub(1)..=(y + 1).min(src.h - 1) {
                    for xx in x.saturating_sub(1)..=(x + 1).min(src.w - 1) {
                        let j = yy * src.w + xx;
                        if candidates[j] {
                            candidates[j] = false;
                            component.push(j);
                        }
                    }
                }
            }
            if component.len() < 12 {
                continue;
            }
            let n = component.len() as f64;
            let mx = component.iter().map(|i| (i % src.w) as f64).sum::<f64>() / n;
            let my = component.iter().map(|i| (i / src.w) as f64).sum::<f64>() / n;
            let (mut xx, mut yy, mut xy) = (0.0, 0.0, 0.0);
            for &i in &component {
                let (x, y) = ((i % src.w) as f64 - mx, (i / src.w) as f64 - my);
                xx += x * x / n;
                yy += y * y / n;
                xy += x * y / n;
            }
            let delta = ((xx - yy).powi(2) + 4.0 * xy * xy).sqrt();
            let major = (xx + yy + delta) * 0.5;
            let minor = ((xx + yy - delta) * 0.5).max(0.0);
            if major >= 12.0 && minor <= 2.0 && major >= 16.0 * minor.max(0.25) {
                for &i in &component {
                    wires[i] = 1.0;
                }
            }
        }
        Ok(DistractionMasks { wires, people })
    }
}
