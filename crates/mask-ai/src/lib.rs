//! Shared AI-mask backend and composition for preview and export.
use engine_api::recipe::{
    mask::{LocalAdjustment, MaskCombine, MaskComponent, MaskKind},
    settings::NormalizedRect,
};
use image::RgbImage;
use std::{path::PathBuf, sync::Arc};
/// Sensor-aligned single-channel raster.
#[derive(Clone)]
pub struct AlphaPlane {
    pub width: u32,
    pub height: u32,
    pub data: Vec<f32>,
}
/// A segmentation request on a display-oriented image; boxes and clicks are
/// normalized to it.
#[derive(Clone, Debug, PartialEq)]
pub enum SegmentRequest {
    Subject,
    Sky,
    Background,
    Prompts {
        clicks: Vec<[f32; 2]>,
        boxes: Vec<[f32; 4]>,
    },
}

/// The segmentation backend: `ml_segment::Segmenter`, or a stand-in in tests.
pub trait MaskSegmenter: Send {
    /// A mask of `image`'s size, values in `0..=1`.
    fn segment(&mut self, image: &RgbImage, request: &SegmentRequest) -> anyhow::Result<Vec<f32>>;
}

impl MaskSegmenter for ml_segment::Segmenter {
    fn segment(&mut self, image: &RgbImage, request: &SegmentRequest) -> anyhow::Result<Vec<f32>> {
        let raster = match request {
            SegmentRequest::Subject => self.subject(image, 0)?,
            SegmentRequest::Sky => self.sky(image, 0)?,
            SegmentRequest::Background => self.background(image, 0)?,
            SegmentRequest::Prompts { clicks, boxes } => self.promptable(
                image,
                &ml_segment::Prompts {
                    clicks: clicks
                        .iter()
                        .map(|&point| ml_segment::Click {
                            point,
                            positive: true,
                        })
                        .collect(),
                    boxes: boxes.clone(),
                },
                0,
            )?,
        };
        anyhow::ensure!(
            (raster.width(), raster.height()) == image.dimensions(),
            "segmentation returned a different size"
        );
        Ok(raster.data().to_vec())
    }
}

/// Bilinear resampling with pixel-centre alignment.
pub fn resample(src: &AlphaPlane, width: u32, height: u32) -> Vec<f32> {
    let (sw, sh) = (src.width as usize, src.height as usize);
    let (w, h) = (width as usize, height as usize);
    if (sw, sh) == (w, h) {
        return src.data.clone();
    }
    let axis = |i: usize, n: usize, sn: usize| {
        let s = ((i as f32 + 0.5) * sn as f32 / n as f32 - 0.5).clamp(0.0, (sn - 1) as f32);
        let i0 = s.floor() as usize;
        (i0, (i0 + 1).min(sn - 1), s - i0 as f32)
    };
    let xs: Vec<_> = (0..w).map(|x| axis(x, w, sw)).collect();
    let mut out = vec![0f32; w * h];
    for (y, row) in out.chunks_mut(w).enumerate() {
        let (y0, y1, fy) = axis(y, h, sh);
        let (r0, r1) = (&src.data[y0 * sw..][..sw], &src.data[y1 * sw..][..sw]);
        for (o, &(x0, x1, fx)) in row.iter_mut().zip(&xs) {
            let top = r0[x0] + (r0[x1] - r0[x0]) * fx;
            let bottom = r1[x0] + (r1[x1] - r1[x0]) * fx;
            *o = (top + (bottom - top) * fy).clamp(0.0, 1.0);
        }
    }
    out
}

/// Compose external AI planes and procedural masks with the recipe semantics.
pub fn compose(
    input: &pipeline_cpu::Image,
    group: &LocalAdjustment,
    mut ai: impl FnMut(&MaskKind, u32, u32) -> engine_api::EngineResult<Arc<[f32]>>,
) -> engine_api::EngineResult<Vec<f32>> {
    let (w, h) = (input.width(), input.height());
    let mut out = vec![0f32; w as usize * h as usize];
    for (index, c) in group.components.iter().enumerate() {
        let plane: Arc<[f32]> = match c.kind.is_ai() {
            true => ai(&c.kind, w, h)?,
            false => {
                let single = LocalAdjustment {
                    components: vec![MaskComponent::new(c.kind.clone())],
                    ..Default::default()
                };
                pipeline_cpu::masks::rasterize(
                    input,
                    &single,
                    pipeline_cpu::masks::MaskOptions::default(),
                )?
                .into()
            }
        };
        if plane.len() != out.len() || plane.iter().any(|v| !(0.0..=1.0).contains(v)) {
            return Err(engine_api::EngineError::invalid(
                "mask",
                "invalid segmentation raster",
            ));
        }
        // Composition exactly as `pipeline_cpu::masks::rasterize`.
        for (a, &b) in out.iter_mut().zip(plane.iter()) {
            let b = if c.invert { 1.0 - b } else { b };
            *a = if index == 0 {
                b
            } else {
                match c.combine {
                    MaskCombine::Add => a.max(b),
                    MaskCombine::Subtract => *a * (1.0 - b),
                    MaskCombine::Intersect => *a * b,
                }
            };
        }
    }
    if group.invert && !group.components.is_empty() {
        for v in &mut out {
            *v = 1.0 - *v;
        }
    }
    Ok(out)
}
/// EXIF orientation: displayed normalized point → stored (sensor) point, as
/// the loupe shader samples.
pub fn orient(d: [f32; 2], o: u16) -> [f32; 2] {
    let [x, y] = d;
    match o {
        2 => [1.0 - x, y],
        3 => [1.0 - x, 1.0 - y],
        4 => [x, 1.0 - y],
        5 => [y, x],
        6 => [y, 1.0 - x],
        7 => [1.0 - y, 1.0 - x],
        8 => [1.0 - y, x],
        _ => [x, y],
    }
}

/// Stored → displayed (the inverse of [`orient`]).
pub fn unorient(s: [f32; 2], o: u16) -> [f32; 2] {
    orient(
        s,
        match o {
            6 => 8,
            8 => 6,
            o => o,
        },
    )
}

/// `f(displayed x, y)` for every stored pixel of a `w × h` (sensor) plane, or
/// the other way round: maps pixel centres through the orientation.
pub fn reorient<T: Copy>(
    src: &[T],
    sw: u32,
    sh: u32,
    dw: u32,
    dh: u32,
    map: impl Fn([f32; 2]) -> [f32; 2],
) -> Vec<T> {
    let mut out = Vec::with_capacity(dw as usize * dh as usize);
    for y in 0..dh {
        for x in 0..dw {
            let p = map([(x as f32 + 0.5) / dw as f32, (y as f32 + 0.5) / dh as f32]);
            let sx = ((p[0] * sw as f32) as u32).min(sw - 1);
            let sy = ((p[1] * sh as f32) as u32).min(sh - 1);
            out.push(src[(sy * sw + sx) as usize]);
        }
    }
    out
}

/// Valid clicks and boxes of an object component, `None` if any is invalid.
#[allow(clippy::type_complexity)]
pub fn object_prompts(
    region: Option<&NormalizedRect>,
    points: &[[f32; 2]],
) -> Option<(Vec<[f32; 2]>, Vec<[f32; 4]>)> {
    let unit = |v: f32| v.is_finite() && (0.0..=1.0).contains(&v);
    if !points.iter().flatten().all(|&v| unit(v)) || points.len() > 256 {
        return None;
    }
    let boxes = match region {
        Some(r) => {
            let b = [r.left, r.top, r.right, r.bottom];
            if !b.iter().all(|&v| unit(v)) || b[0] >= b[2] || b[1] >= b[3] {
                return None;
            }
            vec![b]
        }
        None => Vec::new(),
    };
    Some((points.to_vec(), boxes))
}

pub fn request(kind: &MaskKind, orientation: u16) -> anyhow::Result<SegmentRequest> {
    Ok(match kind {
        MaskKind::Subject { .. } => SegmentRequest::Subject,
        MaskKind::Sky { .. } => SegmentRequest::Sky,
        MaskKind::Background { .. } => SegmentRequest::Background,
        MaskKind::Object { region, points, .. } => {
            let (clicks, boxes) = object_prompts(region.as_ref(), points)
                .ok_or_else(|| anyhow::anyhow!("invalid object prompt"))?;
            anyhow::ensure!(
                !clicks.is_empty() || !boxes.is_empty(),
                "object mask requires a box or click prompt; text prompts are unsupported"
            );
            let to_shown = |p: [f32; 2]| unorient(p, orientation);
            SegmentRequest::Prompts {
                clicks: clicks.into_iter().map(to_shown).collect(),
                boxes: boxes
                    .into_iter()
                    .map(|b| {
                        let a = to_shown([b[0], b[1]]);
                        let c = to_shown([b[2], b[3]]);
                        [
                            a[0].min(c[0]),
                            a[1].min(c[1]),
                            a[0].max(c[0]),
                            a[1].max(c[1]),
                        ]
                    })
                    .collect(),
            }
        }
        _ => anyhow::bail!("unsupported AI mask kind"),
    })
}
pub fn load_segmenter(support: &std::path::Path) -> anyhow::Result<Box<dyn MaskSegmenter>> {
    let dir = support.join("models");
    std::fs::create_dir_all(&dir)?;
    let manifest = dir.join("models.toml");
    let text = include_str!("../../ml-runtime/models.toml");
    if std::fs::read_to_string(&manifest).ok().as_deref() != Some(text) {
        std::fs::write(&manifest, text)?;
    }
    let cache = std::env::var_os("TESSERA_SEGMENT_MODELS")
        .map(PathBuf::from)
        .unwrap_or_else(|| dir.join("cache"));
    let registry = ml_runtime::ModelRegistry::open(&manifest, &cache)?;
    let store = ml_segment::MaskStore::new(support.join("mask-cache"), 256 << 20)?;
    Ok(Box::new(ml_segment::Segmenter::load(
        &registry,
        ml_runtime::SessionOptions::default(),
        store,
    )?))
}
