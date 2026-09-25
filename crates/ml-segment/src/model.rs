use crate::{MaskRaster, MaskStore, refine};
use anyhow::{Context, Result, ensure};
use engine_api::id::ModelRef;
use image::{RgbImage, imageops};
use ml_runtime::{
    ModelRegistry, PartitionReport, Session, SessionOptions, TensorInput, TensorOutput,
};
use serde::Serialize;

pub const SUBJECT_VERSION: &str = "7fc34deee10329bc039c10a73b98090d0c6f5c59";
pub const SAM_VERSION: &str = "5050a79cd4b912dd745fff83047c4ef6fbd97be5";
const HASHES: [&str; 3] = [
    "8d10d2f3bb75ae3b6d527c77944fc5e7dcd94b29809d47a739a7a728a912b491",
    "8c1494f7dc70b61b15bc7ab8e4291804d47294d881da137b099b04c1776535dd",
    "23c11087a0c1930d863ba37fdf0f2b0080f5a94269191f679ffd16b8415f0a38",
];
#[derive(Clone, Debug, Serialize)]
pub struct Click {
    pub point: [f32; 2],
    pub positive: bool,
}
/// Normalized image coordinates. Boxes are [left, top, right, bottom]. Multiple
/// boxes are independent objects, unioned with max, not one malformed SAM box.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Prompts {
    pub clicks: Vec<Click>,
    pub boxes: Vec<[f32; 4]>,
}
impl Prompts {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.clicks.is_empty() || !self.boxes.is_empty(),
            "empty prompts"
        );
        ensure!(
            self.clicks.len() <= 256 && self.boxes.len() <= 32,
            "too many prompts"
        );
        let valid = |v: &f32| v.is_finite() && (0.0..=1.0).contains(v);
        ensure!(
            self.clicks.iter().all(|p| p.point.iter().all(valid)),
            "invalid click"
        );
        ensure!(
            self.boxes
                .iter()
                .all(|b| b.iter().all(valid) && b[0] < b[2] && b[1] < b[3]),
            "invalid box"
        );
        Ok(())
    }
}
/// Face-crate pixel [x,y,width,height] to a conservative full-person box.
/// This is a geometric proxy, not human-part parsing.
pub fn person_box(image: &RgbImage, face: [f32; 4]) -> Result<[f32; 4]> {
    validate_image(image)?;
    let [x, y, w, h] = face;
    ensure!(
        face.iter().all(|v| v.is_finite()) && w > 0. && h > 0.,
        "invalid face box"
    );
    ensure!(
        x < image.width() as f32 && y < image.height() as f32 && x + w > 0. && y + h > 0.,
        "face outside image"
    );
    let out = [
        (x - w) / (image.width() as f32),
        (y - 0.5 * h) / (image.height() as f32),
        (x + 2. * w) / (image.width() as f32),
        (y + 7. * h) / (image.height() as f32),
    ]
    .map(|v| v.clamp(0., 1.));
    ensure!(
        out.iter().all(|v| v.is_finite()) && out[0] < out[2] && out[1] < out[3],
        "invalid expanded face"
    );
    Ok(out)
}

pub struct Segmenter {
    subject: Session,
    encoder: Session,
    decoder: Session,
    cache: MaskStore,
    embedding: Option<([u8; 32], Vec<f32>)>,
    pub(crate) cancellation: Option<engine_api::jobs::CancellationToken>,
}
impl Segmenter {
    /// Explicit load may fetch missing weights. Inference itself never downloads.
    pub fn load(
        registry: &ModelRegistry,
        options: SessionOptions,
        cache: MaskStore,
    ) -> Result<Self> {
        let mut sessions = Vec::new();
        for (i, (id, version)) in [
            ("segment/u2net", SUBJECT_VERSION),
            ("segment/sam-encoder", SAM_VERSION),
            ("segment/sam-decoder", SAM_VERSION),
        ]
        .iter()
        .enumerate()
        {
            let model = registry.resolve_ref(&ModelRef {
                id: (*id).into(),
                version: (*version).into(),
            })?;
            ensure!(
                model.spec().sha256 == HASHES[i],
                "unexpected weights for {id}"
            );
            sessions.push(Session::load(model.path(), options.clone())?);
        }
        let decoder = sessions.pop().unwrap();
        let encoder = sessions.pop().unwrap();
        let subject = sessions.pop().unwrap();
        Ok(Self {
            subject,
            encoder,
            decoder,
            cache,
            embedding: None,
            cancellation: None,
        })
    }
    pub fn subject(&mut self, image: &RgbImage, level: u8) -> Result<MaskRaster> {
        let guide = level_image(image, level)?;
        let key = cache_key(
            image,
            "subject",
            SUBJECT_VERSION,
            &Prompts::default(),
            level,
        )?;
        if let Some(mask) = self.cache.get(&key) {
            return Ok(mask);
        }
        let native_key = cache_key(
            image,
            "subject-native",
            SUBJECT_VERSION,
            &Prompts::default(),
            0,
        )?;
        let low = if let Some(mask) = self.cache.get(&native_key) {
            mask
        } else {
            let small = imageops::resize(image, 320, 320, imageops::FilterType::Triangle);
            let mut data = Vec::with_capacity(3 * 320 * 320);
            for c in 0..3 {
                data.extend(small.pixels().map(|p| {
                    (p[c] as f32 / 255. - [0.485, 0.456, 0.406][c]) / [0.229, 0.224, 0.225][c]
                }));
            }
            let output = take(
                self.subject
                    .run_tensors(&[("input.1", tensor(&[1, 3, 320, 320], data))])?,
                "1959",
                &[1, 1, 320, 320],
            )?;
            let low = MaskRaster::new(320, 320, output)?;
            self.persist(&native_key, &low)?;
            low
        };
        let out = refine(&low, &guide, 8, 0.0001)?;
        self.persist(&key, &out)?;
        Ok(out)
    }
    pub fn background(&mut self, image: &RgbImage, level: u8) -> Result<MaskRaster> {
        // Invert after refinement, so the partition is exactly complementary.
        let mask = self.subject(image, level)?.inverted();
        self.persist(
            &cache_key(
                image,
                "background",
                SUBJECT_VERSION,
                &Prompts::default(),
                level,
            )?,
            &mask,
        )?;
        Ok(mask)
    }
    /// Explicit phase-one heuristic, NOT a semantic sky network. Top-connected
    /// blue prior multiplied by U2Net inverse suppresses foreground objects.
    pub fn sky(&mut self, image: &RgbImage, level: u8) -> Result<MaskRaster> {
        let guide = level_image(image, level)?;
        let key = cache_key(
            image,
            "sky-blue-connected-v1",
            SUBJECT_VERSION,
            &Prompts::default(),
            level,
        )?;
        if let Some(mask) = self.cache.get(&key) {
            return Ok(mask);
        }
        let subject = self.subject(image, level)?;
        let out = sky_prior(&guide, &subject)?;
        let out = refine(&out, &guide, 8, 0.0001)?;
        self.persist(&key, &out)?;
        Ok(out)
    }
    pub fn person(
        &mut self,
        image: &RgbImage,
        face_box: [f32; 4],
        level: u8,
    ) -> Result<MaskRaster> {
        self.promptable(
            image,
            &Prompts {
                clicks: vec![],
                boxes: vec![person_box(image, face_box)?],
            },
            level,
        )
    }
    pub fn promptable(
        &mut self,
        image: &RgbImage,
        prompts: &Prompts,
        level: u8,
    ) -> Result<MaskRaster> {
        prompts.validate()?;
        let guide = level_image(image, level)?;
        let key = cache_key(image, "sam", SAM_VERSION, prompts, level)?;
        if let Some(mask) = self.cache.get(&key) {
            return Ok(mask);
        }
        let hash = image_hash(image);
        let scale = 1024. / image.width().max(image.height()) as f64;
        let w = (image.width() as f64 * scale).round().max(1.) as u32;
        let h = (image.height() as f64 * scale).round().max(1.) as u32;
        if self.embedding.as_ref().is_none_or(|(key, _)| *key != hash) {
            let small = imageops::resize(image, w, h, imageops::FilterType::Triangle);
            let out = self.encoder.run_tensors(&[(
                "input_image",
                tensor(
                    &[h as usize, w as usize, 3],
                    small.as_raw().iter().map(|&v| v as f32).collect(),
                ),
            )])?;
            self.embedding = Some((hash, take(out, "image_embeddings", &[1, 256, 64, 64])?));
        }
        // Decode at encoder-resolution aspect ratio, bounded to 1024², then
        // guided-upsample rather than allocating original-resolution logits.
        let mut result = vec![0f32; w as usize * h as usize];
        let boxes: Vec<_> = if prompts.boxes.is_empty() {
            vec![None]
        } else {
            prompts.boxes.iter().map(Some).collect()
        };
        for b in boxes {
            let mut coords = Vec::new();
            let mut labels = Vec::new();
            for click in &prompts.clicks {
                coords.extend([click.point[0] * w as f32, click.point[1] * h as f32]);
                labels.push(if click.positive { 1. } else { 0. });
            }
            if let Some(b) = b {
                coords.extend([
                    b[0] * w as f32,
                    b[1] * h as f32,
                    b[2] * w as f32,
                    b[3] * h as f32,
                ]);
                labels.extend([2., 3.]);
            } else {
                coords.extend([0., 0.]);
                labels.push(-1.);
            }
            let n = labels.len();
            let outputs = self.decoder.run_tensors(&[
                (
                    "image_embeddings",
                    tensor(
                        &[1, 256, 64, 64],
                        self.embedding.as_ref().unwrap().1.clone(),
                    ),
                ),
                ("point_coords", tensor(&[1, n, 2], coords)),
                ("point_labels", tensor(&[1, n], labels)),
                ("mask_input", tensor(&[1, 1, 256, 256], vec![0.; 256 * 256])),
                ("has_mask_input", tensor(&[1], vec![0.])),
                ("orig_im_size", tensor(&[2], vec![h as f32, w as f32])),
            ])?;
            let data = take(outputs, "masks", &[1, 1, h as usize, w as usize])?;
            for (a, v) in result.iter_mut().zip(data) {
                *a = a.max(1. / (1. + (-v).exp()));
            }
        }
        let out = refine(&MaskRaster::new(w, h, result)?, &guide, 8, 0.0001)?;
        self.persist(&key, &out)?;
        Ok(out)
    }
    fn persist(&self, key: &[u8; 32], mask: &MaskRaster) -> Result<()> {
        if let Some(token) = &self.cancellation {
            token.check()?;
        }
        Ok(self.cache.put(key, mask)?)
    }
    /// Finalizes profiling snapshots after representative execution.
    pub fn partition_reports(&mut self) -> Result<Vec<(&'static str, PartitionReport)>> {
        Ok(vec![
            ("u2net", self.subject.partition_report()?),
            ("sam-encoder", self.encoder.partition_report()?),
            ("sam-decoder", self.decoder.partition_report()?),
        ])
    }
}
fn tensor(shape: &[usize], data: Vec<f32>) -> TensorInput {
    TensorInput::F32 {
        shape: shape.to_vec(),
        data,
    }
}
fn take(outputs: Vec<TensorOutput>, name: &str, shape: &[usize]) -> Result<Vec<f32>> {
    let output = outputs
        .into_iter()
        .find(|o| o.name == name)
        .with_context(|| format!("missing {name}"))?;
    ensure!(
        output.shape == shape
            && output.data.len() == shape.iter().product::<usize>()
            && output.data.iter().all(|v| v.is_finite()),
        "invalid {name} output"
    );
    Ok(output.data)
}
fn validate_image(image: &RgbImage) -> Result<()> {
    ensure!(image.width() > 0 && image.height() > 0, "empty image");
    Ok(())
}
fn level_image(image: &RgbImage, level: u8) -> Result<RgbImage> {
    validate_image(image)?;
    ensure!(level <= 8, "level must be 0..=8");
    if level == 0 {
        return Ok(image.clone());
    }
    Ok(imageops::resize(
        image,
        (image.width() >> level).max(1),
        (image.height() >> level).max(1),
        imageops::FilterType::Triangle,
    ))
}
pub fn image_hash(image: &RgbImage) -> [u8; 32] {
    let mut hash = blake3::Hasher::new();
    hash.update(&image.width().to_le_bytes());
    hash.update(&image.height().to_le_bytes());
    hash.update(image.as_raw());
    *hash.finalize().as_bytes()
}
/// Includes pixel dimensions/content, algorithm revision, model ID/version,
/// prompts and output level. Refinement is fixed by the algorithm revision.
pub fn cache_key(
    image: &RgbImage,
    kind: &str,
    version: &str,
    prompts: &Prompts,
    level: u8,
) -> Result<[u8; 32]> {
    let data = serde_json::to_vec(&(
        "segment-rgb-v1-guided8-e1e-4",
        image_hash(image),
        kind,
        version,
        prompts,
        level,
    ))?;
    Ok(*blake3::hash(&data).as_bytes())
}
pub fn sky_prior(image: &RgbImage, subject: &MaskRaster) -> Result<MaskRaster> {
    ensure!(
        image.dimensions() == (subject.width(), subject.height()),
        "sky dimensions differ"
    );
    validate_image(image)?;
    let (w, h) = (image.width() as usize, image.height() as usize);
    let prior: Vec<f32> = image
        .pixels()
        .map(|p| ((p[2] as f32 - p[0].max(p[1]) as f32) / 40.).clamp(0., 1.))
        .collect();
    let mut selected = vec![false; w * h];
    let mut stack = Vec::new();
    for x in 0..w {
        if prior[x] > 0.1 {
            selected[x] = true;
            stack.push(x);
        }
    }
    while let Some(i) = stack.pop() {
        let x = i % w;
        let y = i / w;
        for n in [
            if x > 0 { Some(i - 1) } else { None },
            if x + 1 < w { Some(i + 1) } else { None },
            if y > 0 { Some(i - w) } else { None },
            if y + 1 < h { Some(i + w) } else { None },
        ]
        .into_iter()
        .flatten()
        {
            if !selected[n] && prior[n] > 0.1 {
                selected[n] = true;
                stack.push(n);
            }
        }
    }
    Ok(MaskRaster::new(
        w as u32,
        h as u32,
        (0..w * h)
            .map(|i| {
                if selected[i] {
                    prior[i] * (1. - subject.data()[i])
                } else {
                    0.
                }
            })
            .collect(),
    )?)
}
