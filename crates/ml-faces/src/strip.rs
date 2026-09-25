use crate::{Face, face_signals};
use anyhow::Result;
use image::RgbImage;

/// Per-face UI metadata; pixels remain in the caller's source image.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceChip {
    /// Clipped source-image pixel rectangle [x, y, width, height].
    /// Fractional bounds round outward; width and height are nonzero.
    pub crop_rect: [u32; 4],
    /// Normalized crop sharpness in [0, 1], higher is sharper.
    pub focus_score: f64,
    /// Weak landmark-geometry heuristic, NOT eyelid aperture or blink probability.
    /// None means degenerate geometry. Never use as an automatic blink label.
    pub eyes_open: Option<f64>,
    /// Persistent identity, if assigned by the caller; never a detection ordinal.
    pub person_id: Option<String>,
}

/// Build UI chips from supplied detections without loading or downloading models.
/// Preserves input order and leaves person identities unassigned. Empty detections
/// yield an empty strip for a nonempty image. Empty images, invalid detections,
/// and boxes wholly outside the image return an error (no partial strip).
/// Chips reference the same unaligned, clipped crops used to measure focus.
pub fn face_strip(image: &RgbImage, faces: &[Face]) -> Result<Vec<FaceChip>> {
    anyhow::ensure!(image.width() > 0 && image.height() > 0, "empty image");
    faces
        .iter()
        .map(|face| {
            let signals = face_signals(image, face)?;
            Ok(FaceChip {
                crop_rect: crate::geometry::crop_rect(image.dimensions(), face)?,
                focus_score: signals.sharpness,
                eyes_open: signals.eyes_open,
                person_id: None,
            })
        })
        .collect()
}

/// Read cached signals without running models. Dimensions describe the preview
/// coordinate space used at detection time, not necessarily the original RAW.
/// The host resolves its person identity from a face/descriptor. Local face IDs
/// are not person IDs and must not be reused as identities across images.
pub fn face_strip_from_index(
    index: &index::Index,
    image: engine_api::id::ImageId,
    dimensions: (u32, u32),
    person: impl Fn(engine_api::id::ImageId, &index::FaceRecord) -> Option<String>,
) -> Result<Vec<FaceChip>> {
    anyhow::ensure!(dimensions.0 > 0 && dimensions.1 > 0, "empty image");
    index.image_info(image)?;
    index
        .faces(image)?
        .iter()
        .map(|record| {
            let face = Face {
                bbox: record.bbox,
                landmarks5: record.landmarks5,
                score: record.confidence,
            };
            Ok(FaceChip {
                crop_rect: crate::geometry::crop_rect(dimensions, &face)?,
                focus_score: record.sharpness,
                eyes_open: record.eyes_open,
                person_id: person(image, record),
            })
        })
        .collect()
}

/// Review-only per-person filter for low eyes-open proxy scores. This is NOT a
/// verified blink detector. Unknown identities/eyes are excluded; ties are not
/// closed. Returns unique image IDs in input order, without Selection writes.
pub fn frames_with_person_eyes_closed(
    index: &index::Index,
    images: &[engine_api::id::ImageId],
    person_id: &str,
    below: f64,
    person: impl Fn(engine_api::id::ImageId, &index::FaceRecord) -> Option<String>,
) -> Result<Vec<engine_api::id::ImageId>> {
    anyhow::ensure!(
        !person_id.is_empty() && (0. ..=1.).contains(&below),
        "invalid person or threshold"
    );
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for &image in images {
        if !seen.insert(image) {
            continue;
        }
        index.image_info(image)?;
        if index.faces(image)?.iter().any(|f| {
            f.eyes_open.is_some_and(|v| v < below) && person(image, f).as_deref() == Some(person_id)
        }) {
            result.push(image);
        }
    }
    Ok(result)
}
