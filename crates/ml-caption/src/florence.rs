use anyhow::{Context, Result, ensure};
use image::RgbImage;
use ml_runtime::{
    ModelRegistry, PartitionReport, Session, SessionOptions, TensorInput, TensorOutput,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
use tokenizers::Tokenizer;

pub const FLORENCE_REVISION: &str = "e88a44eaf3791a35eae0c5a47b3dbcd36e67eb6f";
pub const FLORENCE_VERSION: &str =
    "florence-2-base-ft/e88a44eaf3791a35eae0c5a47b3dbcd36e67eb6f/fp16-rgb768-greedy-v1";
const TOKENIZER_HASH: &str = "d69dcdb2323e124ac4f800cb9863ddccea0d7bb11e16125e8df3bd60f2f8aeac";
const MODELS: [(&str, &str); 4] = [
    (
        "vision",
        "a7abcd77199c5d0089cf985ede4dd8089acd84f30fb3fb1462d5930345c688b3",
    ),
    (
        "embed",
        "da2607930eea5e21e4a2bd5fd069de550f1acc30316a4e8f824551a95232ba39",
    ),
    (
        "encoder",
        "0d1d929f282963e983b8ac5ac4957f19a8fa48233eab41166951b769e5cf2fd2",
    ),
    (
        "decoder",
        "ce583853b630f230eaa1ef201e35001cdda968c84749d67c18ce3707171cfa0c",
    ),
];
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Caption {
    pub caption: String,
    pub alt_text: String,
}

pub struct Florence {
    vision: Session,
    embed: Session,
    encoder: Session,
    decoder: Session,
    tokenizer: Tokenizer,
}
impl Florence {
    /// Existence check only, used to skip offline tests. Load verifies all hashes.
    pub fn is_cached(cache: &Path) -> bool {
        cache.join("florence-tokenizer.json").is_file()
            && MODELS
                .iter()
                .all(|(_, hash)| cache.join(format!("{hash}.onnx")).is_file())
    }
    /// Explicit resolution can download missing weights. Tokenizer is installed
    /// by tools/fetch_florence.py. No Python or remote code executes in inference.
    pub fn load(
        registry: &ModelRegistry,
        tokenizer: &Path,
        options: SessionOptions,
    ) -> Result<Self> {
        let bytes = std::fs::read(tokenizer)?;
        ensure!(
            format!("{:x}", Sha256::digest(&bytes)) == TOKENIZER_HASH,
            "Florence tokenizer SHA-256 mismatch"
        );
        let tokenizer = Tokenizer::from_bytes(bytes).map_err(|e| anyhow::anyhow!(e))?;
        let mut sessions = Vec::new();
        for (name, hash) in MODELS {
            let id = format!("caption/florence-{name}");
            let spec = registry
                .models()
                .iter()
                .find(|s| s.id == id && s.version == FLORENCE_REVISION)
                .context("missing pinned Florence model")?;
            ensure!(spec.sha256 == hash, "unexpected Florence model hash");
            let handle = registry.resolve_ref(&engine_api::id::ModelRef {
                id: id.as_str().into(),
                version: FLORENCE_REVISION.into(),
            })?;
            let dims: &[(&str, i64)] = if name == "vision" {
                &[("batch_size", 1), ("height", 768), ("width", 768)]
            } else {
                &[("batch_size", 1)]
            };
            sessions.push(Session::load_with_dimensions(
                handle.path(),
                options.clone(),
                dims,
            )?);
        }
        let mut sessions = sessions.into_iter();
        Ok(Self {
            vision: sessions.next().unwrap(),
            embed: sessions.next().unwrap(),
            encoder: sessions.next().unwrap(),
            decoder: sessions.next().unwrap(),
            tokenizer,
        })
    }
    fn visual(&mut self, image: &RgbImage) -> Result<TensorOutput> {
        ensure!(image.width() > 0 && image.height() > 0, "empty image");
        let image =
            image::imageops::resize(image, 768, 768, image::imageops::FilterType::CatmullRom);
        let mut pixels = Vec::with_capacity(3 * 768 * 768);
        for (c, (mean, std)) in [(0.485, 0.229), (0.456, 0.224), (0.406, 0.225)]
            .into_iter()
            .enumerate()
        {
            pixels.extend(
                image
                    .pixels()
                    .map(|p| (f32::from(p[c]) / 255. - mean) / std),
            );
        }
        let output = named(
            self.vision.run_tensors(&[(
                "pixel_values",
                TensorInput::F32 {
                    shape: vec![1, 3, 768, 768],
                    data: pixels,
                },
            )])?,
            "image_features",
        )?;
        ensure!(
            output.shape == [1, 577, 768],
            "unexpected Florence vision shape"
        );
        Ok(output)
    }
    fn embeddings(&mut self, ids: &[i64]) -> Result<TensorOutput> {
        let out = named(
            self.embed.run_tensors(&[(
                "input_ids",
                TensorInput::I64 {
                    shape: vec![1, ids.len()],
                    data: ids.to_vec(),
                },
            )])?,
            "inputs_embeds",
        )?;
        ensure!(
            out.shape == [1, ids.len(), 768],
            "unexpected token embedding shape"
        );
        Ok(out)
    }
    fn generate(
        &mut self,
        visual: &TensorOutput,
        prompt: &str,
        max_tokens: usize,
    ) -> Result<Generated> {
        let encoding = self
            .tokenizer
            .encode(prompt, true)
            .map_err(|e| anyhow::anyhow!(e))?;
        let ids: Vec<_> = encoding.get_ids().iter().map(|&v| i64::from(v)).collect();
        let text = self.embeddings(&ids)?;
        let n = visual.shape[1] + ids.len();
        let mut data = visual.data.clone();
        data.extend(text.data);
        let hidden = named(
            self.encoder.run_tensors(&[
                (
                    "attention_mask",
                    TensorInput::I64 {
                        shape: vec![1, n],
                        data: vec![1; n],
                    },
                ),
                (
                    "inputs_embeds",
                    TensorInput::F32 {
                        shape: vec![1, n, 768],
                        data,
                    },
                ),
            ])?,
            "last_hidden_state",
        )?;
        ensure!(hidden.shape == [1, n, 768], "unexpected encoder shape");
        // Full-prefix greedy decoding avoids a boolean merged-graph input and
        // keeps the shared runtime unchanged. Bounded to 256 output tokens.
        let mut generated = vec![2i64, 0]; // decoder start, forced BOS
        let mut probabilities = Vec::new();
        for _ in 0..max_tokens {
            let embeds = self.embeddings(&generated)?;
            let logits = named(
                self.decoder.run_tensors(&[
                    (
                        "encoder_attention_mask",
                        TensorInput::I64 {
                            shape: vec![1, n],
                            data: vec![1; n],
                        },
                    ),
                    (
                        "encoder_hidden_states",
                        TensorInput::F32 {
                            shape: hidden.shape.clone(),
                            data: hidden.data.clone(),
                        },
                    ),
                    (
                        "inputs_embeds",
                        TensorInput::F32 {
                            shape: embeds.shape,
                            data: embeds.data,
                        },
                    ),
                ])?,
                "logits",
            )?;
            ensure!(
                logits.shape == [1, generated.len(), 51289],
                "unexpected decoder logits shape"
            );
            let row = &logits.data[(generated.len() - 1) * 51289..];
            let (token, probability) = greedy_token(row)?;
            if token == 2 {
                return Ok(Generated {
                    ids: generated[2..].iter().map(|&v| v as u32).collect(),
                    probabilities,
                });
            }
            generated.push(i64::from(token));
            probabilities.push(probability);
        }
        anyhow::bail!("Florence generation exceeded {max_tokens} tokens without EOS")
    }
    pub fn caption(&mut self, image: &RgbImage) -> Result<Caption> {
        let visual = self.visual(image)?;
        let short = self.generate(&visual, "What does the image describe?", 96)?;
        let detail = self.generate(
            &visual,
            "Describe in detail what is shown in the image.",
            192,
        )?;
        let short = self
            .tokenizer
            .decode(&short.ids, true)
            .map_err(|e| anyhow::anyhow!(e))?;
        let alt = self
            .tokenizer
            .decode(&detail.ids, true)
            .map_err(|e| anyhow::anyhow!(e))?;
        ensure!(
            !short.trim().is_empty() && !alt.trim().is_empty(),
            "empty generated caption"
        );
        Ok(Caption {
            caption: first_sentence(&short),
            alt_text: alt.trim().to_owned(),
        })
    }
    pub fn ocr(&mut self, image: &RgbImage) -> Result<Vec<index::OcrRegion>> {
        let visual = self.visual(image)?;
        let generated =
            self.generate(&visual, "What is the text in the image, with regions?", 256)?;
        let decoded = self
            .tokenizer
            .decode(&generated.ids, false)
            .map_err(|e| anyhow::anyhow!(e))?;
        // Geometric mean of autoregressive token probabilities. Sequence-level
        // confidence shared across regions, NOT calibrated OCR detection scores.
        let confidence = if generated.probabilities.is_empty() {
            0.
        } else {
            (generated
                .probabilities
                .iter()
                .map(|p| p.max(f32::MIN_POSITIVE).ln())
                .sum::<f32>()
                / generated.probabilities.len() as f32)
                .exp()
        };
        parse_ocr(&decoded, confidence)
    }
    pub fn partition_reports(&mut self) -> Result<Vec<(String, PartitionReport)>> {
        [
            ("vision", &mut self.vision),
            ("embed", &mut self.embed),
            ("encoder", &mut self.encoder),
            ("decoder", &mut self.decoder),
        ]
        .into_iter()
        .map(|(name, s)| Ok((name.into(), s.partition_report()?)))
        .collect()
    }
}
struct Generated {
    ids: Vec<u32>,
    probabilities: Vec<f32>,
}
fn named(outputs: Vec<TensorOutput>, name: &str) -> Result<TensorOutput> {
    let out = outputs
        .into_iter()
        .find(|o| o.name == name)
        .with_context(|| format!("missing {name}"))?;
    ensure!(
        out.shape.iter().product::<usize>() == out.data.len()
            && out.data.iter().all(|v| v.is_finite()),
        "invalid {name} output"
    );
    Ok(out)
}
fn greedy_token(row: &[f32]) -> Result<(u32, f32)> {
    ensure!(
        !row.is_empty() && row.iter().all(|v| v.is_finite()),
        "invalid logits"
    );
    let (token, &max) = row
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1).then(b.0.cmp(&a.0)))
        .unwrap();
    let sum: f32 = row.iter().map(|v| (*v - max).exp()).sum();
    Ok((token as u32, 1. / sum))
}
fn first_sentence(text: &str) -> String {
    let text = text.trim();
    let end = text
        .char_indices()
        .find(|(_, c)| matches!(c, '.' | '!' | '?'))
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(text.len());
    text[..end].to_owned()
}
/// Florence emits text followed by four quantized (x,y) polygon corners.
/// Return their enclosing axis-aligned box in normalized displayed-image space.
pub fn parse_ocr(text: &str, confidence: f32) -> Result<Vec<index::OcrRegion>> {
    ensure!(
        confidence.is_finite() && (0. ..=1.).contains(&confidence),
        "invalid OCR confidence"
    );
    let clean = text
        .replace("<s>", "")
        .replace("</s>", "")
        .replace("<pad>", "");
    let mut rest = clean.as_str();
    let mut regions = Vec::new();
    while let Some(start) = rest.find("<loc_") {
        let label = rest[..start].trim().to_owned();
        ensure!(!label.is_empty(), "OCR region without text");
        rest = &rest[start..];
        let mut coords = [0.; 8];
        for coord in &mut coords {
            let token = rest
                .strip_prefix("<loc_")
                .context("incomplete OCR quadrilateral")?;
            let end = token.find('>').context("incomplete location token")?;
            let value: u32 = token[..end].parse()?;
            ensure!(value < 1000, "OCR coordinate outside quantization range");
            *coord = (value as f32 + 0.5) / 1000.;
            rest = &token[end + 1..];
        }
        let xs = [coords[0], coords[2], coords[4], coords[6]];
        let ys = [coords[1], coords[3], coords[5], coords[7]];
        regions.push(index::OcrRegion {
            text: label,
            bbox: [
                xs.into_iter().fold(1., f32::min),
                ys.into_iter().fold(1., f32::min),
                xs.into_iter().fold(0., f32::max),
                ys.into_iter().fold(0., f32::max),
            ],
            confidence,
        });
    }
    ensure!(rest.trim().is_empty(), "unparsed OCR output");
    Ok(regions)
}
