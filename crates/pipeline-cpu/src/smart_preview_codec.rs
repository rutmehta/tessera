//! Version 2 internal camera-linear container, not DNG. Planar LE f16/f32 + zstd.
//! Reads legacy version 1 as Detail2560; all new writes explicitly record the tier.
//! The source digest and byte length are caller assertions, not source verification.
use super::{CameraLinearProxy, SmartPreviewTier};
use crate::{CorrectionSource, Image, ManualCaSettings, ResolvedLens};
use engine_api::{
    EngineError, EngineResult,
    color::ColorMatrix3,
    recipe::{DevelopSettings, settings::*},
};
use raw_decode::{CfaLayout, RawMetadata};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

const MAGIC: &[u8; 8] = b"TESSCLP\0";
const HEADER: usize = 96;
const MAX_METADATA: usize = 4 * 1024 * 1024;
const MAX_PAYLOAD: usize = 2560 * 2560 * 3 * 4;
const MAX_COMPRESSED: usize = MAX_PAYLOAD + 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SmartPreviewEncoding {
    F16,
    F32,
}
#[derive(Clone, Debug)]
pub struct DecodedSmartPreview {
    pub proxy: CameraLinearProxy,
    /// Asserted original byte length; callers must independently verify originals.
    pub original_byte_length: u64,
    /// Exact container identity: header (excluding this digest), metadata and compressed bytes.
    pub container_digest: [u8; 32],
    pub encoding: SmartPreviewEncoding,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    generator: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tier: Option<SmartPreviewTier>,
    width: u32,
    height: u32,
    scale: u32,
    encoding: SmartPreviewEncoding,
    original_digest: [u8; 32],
    original_byte_length: u64,
    metadata: Metadata,
    decode: DecodeSettings,
    linearize: LinearizeSettings,
    demosaic: DemosaicSettings,
    denoise: DenoiseSettings,
    lens: LensSettings,
    correction: Correction,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Correction {
    source: Source,
    manual_ca: [f32; 2],
    sample: Option<lens::CalibrationSample>,
}
#[derive(Serialize, Deserialize)]
enum Source {
    Embedded,
    Database,
    Image,
    Manual,
}
#[derive(Serialize, Deserialize)]
enum Cfa {
    Bayer([[u8; 2]; 2]),
    XTrans([[u8; 6]; 6]),
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    make: String,
    model: String,
    lens: Option<String>,
    iso: f32,
    shutter_s: f32,
    aperture: f32,
    focal_mm: f32,
    capture_time: i64,
    orientation: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    catalog_orientation: Option<u16>,
    #[serde(default)]
    baseline_exposure: f32,
    width: u32,
    height: u32,
    black_levels: [f32; 4],
    white_level: u32,
    as_shot_wb: [f32; 4],
    cam_xyz: [[f32; 3]; 4],
    rgb_cam: [[f32; 4]; 3],
    default_crop: [u32; 4],
    has_gain_map: bool,
    has_opcode_list: bool,
    opcode_lists: [Option<Vec<u8>>; 3],
    cfa: Cfa,
    camera_to_xyz: [[f64; 3]; 3],
}
impl Metadata {
    fn capture(m: &RawMetadata) -> EngineResult<Self> {
        let variable_bytes = m
            .make
            .len()
            .checked_add(m.model.len())
            .and_then(|n| n.checked_add(m.lens.as_ref().map_or(0, String::len)))
            .and_then(|n| {
                m.opcode_lists
                    .iter()
                    .flatten()
                    .try_fold(n, |n, b| n.checked_add(b.len()))
            });
        if variable_bytes.is_none_or(|n| n > MAX_METADATA) {
            return Err(invalid("metadata too large"));
        }
        Ok(Self {
            make: m.make.clone(),
            model: m.model.clone(),
            lens: m.lens.clone(),
            iso: m.iso,
            shutter_s: m.shutter_s,
            aperture: m.aperture,
            focal_mm: m.focal_mm,
            capture_time: m.capture_time,
            orientation: m.orientation,
            catalog_orientation: m.catalog_orientation,
            baseline_exposure: m.baseline_exposure,
            width: m.width,
            height: m.height,
            black_levels: m.black_levels,
            white_level: m.white_level,
            as_shot_wb: m.as_shot_wb,
            cam_xyz: m.cam_xyz,
            rgb_cam: m.rgb_cam,
            default_crop: m.default_crop,
            has_gain_map: m.has_gain_map,
            has_opcode_list: m.has_opcode_list,
            opcode_lists: m.opcode_lists.clone(),
            camera_to_xyz: m.camera_to_xyz.0,
            cfa: match m.cfa_layout {
                CfaLayout::Bayer(p) => Cfa::Bayer(p),
                CfaLayout::XTrans(p) => Cfa::XTrans(p),
                _ => return Err(invalid("unsupported CFA")),
            },
        })
    }
    fn restore(self) -> EngineResult<RawMetadata> {
        let m = RawMetadata {
            make: self.make,
            model: self.model,
            lens: self.lens,
            iso: self.iso,
            shutter_s: self.shutter_s,
            aperture: self.aperture,
            focal_mm: self.focal_mm,
            capture_time: self.capture_time,
            catalog_orientation: self.catalog_orientation,
            baseline_exposure: self.baseline_exposure,
            orientation: self.orientation,
            width: self.width,
            height: self.height,
            black_levels: self.black_levels,
            white_level: self.white_level,
            as_shot_wb: self.as_shot_wb,
            cam_xyz: self.cam_xyz,
            rgb_cam: self.rgb_cam,
            default_crop: self.default_crop,
            has_gain_map: self.has_gain_map,
            has_opcode_list: self.has_opcode_list,
            opcode_lists: self.opcode_lists,
            camera_to_xyz: ColorMatrix3(self.camera_to_xyz),
            cfa_layout: match self.cfa {
                Cfa::Bayer(p) => CfaLayout::Bayer(p),
                Cfa::XTrans(p) => CfaLayout::XTrans(p),
            },
        };
        let [x, y, w, h] = m.default_crop;
        if w == 0
            || h == 0
            || x.checked_add(w).is_none_or(|v| v > m.width)
            || y.checked_add(h).is_none_or(|v| v > m.height)
            || !m.baseline_exposure.is_finite()
            || !(1..=8).contains(&m.orientation)
            || m.catalog_orientation
                .is_some_and(|o| !(1..=8).contains(&o) || m.orientation != 1)
            || m.white_level == 0
            || ![m.iso, m.shutter_s, m.aperture, m.focal_mm]
                .iter()
                .chain(m.black_levels.iter())
                .chain(m.as_shot_wb.iter())
                .chain(m.cam_xyz.iter().flatten())
                .chain(m.rgb_cam.iter().flatten())
                .all(|v| v.is_finite())
            || !m.camera_to_xyz.0.iter().flatten().all(|v| v.is_finite())
        {
            return Err(invalid("invalid original metadata"));
        }
        crate::mosaic::validate_cfa(m.cfa_layout)?;
        let period = if matches!(m.cfa_layout, CfaLayout::XTrans(_)) {
            6
        } else {
            2
        };
        if m.width < period || m.height < period {
            return Err(invalid("incomplete sensor CFA"));
        }
        Ok(m)
    }
}
// Limit writes before vector growth, including expansion of opcode bytes into JSON.
struct BoundedMetadata(Vec<u8>);
impl Write for BoundedMetadata {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_METADATA.saturating_sub(self.0.len()) {
            return Err(std::io::Error::other("metadata too large"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
/// Freeze the version-1 nested object shape independently of the recipe's
/// permissive serde defaults. Typed decoding still runs first to reject duplicate
/// known members, then this check rejects missing and unknown nested members.
fn validate_nested_schema(bytes: &[u8], version: u32) -> EngineResult<()> {
    use serde_json::Value;
    fn keys(v: &Value, expected: &[&str]) -> EngineResult<()> {
        let object = v
            .as_object()
            .ok_or_else(|| invalid("nested object required"))?;
        if object.len() != expected.len() || !expected.iter().all(|k| object.contains_key(*k)) {
            return Err(invalid("missing or unknown version-1 nested member"));
        }
        Ok(())
    }
    let v: Value = serde_json::from_slice(bytes).map_err(invalid)?;
    if version == 1 && v.get("tier").is_some() {
        return Err(invalid("legacy version 1 must not declare a tier"));
    }
    keys(&v["decode"], &["frame_index", "pixel_shift_merge"])?;
    keys(&v["linearize"], &["highlight_reconstruction"])?;
    keys(&v["demosaic"], &["method", "model"])?;
    // Version 1 admits only nonlearned demosaic and raw denoise Off.
    if !v["demosaic"]["model"].is_null() {
        return Err(invalid("unsupported demosaic model"));
    }
    keys(&v["denoise"], &["method", "amount", "chroma_only"])?;
    keys(&v["denoise"]["method"], &["kind"])?;
    // Legacy CA members are additive recipe fields; old snapshots omit them.
    let mut lens = v["lens"].clone();
    if let Some(object) = lens.as_object_mut() {
        object.remove("legacy_ca_red");
        object.remove("legacy_ca_blue");
    }
    keys(
        &lens,
        &[
            "profile",
            "distortion_scale",
            "vignetting_scale",
            "chromatic_aberration_scale",
            "remove_chromatic_aberration",
            "manual_distortion",
            "manual_vignetting",
            "manual_vignetting_midpoint",
            "defringe_purple",
            "defringe_green",
            "softness_correction",
        ],
    )?;
    for band in ["defringe_purple", "defringe_green"] {
        keys(&v["lens"][band], &["amount", "hue_range"])?;
    }
    let profile = &v["lens"]["profile"];
    if profile["kind"] == "database" {
        keys(profile, &["kind", "profile"])?;
        keys(
            &profile["profile"],
            &["name", "filename", "digest", "setup"],
        )?;
    } else {
        keys(profile, &["kind"])?;
    }
    // Option members must also appear explicitly, including null snapshots.
    keys(&v["correction"], &["source", "manual_ca", "sample"])?;
    let sample = &v["correction"]["sample"];
    if !sample.is_null() {
        keys(
            sample,
            &[
                "focal",
                "aperture",
                "distance",
                "distortion",
                "distortion_scale",
                "radial_odd",
                "coordinate_scale",
                "ca_red",
                "ca_blue",
                "vignette",
            ],
        )?;
        keys(
            &sample["distortion"],
            &["k1", "k2", "k3", "p1", "p2", "cx", "cy"],
        )?;
    }
    Ok(())
}
fn invalid(reason: impl std::fmt::Display) -> EngineError {
    EngineError::invalid("smart preview container", reason.to_string())
}
fn identity(bytes: &[u8]) -> [u8; 32] {
    let mut hash = blake3::Hasher::new();
    hash.update(&bytes[..64]);
    hash.update(&bytes[HEADER..]);
    *hash.finalize().as_bytes()
}
fn length(width: u32, height: u32, encoding: SmartPreviewEncoding) -> EngineResult<usize> {
    if width == 0
        || height == 0
        || width > CameraLinearProxy::MAX_EDGE
        || height > CameraLinearProxy::MAX_EDGE
    {
        return Err(invalid("invalid proxy dimensions"));
    }
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(3))
        .and_then(|n| {
            n.checked_mul(if encoding == SmartPreviewEncoding::F16 {
                2
            } else {
                4
            })
        })
        .filter(|n| *n <= MAX_PAYLOAD)
        .ok_or_else(|| invalid("payload length overflow"))
}
impl CameraLinearProxy {
    /// Encode a bounded snapshot. Original length/digest are assertions made by the caller.
    /// F16 is selected only if every sample meets |error| <= 0.0005*|value| + 3e-8.
    pub fn encode_persistent(&self, original_byte_length: u64) -> EngineResult<Vec<u8>> {
        if self.is_external_dng() {
            return Err(invalid("external DNG must remain a DNG source"));
        }
        let half_ok = self.pixels.planes().iter().flatten().all(|v| {
            let h = half::f16::from_f32(*v).to_f32();
            v.is_finite()
                && h.is_finite()
                && (f64::from(h) - f64::from(*v)).abs() <= 0.0005 * f64::from(*v).abs() + 3e-8
        });
        let encoding = if half_ok {
            SmartPreviewEncoding::F16
        } else {
            SmartPreviewEncoding::F32
        };
        let raw_len = length(self.pixels.width(), self.pixels.height(), encoding)?;
        if self.pixels.planes().len() != 3 {
            return Err(invalid("camera RGB required"));
        }
        let mut raw = Vec::with_capacity(raw_len);
        for v in self.pixels.planes().iter().flatten() {
            if !v.is_finite() {
                return Err(invalid("nonfinite sample"));
            }
            match encoding {
                SmartPreviewEncoding::F16 => {
                    raw.extend_from_slice(&half::f16::from_f32(*v).to_bits().to_le_bytes())
                }
                SmartPreviewEncoding::F32 => raw.extend_from_slice(&v.to_le_bytes()),
            }
        }
        let snapshot = Snapshot {
            generator: Self::GENERATOR_REVISION,
            tier: Some(self.tier),
            width: self.pixels.width(),
            height: self.pixels.height(),
            scale: self.scale,
            encoding,
            original_digest: self.original_content_digest,
            original_byte_length,
            metadata: Metadata::capture(&self.metadata)?,
            decode: self.decode.clone(),
            linearize: self.linearize.clone(),
            demosaic: self.demosaic.clone(),
            denoise: self.denoise.clone(),
            lens: self.lens.clone(),
            correction: Correction {
                source: match self.correction.source {
                    CorrectionSource::Embedded => Source::Embedded,
                    CorrectionSource::Database => Source::Database,
                    CorrectionSource::Image => Source::Image,
                    CorrectionSource::Manual => Source::Manual,
                },
                manual_ca: [
                    self.correction.manual_ca.red_cyan,
                    self.correction.manual_ca.blue_yellow,
                ],
                sample: self.correction.sample.clone(),
            },
        };
        let mut writer = BoundedMetadata(Vec::new());
        serde_json::to_writer(&mut writer, &snapshot).map_err(invalid)?;
        let metadata = writer.0;
        let compressed = zstd::bulk::compress(&raw, 3).map_err(invalid)?;
        if compressed.len() > MAX_COMPRESSED {
            return Err(invalid("compressed payload too large"));
        }
        let mut out = Vec::with_capacity(HEADER + metadata.len() + compressed.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&2_u32.to_le_bytes());
        out.extend_from_slice(&(metadata.len() as u32).to_le_bytes());
        out.extend_from_slice(&(compressed.len() as u64).to_le_bytes());
        out.extend_from_slice(&(raw_len as u64).to_le_bytes());
        out.extend_from_slice(blake3::hash(&raw).as_bytes());
        out.extend_from_slice(&[0; 32]);
        out.extend_from_slice(&metadata);
        out.extend_from_slice(&compressed);
        let digest = identity(&out);
        out[64..96].copy_from_slice(&digest);
        // Apply the same schema/semantic checks to both directions.
        Self::decode_persistent(&out)?;
        Ok(out)
    }
    /// Verify framing, container/payload hashes, bounded decompression, and snapshot semantics.
    /// This never verifies source photo bytes and never estimates lens corrections.
    pub fn decode_persistent(bytes: &[u8]) -> EngineResult<DecodedSmartPreview> {
        if bytes.len() < HEADER || &bytes[..8] != MAGIC {
            return Err(invalid("magic/version/header"));
        }
        let version = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
        if !matches!(version, 1 | 2) {
            return Err(invalid("magic/version/header"));
        }
        let metadata_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let compressed_len = u64::from_le_bytes(bytes[16..24].try_into().unwrap());
        let raw_len = u64::from_le_bytes(bytes[24..32].try_into().unwrap());
        if metadata_len > MAX_METADATA
            || compressed_len > MAX_COMPRESSED as u64
            || raw_len > MAX_PAYLOAD as u64
            || (HEADER + metadata_len).checked_add(compressed_len as usize) != Some(bytes.len())
        {
            return Err(invalid("container lengths"));
        }
        let container_digest = identity(bytes);
        if bytes[64..96] != container_digest {
            return Err(invalid("container digest"));
        }
        let s: Snapshot =
            serde_json::from_slice(&bytes[HEADER..HEADER + metadata_len]).map_err(invalid)?;
        validate_nested_schema(&bytes[HEADER..HEADER + metadata_len], version)?;
        let tier = match (version, s.generator, s.tier) {
            (1, 1, None) => SmartPreviewTier::Detail2560,
            (2, Self::GENERATOR_REVISION, Some(tier)) => tier,
            _ => return Err(invalid("inconsistent container version/generator/tier")),
        };
        if s.original_byte_length == 0 || length(s.width, s.height, s.encoding)? as u64 != raw_len {
            return Err(invalid("generator/source/payload length"));
        }
        let metadata = s.metadata.restore()?;
        let [_, _, cw, ch] = metadata.default_crop;
        if s.scale != cw.max(ch).div_ceil(tier.max_edge()).max(1)
            || s.width != cw.div_ceil(s.scale)
            || s.height != ch.div_ceil(s.scale)
        {
            return Err(invalid("crop/scale/dimensions mismatch"));
        }
        let settings = DevelopSettings {
            decode: s.decode.clone(),
            linearize: s.linearize.clone(),
            demosaic: s.demosaic.clone(),
            denoise: s.denoise.clone(),
            lens: s.lens.clone(),
            ..Default::default()
        };
        crate::validate_settings(&settings)?;
        if !matches!(s.denoise.method, DenoiseMethod::Off) {
            return Err(invalid("baked denoise unsupported"));
        }
        let manual_ca = ManualCaSettings {
            red_cyan: s.correction.manual_ca[0],
            blue_yellow: s.correction.manual_ca[1],
        };
        manual_ca.validate()?;
        if let Some(sample) = &s.correction.sample {
            lens::Profile {
                model: "snapshot".into(),
                samples: vec![sample.clone()],
                ..Default::default()
            }
            .validate()
            .map_err(invalid)?;
        }
        let parsed = crate::embedded_lens::Embedded::parse(&metadata)?;
        let use_embedded = matches!(
            s.lens.profile,
            LensProfileSource::Auto | LensProfileSource::Embedded
        );
        if use_embedded && !parsed.stages[2].is_empty() {
            return Err(invalid("late sensor opcodes require original"));
        }
        let source = match s.correction.source {
            Source::Embedded => CorrectionSource::Embedded,
            Source::Database => CorrectionSource::Database,
            Source::Image => CorrectionSource::Image,
            Source::Manual => CorrectionSource::Manual,
        };
        let mode_matches_source = match &s.lens.profile {
            LensProfileSource::Database { .. } => source == CorrectionSource::Database,
            LensProfileSource::Embedded => source == CorrectionSource::Embedded,
            LensProfileSource::None => {
                source == CorrectionSource::Manual
                    || (source == CorrectionSource::Image && s.lens.remove_chromatic_aberration)
            }
            LensProfileSource::AutoCalibrated => {
                matches!(source, CorrectionSource::Image | CorrectionSource::Manual)
            }
            LensProfileSource::Auto => true,
        };
        if !mode_matches_source
            || (source == CorrectionSource::Image && (cw < 8 || ch < 8))
            || (source == CorrectionSource::Embedded) != (use_embedded && parsed.present())
            || (matches!(source, CorrectionSource::Database | CorrectionSource::Image))
                != s.correction.sample.is_some()
            || (matches!(s.lens.profile, LensProfileSource::Embedded)
                && source != CorrectionSource::Embedded)
        {
            return Err(invalid("inconsistent resolved lens snapshot"));
        }
        let correction = ResolvedLens {
            manual_ca,
            source,
            sample: s.correction.sample,
            embedded: if use_embedded {
                parsed
            } else {
                Default::default()
            },
        };
        // Limit zstd's internal window AND output. Never decode_all or trust frame content size.
        let mut decoder =
            zstd::stream::read::Decoder::new(&bytes[HEADER + metadata_len..]).map_err(invalid)?;
        decoder.window_log_max(23).map_err(invalid)?;
        let mut raw = vec![0; raw_len as usize];
        decoder.read_exact(&mut raw).map_err(invalid)?;
        if decoder.read(&mut [0_u8; 1]).map_err(invalid)? != 0
            || bytes[32..64] != *blake3::hash(&raw).as_bytes()
        {
            return Err(invalid("payload length/digest"));
        }
        let samples: Vec<f32> = match s.encoding {
            SmartPreviewEncoding::F16 => raw
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| half::f16::from_bits(u16::from_le_bytes(*b)).to_f32())
                .collect(),
            SmartPreviewEncoding::F32 => raw
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| f32::from_le_bytes(*b))
                .collect(),
        };
        let plane_len = s.width as usize * s.height as usize;
        let pixels = Image::new(
            s.width,
            s.height,
            samples
                .chunks_exact(plane_len)
                .map(<[f32]>::to_vec)
                .collect(),
        )?;
        Ok(DecodedSmartPreview {
            original_byte_length: s.original_byte_length,
            container_digest,
            encoding: s.encoding,
            proxy: Self {
                external_dng: false,
                pixels,
                metadata,
                correction,
                decode: s.decode,
                linearize: s.linearize,
                demosaic: s.demosaic,
                denoise: s.denoise,
                lens: s.lens,
                original_content_digest: s.original_digest,
                scale: s.scale,
                tier,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(values: &[f32]) -> CameraLinearProxy {
        let metadata = RawMetadata {
            make: "synthetic".into(),
            model: "camera".into(),
            lens: None,
            iso: 100.,
            shutter_s: 0.01,
            aperture: 4.,
            focal_mm: 50.,
            capture_time: 0,
            catalog_orientation: None,
            baseline_exposure: 0.,
            orientation: 1,
            width: 2,
            height: 2,
            cfa_layout: CfaLayout::Bayer([[0, 1], [1, 2]]),
            black_levels: [0.; 4],
            white_level: 65535,
            as_shot_wb: [2., 1., 1.5, 1.],
            camera_to_xyz: ColorMatrix3::IDENTITY,
            cam_xyz: [[0.7, 0.2, 0.1], [0.1, 0.8, 0.1], [0.1, 0.2, 0.7], [0.; 3]],
            rgb_cam: [[0.; 4]; 3],
            default_crop: [0, 0, 2, 2],
            has_gain_map: false,
            has_opcode_list: false,
            opcode_lists: [None, None, None],
        };
        let s = DevelopSettings::default();
        CameraLinearProxy {
            external_dng: false,
            pixels: Image::new(2, 2, vec![values.to_vec(); 3]).unwrap(),
            metadata,
            correction: ResolvedLens {
                manual_ca: Default::default(),
                source: CorrectionSource::Manual,
                sample: None,
                embedded: Default::default(),
            },
            decode: s.decode,
            linearize: s.linearize,
            demosaic: s.demosaic,
            denoise: s.denoise,
            lens: s.lens,
            original_content_digest: [1; 32],
            scale: 1,
            tier: SmartPreviewTier::Detail2560,
        }
    }
    #[test]
    fn finite_signed_hdr_uses_f16_only_within_error_bound() {
        for tier in [SmartPreviewTier::Detail2560, SmartPreviewTier::Compact2048] {
            let mut p = fixture(&[-0.13, 3.7, 0.000000035, 65504.]);
            p.tier = tier;
            let d =
                CameraLinearProxy::decode_persistent(&p.encode_persistent(50).unwrap()).unwrap();
            assert_eq!(d.encoding, SmartPreviewEncoding::F16);
            for (a, b) in p
                .pixels
                .planes()
                .iter()
                .flatten()
                .zip(d.proxy.pixels.planes().iter().flatten())
            {
                assert!(
                    (f64::from(*a) - f64::from(*b)).abs() <= 0.0005 * f64::from(*a).abs() + 3e-8
                );
            }
            assert!(d.proxy.pixels.planes()[0][0] < 0.);
            assert!(d.proxy.pixels.planes()[0][1] > 1.);
            assert_eq!(d.proxy.tier(), tier);
        }
    }
    #[test]
    fn f16_overflow_uses_exact_f32_for_entire_payload() {
        for tier in [SmartPreviewTier::Detail2560, SmartPreviewTier::Compact2048] {
            let mut p = fixture(&[-100000., 100000., f32::MAX, -f32::MAX]);
            p.tier = tier;
            let d =
                CameraLinearProxy::decode_persistent(&p.encode_persistent(50).unwrap()).unwrap();
            assert_eq!(d.encoding, SmartPreviewEncoding::F32);
            assert_eq!(d.proxy.pixels.planes(), p.pixels.planes());
            assert_eq!(d.proxy.tier(), tier);
        }
    }
    #[test]
    fn image_estimated_sample_is_restored_without_reresolution() {
        let mut p = fixture(&[0.1, 0.2, 0.3, 0.4]);
        p.metadata.width = 8;
        p.metadata.height = 8;
        p.metadata.default_crop = [0, 0, 8, 8];
        p.pixels = Image::new(8, 8, vec![vec![0.2; 64]; 3]).unwrap();
        p.correction.source = CorrectionSource::Image;
        let sample = lens::CalibrationSample {
            vignette: [-0.12345678901234567, 0., 0.],
            ca_red: [1.0123456789012345, 0., 0.],
            ..Default::default()
        };
        p.correction.sample = Some(sample.clone());
        let d = CameraLinearProxy::decode_persistent(&p.encode_persistent(50).unwrap()).unwrap();
        assert_eq!(d.proxy.correction.source, CorrectionSource::Image);
        assert_eq!(d.proxy.correction.sample, Some(sample));
    }
}
