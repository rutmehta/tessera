use crate::{Console, delta::delta_e_2000, pixels};
use engine_api::{
    EngineResult,
    id::ImageId,
    recipe::settings::NormalizedRect,
    tools::{CompareMetric, FaceScore, Scores},
};
use index::Query;
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Debug, Clone, Serialize)]
pub struct Metrics {
    pub mean: f32,
    pub contrast: f32,
    pub clipped_shadows: f32,
    pub clipped_highlights: f32,
}
pub struct Comparison {
    pub images: [image::RgbImage; 2],
    pub metrics: [Metrics; 2],
    pub distance: f32,
}
impl Console {
    pub fn list_images(&self, query: Option<String>) -> EngineResult<Vec<Value>> {
        self.index.search(&Query {text:query,limit:i64::MAX as usize,..Default::default()})?.into_iter().map(|id|{
            let info=self.index.image_info(id)?;
            Ok(json!({"id":id,"path":info.path,"size":info.size,"selection":self.index.selection(id)?}))
        }).collect()
    }
    pub fn describe_image(&self, image: ImageId) -> EngineResult<Value> {
        let (path, doc) = self.document(image)?;
        let info = self.index.image_info(image)?;
        let faces=self.index.faces(image)?.into_iter().map(|face|json!({"ordinal":face.id,"bbox_pixels":face.bbox,"confidence":face.confidence,"focus":face.sharpness,"eyes_open_proxy":face.eyes_open,"has_embedding":face.embedding.is_some()})).collect::<Vec<_>>();
        let stored = self
            .index
            .scores(image)?
            .into_iter()
            .map(|s| json!({"signal":s.signal,"value":s.value,"model":s.model}))
            .collect::<Vec<_>>();
        let metadata = index::MetadataProvider::read(&crate::catalog::Reader, &path)?;
        Ok(
            json!({"id":image,"path":path,"size":info.size,"capture_seconds":info.capture_seconds,"camera":metadata.camera,"lens":metadata.lens,"exif":metadata.values,"selection":doc.recipe.selection,"recipe":doc.recipe.recipe_hash(),"faces":faces,"quality":self.scores(image)?,"stored_signals":stored,"caption":{"status":"placeholder","text":null,"reason":"Embedding similarity is not a caption generator; no inferred scene description is fabricated"}}),
        )
    }
    pub(crate) fn scores(&self, image: ImageId) -> EngineResult<Scores> {
        let preview = self.render_preview(image, 1024)?;
        let measured = ml_quality::analyze(&preview)?;
        let stored = self.index.scores(image)?;
        let value = |name: &str, fallback: f32| {
            stored
                .iter()
                .find(|s| s.signal == name)
                .map_or(fallback, |s| s.value as f32)
        };
        let (path, _) = self.document(image)?;
        let (w, h) = self.previews.source_dimensions(image, &path)?;
        let faces = self
            .index
            .faces(image)?
            .iter()
            .map(|f| FaceScore {
                person: None,
                region: NormalizedRect {
                    left: f.bbox[0] / w as f32,
                    top: f.bbox[1] / h as f32,
                    right: (f.bbox[0] + f.bbox[2]) / w as f32,
                    bottom: (f.bbox[1] + f.bbox[3]) / h as f32,
                },
                focus: f.sharpness as f32,
                eyes_open: f.eyes_open.unwrap_or(0.) as f32,
            })
            .collect();
        Ok(Scores {
            focus: value("sharpness", measured.sharpness as f32),
            motion_blur: value("motion_blur", measured.motion_blur as f32),
            noise: value("noise", measured.noise as f32),
            exposure: value(
                "exposure",
                (1. - measured
                    .shadow_clipping
                    .iter()
                    .chain(&measured.highlight_clipping)
                    .sum::<f64>()
                    / 3.)
                    .clamp(0., 1.) as f32,
            ),
            aesthetic: value("aesthetic", 0.),
            faces,
            ..Default::default()
        })
    }
    pub fn compare_images(
        &self,
        a: ImageId,
        b: ImageId,
        metric: CompareMetric,
    ) -> EngineResult<Comparison> {
        let images = [self.render_preview(a, 1024)?, self.render_preview(b, 1024)?];
        let metrics = [metrics(&images[0])?, metrics(&images[1])?];
        let distance = match metric {
            CompareMetric::RecipeDiff => engine_api::recipe::history::diff(
                &serde_json::to_value(self.document(a)?.1.recipe.settings)?,
                &serde_json::to_value(self.document(b)?.1.recipe.settings)?,
            )
            .len() as f32,
            CompareMetric::Scores => (self.scores(a)?.focus - self.scores(b)?.focus).abs(),
            CompareMetric::DeltaE2000 => {
                let width = images[0].width().min(images[1].width());
                let height = images[0].height().min(images[1].height());
                let left = image::imageops::resize(
                    &images[0],
                    width,
                    height,
                    image::imageops::FilterType::Triangle,
                );
                let right = image::imageops::resize(
                    &images[1],
                    width,
                    height,
                    image::imageops::FilterType::Triangle,
                );
                (left
                    .pixels()
                    .zip(right.pixels())
                    .map(|(a, b)| delta_e_2000(lab(a), lab(b)))
                    .sum::<f64>()
                    / (f64::from(width) * f64::from(height))) as f32
            }
        };
        Ok(Comparison {
            images,
            metrics,
            distance,
        })
    }
}
pub(crate) fn metrics(rgb: &image::RgbImage) -> EngineResult<Metrics> {
    let histogram = pixels::histogram(rgb, 256)?;
    let count = (rgb.width() as f32 * rgb.height() as f32).max(1.);
    let mean = rgb.pixels().map(pixels::luma).sum::<f32>() / count;
    let contrast = (rgb
        .pixels()
        .map(|p| (pixels::luma(p) - mean).powi(2))
        .sum::<f32>()
        / count)
        .sqrt();
    Ok(Metrics {
        mean,
        contrast,
        clipped_shadows: histogram.clipped_shadows,
        clipped_highlights: histogram.clipped_highlights,
    })
}
fn lab(p: &image::Rgb<u8>) -> [f64; 3] {
    let rgb = p.0.map(|v| {
        let x = f64::from(v) / 255.;
        if x <= 0.04045 {
            x / 12.92
        } else {
            ((x + 0.055) / 1.055).powf(2.4)
        }
    });
    let xyz = [
        [0.4124564, 0.3575761, 0.1804375],
        [0.2126729, 0.7151522, 0.0721750],
        [0.0193339, 0.1191920, 0.9503041],
    ]
    .map(|row| row.iter().zip(rgb).map(|(a, b)| a * b).sum::<f64>());
    let f = |t: f64| {
        if t > 216. / 24389. {
            t.cbrt()
        } else {
            (24389. / 27. * t + 16.) / 116.
        }
    };
    let [x, y, z] = [f(xyz[0] / 0.95047), f(xyz[1]), f(xyz[2] / 1.08883)];
    [116. * y - 16., 500. * (x - y), 200. * (y - z)]
}
