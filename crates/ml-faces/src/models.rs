use crate::{Face, align_crop, face_signals, geometry, letterbox, nms, normalize};
use anyhow::{Context, Result, ensure};
use image::RgbImage;
use ml_runtime::{ModelRegistry, SessionOptions};
use ort::{
    ep,
    session::{Session, builder::GraphOptimizationLevel},
    value::Tensor,
};

/// Reusable sessions. Resolves pinned, hash-verified weights through ml-runtime.
/// Uses ORT directly because runtime::Session::run only exposes one 4-D output.
pub struct FaceModels {
    detector: Session,
    recognizer: Session,
    /// Reasons for explicit CPU fallback, if a requested CoreML session failed.
    pub fallback_reasons: Vec<String>,
}
impl FaceModels {
    pub fn load(registry: &ModelRegistry, options: SessionOptions) -> Result<Self> {
        let mut fallback_reasons = Vec::new();
        let mut load = |id: &str| -> Result<Session> {
            let handle = registry.resolve(id)?;
            let build = |coreml: bool| -> Result<Session> {
                let mut builder = Session::builder()?
                    .with_optimization_level(GraphOptimizationLevel::Level1)
                    .map_err(ort::Error::<()>::from)?
                    .with_intra_threads(1)
                    .map_err(ort::Error::<()>::from)?;
                if coreml {
                    builder = builder
                        .with_execution_providers([
                            ep::CoreML::default()
                                .with_compute_units(options.compute_units)
                                .with_model_format(options.model_format)
                                .build()
                                .error_on_failure(),
                            ep::CPU::default().build(),
                        ])
                        .map_err(ort::Error::<()>::from)?;
                } else {
                    builder = builder
                        .with_execution_providers([ep::CPU::default().build()])
                        .map_err(ort::Error::<()>::from)?;
                }
                Ok(builder.commit_from_file(handle.path())?)
            };
            match build(options.coreml) {
                Ok(session) => Ok(session),
                Err(e) if options.coreml => {
                    fallback_reasons.push(format!("{id}: {e:#}"));
                    build(false)
                }
                Err(e) => Err(e),
            }
        };
        let detector = load("opencv/yunet")?;
        let recognizer = load("opencv/sface")?;
        Ok(Self {
            detector,
            recognizer,
            fallback_reasons,
        })
    }

    /// Detect faces and build UI metadata with the already-loaded detector.
    /// Does not embed, assign identities, write the catalog, or fetch weights.
    /// `eyes_open` remains a geometry heuristic, not a measured blink signal.
    pub fn face_strip(&mut self, image: &RgbImage) -> Result<Vec<crate::FaceChip>> {
        crate::face_strip(image, &self.detect(image)?)
    }

    pub fn detect(&mut self, image: &RgbImage) -> Result<Vec<Face>> {
        self.detect_with_thresholds(image, 0.9, 0.3)
    }
    pub fn detect_with_thresholds(
        &mut self,
        image: &RgbImage,
        confidence: f32,
        overlap: f32,
    ) -> Result<Vec<Face>> {
        ensure!(
            (0.0..=1.0).contains(&confidence) && (0.0..=1.0).contains(&overlap),
            "invalid thresholds"
        );
        let (input, mapping) = letterbox(image)?;
        let outputs = self.detector.run(ort::inputs![Tensor::from_array((
            input.shape(),
            input.data().to_vec()
        ))?])?;
        let mut faces = Vec::new();
        for stride in [8usize, 16, 32] {
            let cells = (geometry::DETECTOR_SIZE as usize / stride).pow(2);
            let get = |name: &str, channels: usize| -> Result<&[f32]> {
                let output = outputs
                    .get(format!("{name}_{stride}"))
                    .context("missing YuNet output")?;
                let (shape, values) = output.try_extract_tensor::<f32>()?;
                ensure!(
                    shape.as_ref() == [1, cells as i64, channels as i64],
                    "unexpected YuNet output shape: {shape:?}"
                );
                ensure!(
                    values.iter().all(|v| v.is_finite()),
                    "nonfinite YuNet output"
                );
                Ok(values)
            };
            let cls = get("cls", 1)?;
            let obj = get("obj", 1)?;
            let boxes = get("bbox", 4)?;
            let points = get("kps", 10)?;
            for i in 0..cells {
                let score = (cls[i].clamp(0., 1.) * obj[i].clamp(0., 1.)).sqrt();
                if score < confidence {
                    continue;
                }
                let c = (i % (geometry::DETECTOR_SIZE as usize / stride)) as f32;
                let r = (i / (geometry::DETECTOR_SIZE as usize / stride)) as f32;
                let s = stride as f32;
                let cx = (c + boxes[i * 4]) * s;
                let cy = (r + boxes[i * 4 + 1]) * s;
                let w = boxes[i * 4 + 2].exp() * s;
                let h = boxes[i * 4 + 3].exp() * s;
                ensure!(w.is_finite() && h.is_finite(), "invalid YuNet box");
                let min = mapping.unproject([cx - w / 2., cy - h / 2.]);
                let max = mapping.unproject([cx + w / 2., cy + h / 2.]);
                let x = min[0].clamp(0., image.width() as f32);
                let y = min[1].clamp(0., image.height() as f32);
                let right = max[0].clamp(0., image.width() as f32);
                let bottom = max[1].clamp(0., image.height() as f32);
                if right <= x || bottom <= y {
                    continue;
                }
                let mut landmarks5 = [[0.; 2]; 5];
                for (n, point) in landmarks5.iter_mut().enumerate() {
                    *point = mapping.unproject([
                        (c + points[i * 10 + n * 2]) * s,
                        (r + points[i * 10 + n * 2 + 1]) * s,
                    ]);
                    // Preserve predicted geometry for alignment. Faces whose landmarks
                    // are outside the image cannot support reliable face signals.
                }
                if landmarks5.iter().any(|p| {
                    p[0] < 0.
                        || p[1] < 0.
                        || p[0] >= image.width() as f32
                        || p[1] >= image.height() as f32
                }) {
                    continue;
                }
                faces.push(Face {
                    bbox: [x, y, right - x, bottom - y],
                    landmarks5,
                    score,
                });
            }
        }
        nms(faces, overlap)
    }

    /// Aligned RGB 112x112, raw 0..255. SFace includes its own normalization.
    /// The returned descriptor is L2-normalized, finite, and exactly 128 floats.
    pub fn embed(&mut self, image: &RgbImage, face: &Face) -> Result<[f32; 128]> {
        let crop = align_crop(image, face)?;
        let input = geometry::planes(&crop, false)?;
        let output = self.recognizer.run(ort::inputs![Tensor::from_array((
            input.shape(),
            input.data().to_vec()
        ))?])?;
        let (shape, values) = output
            .get("fc1")
            .context("missing SFace fc1")?
            .try_extract_tensor::<f32>()?;
        ensure!(
            shape.as_ref() == [1, 128],
            "unexpected SFace output shape: {shape:?}"
        );
        normalize(values.try_into().context("expected 128 components")?)
    }

    /// Compute everything before replacing rows. Detection with no faces clears
    /// old per-face scores, rather than leaving stale people in the catalog.
    pub fn analyze_and_store(
        &mut self,
        index: &index::Index,
        id: engine_api::id::ImageId,
        image: &RgbImage,
    ) -> Result<Vec<index::FaceRecord>> {
        index.image_info(id)?;
        let mut records = Vec::new();
        for (ordinal, face) in self.detect(image)?.iter().enumerate() {
            let signals = face_signals(image, face)?;
            records.push(index::FaceRecord {
                id: u32::try_from(ordinal)?,
                bbox: face.bbox,
                landmarks5: face.landmarks5,
                confidence: face.score,
                embedding: Some(self.embed(image, face)?.to_vec()),
                sharpness: signals.sharpness,
                eyes_open: signals.eyes_open,
            });
        }
        index.replace_faces(id, &records)?;
        index.set_score(
            id,
            &index::Score {
                signal: "faces_analyzed".into(),
                value: 1.,
                model: "yunet-sface-v1".into(),
            },
        )?;
        Ok(records)
    }
}
