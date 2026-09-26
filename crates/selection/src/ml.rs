//! Object / Subject / Sky selection through a segmentation model.
//!
//! [`SegmentModel`] abstracts the network; with the `ml` feature
//! [`MlSegmenter`] adapts `ml_segment::Segmenter` (U²-Net subject, SAM
//! promptable, sky prior). Model masks may come at any resolution; they are
//! resampled to the image and refined with a luminance-guided filter.

use engine_api::{EngineError, EngineResult};

use crate::filter::guided_filter;
use crate::marquee::polygon;
use crate::mask::{Image, Mask};
use crate::ops;

/// Object Selection prompt, in image pixels.
#[derive(Debug, Clone, PartialEq)]
pub enum ObjectPrompt {
    /// Rectangle mode `[left, top, right, bottom]`.
    Box([f32; 4]),
    /// Clicks `(point, positive)`.
    Points(Vec<([f32; 2], bool)>),
    /// Lasso mode: the object inside the outline.
    Lasso(Vec<[f32; 2]>),
}

/// A segmentation network.
pub trait SegmentModel {
    /// Salient subject.
    fn subject(&mut self, img: &Image) -> EngineResult<Mask>;
    /// Sky.
    fn sky(&mut self, img: &Image) -> EngineResult<Mask>;
    /// Object for a box or click prompt (never `Lasso`; that is reduced to a
    /// box by [`select_object`]).
    fn object(&mut self, img: &Image, prompt: &ObjectPrompt) -> EngineResult<Mask>;
}

/// Bilinear resample of a mask to `w × h` (pixel-centre aligned).
pub fn resample(m: &Mask, w: u32, h: u32) -> Mask {
    if m.width() == w && m.height() == h {
        return m.clone();
    }
    let (sx, sy) = (m.width() as f32 / w as f32, m.height() as f32 / h as f32);
    Mask::from_fn(w, h, |x, y| {
        let fx = ((x as f32 + 0.5) * sx - 0.5).clamp(0.0, m.width() as f32 - 1.0);
        let fy = ((y as f32 + 0.5) * sy - 0.5).clamp(0.0, m.height() as f32 - 1.0);
        let (x0, y0) = (fx.floor() as i64, fy.floor() as i64);
        let (ax, ay) = (fx - x0 as f32, fy - y0 as f32);
        let x1 = (x0 + 1).min(i64::from(m.width()) - 1);
        let y1 = (y0 + 1).min(i64::from(m.height()) - 1);
        let a = m.get(x0, y0) + (m.get(x1, y0) - m.get(x0, y0)) * ax;
        let b = m.get(x0, y1) + (m.get(x1, y1) - m.get(x0, y1)) * ax;
        a + (b - a) * ay
    })
}

fn finish(m: Mask, img: &Image, refine_radius: usize) -> EngineResult<Mask> {
    if m.width() == 0 || m.height() == 0 {
        return Err(EngineError::internal("segmentation returned an empty mask"));
    }
    let m = resample(&m, img.width, img.height);
    if refine_radius == 0 {
        return Ok(m);
    }
    let lum = img.luminance();
    let (w, h) = (img.width as usize, img.height as usize);
    let d = guided_filter(m.data(), &lum, w, h, refine_radius, 1e-3)
        .into_iter()
        .map(|v| v.clamp(0.0, 1.0))
        .collect();
    Mask::from_vec(img.width, img.height, d)
}

/// Select > Subject.
pub fn select_subject(
    model: &mut dyn SegmentModel,
    img: &Image,
    refine_radius: usize,
) -> EngineResult<Mask> {
    finish(model.subject(img)?, img, refine_radius)
}

/// Select > Sky.
pub fn select_sky(
    model: &mut dyn SegmentModel,
    img: &Image,
    refine_radius: usize,
) -> EngineResult<Mask> {
    finish(model.sky(img)?, img, refine_radius)
}

/// Object Selection tool. Lasso prompts query the lasso's bounding box and
/// keep the result inside the outline (grown by 8 px).
pub fn select_object(
    model: &mut dyn SegmentModel,
    img: &Image,
    prompt: &ObjectPrompt,
    refine_radius: usize,
) -> EngineResult<Mask> {
    match prompt {
        ObjectPrompt::Lasso(pts) => {
            if pts.len() < 3 {
                return Err(EngineError::invalid("prompt", "lasso needs 3 points"));
            }
            let b = pts.iter().fold(
                [
                    f32::INFINITY,
                    f32::INFINITY,
                    f32::NEG_INFINITY,
                    f32::NEG_INFINITY,
                ],
                |b, p| {
                    [
                        b[0].min(p[0]),
                        b[1].min(p[1]),
                        b[2].max(p[0]),
                        b[3].max(p[1]),
                    ]
                },
            );
            let m = finish(
                model.object(img, &ObjectPrompt::Box(b))?,
                img,
                refine_radius,
            )?;
            let region = ops::grow(&polygon(img.width, img.height, pts, true), 8.0);
            ops::combine(&m, &region, ops::Combine::Intersect)
        }
        p => finish(model.object(img, p)?, img, refine_radius),
    }
}

#[cfg(feature = "ml")]
pub use adapter::MlSegmenter;

#[cfg(feature = "ml")]
mod adapter {
    use super::*;
    use ml_segment::{Click, MaskRaster, Prompts, Segmenter};

    /// `ml_segment::Segmenter` as a [`SegmentModel`].
    pub struct MlSegmenter {
        /// The loaded models.
        pub inner: Segmenter,
        /// Pyramid level the masks are computed at.
        pub level: u8,
    }

    fn rgb(img: &Image) -> EngineResult<image::RgbImage> {
        let raw = img
            .data
            .iter()
            .flat_map(|p| [p[0], p[1], p[2]].map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8))
            .collect();
        image::RgbImage::from_raw(img.width, img.height, raw)
            .ok_or_else(|| EngineError::invalid("image", "size"))
    }

    fn mask(m: MaskRaster) -> EngineResult<Mask> {
        Mask::from_vec(m.width(), m.height(), m.data().to_vec())
    }

    fn ml(e: impl std::fmt::Display) -> EngineError {
        EngineError::internal(format!("segmentation: {e:#}"))
    }

    impl SegmentModel for MlSegmenter {
        fn subject(&mut self, img: &Image) -> EngineResult<Mask> {
            mask(self.inner.subject(&rgb(img)?, self.level).map_err(ml)?)
        }
        fn sky(&mut self, img: &Image) -> EngineResult<Mask> {
            mask(self.inner.sky(&rgb(img)?, self.level).map_err(ml)?)
        }
        fn object(&mut self, img: &Image, prompt: &ObjectPrompt) -> EngineResult<Mask> {
            let (w, h) = (img.width as f32, img.height as f32);
            let n = |p: [f32; 2]| [(p[0] / w).clamp(0.0, 1.0), (p[1] / h).clamp(0.0, 1.0)];
            let prompts = match prompt {
                ObjectPrompt::Box(b) => {
                    let (a, c) = (n([b[0], b[1]]), n([b[2], b[3]]));
                    Prompts {
                        clicks: vec![],
                        boxes: vec![[a[0], a[1], c[0], c[1]]],
                    }
                }
                ObjectPrompt::Points(ps) => Prompts {
                    clicks: ps
                        .iter()
                        .map(|(p, positive)| Click {
                            point: n(*p),
                            positive: *positive,
                        })
                        .collect(),
                    boxes: vec![],
                },
                ObjectPrompt::Lasso(_) => {
                    return Err(EngineError::invalid(
                        "prompt",
                        "lasso is reduced to a box first",
                    ));
                }
            };
            mask(
                self.inner
                    .promptable(&rgb(img)?, &prompts, self.level)
                    .map_err(ml)?,
            )
        }
    }
}
