use anyhow::{Context, Result, ensure};
use engine_api::id::ModelRef;
use image::RgbImage;
use ml_runtime::{ModelRegistry, Session, SessionOptions, TensorInput, TensorOutput};
use sha2::{Digest, Sha256};
use std::path::Path;
use tokenizers::{PaddingParams, PaddingStrategy, Tokenizer, TruncationParams};

pub const DIMENSION: usize = 768;
pub const REVISION: &str = "4649052661e53c7000355844105f8a1792088239";
/// Includes preprocessing/tokenization revision as well as both pinned towers.
pub const MODEL_VERSION: &str =
    "siglip-base-patch16-224/4649052661e53c7000355844105f8a1792088239/rgb224-eos64-v1";
pub const TOKENIZER_SHA256: &str =
    "4a17c975210be5ab4c36b47d8dae4eefb866dbfb1e676e394aad85dc30a3ae08";

// Fixed input bounds allow CoreML to fold shape operations during partitioning.
const IMAGE_BATCH: usize = 4;

pub struct Siglip {
    vision: Session,
    text: Session,
    tokenizer: Tokenizer,
}
impl Siglip {
    /// Explicit model resolution may download on a cache miss. The tokenizer is
    /// installed separately by tools/fetch_siglip.py and verified before use.
    /// Uses CoreML's NeuralNetwork format for this export regardless of
    /// `options.model_format`; `coreml` and `compute_units` remain caller-selected.
    pub fn load(
        registry: &ModelRegistry,
        tokenizer: &Path,
        options: SessionOptions,
    ) -> Result<Self> {
        let bytes = std::fs::read(tokenizer)?;
        ensure!(
            format!("{:x}", Sha256::digest(&bytes)) == TOKENIZER_SHA256,
            "tokenizer SHA-256 mismatch"
        );
        let mut tokenizer = Tokenizer::from_bytes(bytes).map_err(|e| anyhow::anyhow!(e))?;
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: 64,
                ..Default::default()
            }))
            .map_err(|e| anyhow::anyhow!(e))?;
        tokenizer.with_padding(Some(PaddingParams {
            strategy: PaddingStrategy::Fixed(64),
            pad_id: 1,
            pad_token: "</s>".into(),
            ..Default::default()
        }));
        let vision = registry.resolve_ref(&ModelRef {
            id: "siglip/vision".into(),
            version: REVISION.into(),
        })?;
        let text = registry.resolve_ref(&ModelRef {
            id: "siglip/text".into(),
            version: REVISION.into(),
        })?;
        // Enforce the weights expected by this preprocessing contract even with a custom manifest.
        ensure!(
            vision.spec().sha256
                == "f89d41bac7f4d4b87e010a467d93f98689d708916ed22f5a07f96fdfa26f475f",
            "unexpected vision model"
        );
        ensure!(
            text.spec().sha256
                == "3aa7fdbd20eaa8740cce17bf82913de641fcb632a768fed59f661cdcd0c32553",
            "unexpected text model"
        );
        // This pinned export fails Apple's MLProgram compiler even with static
        // bounds (vision then falls entirely back to CPU). NeuralNetwork executes
        // both towers through CoreML, with unsupported ops remaining on CPU.
        // Keep the general runtime's caller-selected model format unchanged.
        let options = SessionOptions {
            model_format: ml_runtime::ModelFormat::NeuralNetwork,
            ..options
        };
        Ok(Self {
            vision: Session::load_with_dimensions(
                vision.path(),
                options.clone(),
                &[
                    ("batch_size", IMAGE_BATCH as i64),
                    ("num_channels", 3),
                    ("height", 224),
                    ("width", 224),
                ],
            )?,
            text: Session::load_with_dimensions(
                text.path(),
                options,
                &[("batch_size", 1), ("sequence_length", 64)],
            )?,
            tokenizer,
        })
    }
    pub fn tokenize(&self, query: &str) -> Result<Vec<i64>> {
        ensure!(!query.trim().is_empty(), "empty semantic query");
        // SiglipTokenizer's do_lower_case is outside tokenizer.json's normalizer.
        let encoding = self
            .tokenizer
            .encode(query.trim().to_lowercase(), true)
            .map_err(|e| anyhow::anyhow!(e))?;
        ensure!(encoding.len() == 64, "expected 64 SigLIP tokens");
        Ok(encoding.get_ids().iter().map(|&id| i64::from(id)).collect())
    }
    pub fn embed_images(&mut self, previews: &[RgbImage]) -> Result<Vec<[f32; DIMENSION]>> {
        ensure!(!previews.is_empty(), "empty image batch");
        let mut embeddings = Vec::with_capacity(previews.len());
        for chunk in previews.chunks(IMAGE_BATCH) {
            let mut data = crate::preprocess(chunk)?;
            // Zero is a valid normalized pixel. Padding rows are independent and
            // never escape this method; preprocessing memory stays bounded.
            data.resize(IMAGE_BATCH * 3 * 224 * 224, 0.0);
            let outputs = self.vision.run_tensors(&[(
                "pixel_values",
                TensorInput::F32 {
                    shape: vec![IMAGE_BATCH, 3, 224, 224],
                    data,
                },
            )])?;
            embeddings.extend(pooled(outputs, IMAGE_BATCH)?.into_iter().take(chunk.len()));
        }
        Ok(embeddings)
    }
    pub fn embed_image(&mut self, preview: &RgbImage) -> Result<[f32; DIMENSION]> {
        Ok(self.embed_images(std::slice::from_ref(preview))?.remove(0))
    }
    pub fn embed_text(&mut self, query: &str) -> Result<[f32; DIMENSION]> {
        let data = self.tokenize(query)?;
        let outputs = self.text.run_tensors(&[(
            "input_ids",
            TensorInput::I64 {
                shape: vec![1, 64],
                data,
            },
        )])?;
        Ok(pooled(outputs, 1)?.remove(0))
    }
    /// Actual provider assignments, not a claim that every op was accelerated.
    pub fn partition_reports(
        &mut self,
    ) -> Result<(ml_runtime::PartitionReport, ml_runtime::PartitionReport)> {
        Ok((
            self.vision.partition_report()?,
            self.text.partition_report()?,
        ))
    }
}
fn pooled(outputs: Vec<TensorOutput>, batch: usize) -> Result<Vec<[f32; DIMENSION]>> {
    let output = outputs
        .into_iter()
        .find(|o| o.name == "pooler_output")
        .context("missing pooler_output")?;
    ensure!(
        output.shape == [batch, DIMENSION] && output.data.len() == batch * DIMENSION,
        "invalid embedding output shape"
    );
    output
        .data
        .as_chunks::<DIMENSION>()
        .0
        .iter()
        .map(|row| {
            ensure!(row.iter().all(|v| v.is_finite()), "nonfinite embedding");
            let norm = row
                .iter()
                .map(|&v| f64::from(v).powi(2))
                .sum::<f64>()
                .sqrt();
            ensure!(norm > 0.0, "zero embedding");
            let mut vector = [0.; DIMENSION];
            for (out, &v) in vector.iter_mut().zip(row) {
                *out = (f64::from(v) / norm) as f32;
            }
            Ok(vector)
        })
        .collect()
}
