use crate::{features::SceneStats, Features};
use engine_api::{EngineError, EngineResult};
/// Cached SigLIP output (ml-embed VectorIndex rows), index::faces records from
/// ml-faces, and an UNEDITED linear Rec.709 preview. Face boxes must be scaled to
/// this preview. Convert pipeline-cpu's Rec.2020 output before passing it here.
pub struct PerceptionInput<'a> {
    pub embedding_model: &'a str,
    pub embedding: &'a [f32],
    pub linear_rgb: &'a [[f32; 3]],
    pub width: u32,
    pub height: u32,
    pub faces: &'a [index::FaceRecord],
    pub as_shot_cct: f64,
    pub as_shot_duv: f64,
    pub camera: &'a str,
    pub lens: &'a str,
}
impl Features {
    pub fn from_perception(input: PerceptionInput<'_>) -> EngineResult<Self> {
        let bad =
            || EngineError::invalid("perception", "invalid preview dimensions or face geometry");
        if input.width == 0
            || input.height == 0
            || u64::from(input.width) * u64::from(input.height) != input.linear_rgb.len() as u64
        {
            return Err(bad());
        }
        let stats = SceneStats::from_linear_rgb(input.linear_rgb)?;
        let mut covered = vec![false; input.linear_rgb.len()];
        let mut sharpness = 0.;
        for face in input.faces {
            let [x, y, w, h] = face.bbox;
            if face.bbox.iter().any(|v| !v.is_finite())
                || x < 0.
                || y < 0.
                || w <= 0.
                || h <= 0.
                || x + w > input.width as f32
                || y + h > input.height as f32
                || !(0. ..=1.).contains(&face.sharpness)
            {
                return Err(bad());
            }
            for py in y.floor() as u32..(y + h).ceil() as u32 {
                for px in x.floor() as u32..(x + w).ceil() as u32 {
                    covered[(py * input.width + px) as usize] = true;
                }
            }
            sharpness += face.sharpness / input.faces.len() as f64;
        }
        let face_pixels = input
            .linear_rgb
            .iter()
            .zip(&covered)
            .filter_map(|(p, c)| c.then_some(*p))
            .collect::<Vec<_>>();
        let face_mean_luminance = if face_pixels.is_empty() {
            None
        } else {
            Some(SceneStats::from_linear_rgb(&face_pixels)?.mean_luminance)
        };
        let features = Self {
            embedding_model: input.embedding_model.into(),
            embedding: input.embedding.to_vec(),
            mean_luminance: stats.mean_luminance,
            percentiles: stats.percentiles,
            shadow_clipping: stats.shadow_clipping,
            highlight_clipping: stats.highlight_clipping,
            as_shot_cct: input.as_shot_cct,
            as_shot_duv: input.as_shot_duv,
            face_count: input.faces.len(),
            face_mean_luminance,
            face_fraction: face_pixels.len() as f64 / input.linear_rgb.len() as f64,
            face_sharpness: sharpness,
            camera: input.camera.into(),
            lens: input.lens.into(),
        };
        features.validate()?;
        Ok(features)
    }
}
