use crate::{Error, Lut3d, Profile, Result};
pub use lcms2::Intent;
use lcms2::{Flags, PixelFormat};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
};

type LutCell = Arc<OnceLock<Arc<Lut3d>>>;

/// Registry-owned entries survive transform recreation. Profiles retain the cache,
/// not each other. Generation is single-flight per key, outside the map lock.
#[derive(Debug, Default)]
pub(crate) struct LutCache(Mutex<HashMap<LutKey, LutCell>>);

#[derive(Debug, PartialEq, Eq, Hash)]
struct LutKey {
    source: [u8; 32],
    destination: [u8; 32],
    proof: Option<[u8; 32]>,
    intent: u32,
    black_point_compensation: bool,
    simulate_paper: bool,
    gamut_threshold: u32,
}

impl LutCache {
    fn cell(
        &self,
        source: &Profile,
        destination: &Profile,
        proof: Option<&Profile>,
        options: TransformOptions,
    ) -> LutCell {
        let key = LutKey {
            source: source.digest(),
            destination: destination.digest(),
            proof: proof.map(Profile::digest),
            intent: options.intent as u32,
            black_point_compensation: options.black_point_compensation,
            simulate_paper: options.simulate_paper,
            gamut_threshold: options.gamut_threshold.to_bits(),
        };
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(key)
            .or_default()
            .clone()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TransformOptions {
    pub intent: Intent,
    /// Map source black into the destination (or proof medium) black range.
    /// In proof mode LCMS does not apply BPC on the proof-to-display leg,
    /// preserving the simulated medium's nonzero ink black.
    pub black_point_compensation: bool,
    /// Use absolute colorimetry on the proof-to-display leg to retain paper
    /// white. Only affects `Transform::proof`; ink black is simulated in both
    /// proof modes. The source-to-proof rendering intent remains `intent`.
    pub simulate_paper: bool,
    /// DeltaE76 threshold after a clipped destination round trip.
    pub gamut_threshold: f32,
}
impl Default for TransformOptions {
    fn default() -> Self {
        Self {
            intent: Intent::RelativeColorimetric,
            black_point_compensation: true,
            simulate_paper: false,
            gamut_threshold: 2.0,
        }
    }
}

type RgbTransform = lcms2::Transform<[f32; 3], [f32; 3]>;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GamutWarning {
    pub monitor: bool,
    pub proof: bool,
}

// u8 is LCMS's explicitly supported dynamically-sized pixel buffer type.
struct RoundTrip {
    forward: lcms2::Transform<[f32; 3], u8>,
    backward: lcms2::Transform<u8, [f32; 3]>,
    channels: usize,
    maximum: f32,
}
impl RoundTrip {
    fn new(
        source: &lcms2::Profile,
        destination: &lcms2::Profile,
        lab: &lcms2::Profile,
    ) -> Result<Self> {
        let (format, channels, maximum) = match destination.color_space() {
            lcms2::ColorSpaceSignature::RgbData => (PixelFormat::RGB_FLT, 3, 1.0),
            lcms2::ColorSpaceSignature::CmykData => (PixelFormat::CMYK_FLT, 4, 100.0),
            lcms2::ColorSpaceSignature::GrayData => (PixelFormat::GRAY_FLT, 1, 1.0),
            _ => {
                return Err(Error::Unsupported(
                    "gamut destination must be RGB, CMYK or gray",
                ))
            }
        };
        Ok(Self {
            forward: lcms2::Transform::new_flags(
                source,
                PixelFormat::RGB_FLT,
                destination,
                format,
                Intent::RelativeColorimetric,
                Flags::NO_OPTIMIZE,
            )?,
            backward: lcms2::Transform::new_flags(
                destination,
                format,
                lab,
                PixelFormat::Lab_FLT,
                Intent::RelativeColorimetric,
                Flags::NO_OPTIMIZE,
            )?,
            channels,
            maximum,
        })
    }
    fn lab(&self, rgb: [f32; 3]) -> [f32; 3] {
        let mut bytes = [0u8; 16];
        let bytes = &mut bytes[..self.channels * 4];
        self.forward.transform_pixels(&[rgb], bytes);
        for channel in bytes.as_chunks_mut::<4>().0 {
            let value = f32::from_ne_bytes(*channel).clamp(0.0, self.maximum);
            channel.copy_from_slice(&value.to_ne_bytes());
        }
        let mut lab = [[0.; 3]];
        self.backward.transform_pixels(bytes, &mut lab);
        lab[0]
    }
}

pub struct Transform {
    output: RgbTransform,
    to_lab: RgbTransform,
    monitor: RoundTrip,
    threshold: f32,
    proof_gamut: Option<RoundTrip>,
    lut: LutCell,
}
impl Transform {
    pub fn new(working: &Profile, display: &Profile, options: TransformOptions) -> Result<Self> {
        if !options.gamut_threshold.is_finite() || options.gamut_threshold < 0.0 {
            return Err(Error::Unsupported(
                "gamut threshold must be finite and nonnegative",
            ));
        }
        let lut = working.lut_cache.cell(working, display, None, options);
        let working = lcms2::Profile::new_icc(working.icc_bytes())?;
        let display = lcms2::Profile::new_icc(display.icc_bytes())?;
        if working.color_space() != lcms2::ColorSpaceSignature::RgbData
            || display.color_space() != lcms2::ColorSpaceSignature::RgbData
        {
            return Err(Error::Unsupported(
                "working and display profiles must be RGB",
            ));
        }
        let lab = lcms2::Profile::new_lab4_context(
            lcms2::GlobalContext::new(),
            &lcms2::CIExyY {
                x: 0.3457,
                y: 0.3585,
                Y: 1.0,
            },
        )?;
        let mut flags = Flags::NO_OPTIMIZE;
        if options.black_point_compensation {
            flags = flags | Flags::BLACKPOINT_COMPENSATION;
        }
        let output = RgbTransform::new_flags(
            &working,
            PixelFormat::RGB_FLT,
            &display,
            PixelFormat::RGB_FLT,
            options.intent,
            flags,
        )?;
        let to_lab = RgbTransform::new_flags(
            &working,
            PixelFormat::RGB_FLT,
            &lab,
            PixelFormat::Lab_FLT,
            Intent::RelativeColorimetric,
            Flags::NO_OPTIMIZE,
        )?;
        let monitor = RoundTrip::new(&working, &display, &lab)?;
        Ok(Self {
            output,
            to_lab,
            monitor,
            threshold: options.gamut_threshold,
            proof_gamut: None,
            lut,
        })
    }
    pub fn proof(
        working: &Profile,
        display: &Profile,
        proof: &Profile,
        options: TransformOptions,
    ) -> Result<Self> {
        let mut result = Self::new(working, display, options)?;
        result.lut = working
            .lut_cache
            .cell(working, display, Some(proof), options);
        let working = lcms2::Profile::new_icc(working.icc_bytes())?;
        let display = lcms2::Profile::new_icc(display.icc_bytes())?;
        let proof = lcms2::Profile::new_icc(proof.icc_bytes())?;
        let lab = lcms2::Profile::new_lab4_context(
            lcms2::GlobalContext::new(),
            &lcms2::CIExyY {
                x: 0.3457,
                y: 0.3585,
                Y: 1.0,
            },
        )?;
        let mut flags = Flags::NO_OPTIMIZE | Flags::SOFT_PROOFING | Flags::NO_WHITE_ON_WHITE_FIXUP;
        if options.black_point_compensation {
            flags = flags | Flags::BLACKPOINT_COMPENSATION;
        }
        result.output = RgbTransform::new_proofing(
            &working,
            PixelFormat::RGB_FLT,
            &display,
            PixelFormat::RGB_FLT,
            &proof,
            options.intent,
            if options.simulate_paper {
                Intent::AbsoluteColorimetric
            } else {
                Intent::RelativeColorimetric
            },
            flags,
        )?;
        result.proof_gamut = Some(RoundTrip::new(&working, &proof, &lab)?);
        Ok(result)
    }
    /// Lazily generate or reuse the source registry's red-fastest SDR LUT.
    /// Equivalent transforms share one allocation, even across threads and
    /// after earlier transforms have been dropped.
    pub fn lut33(&self) -> Arc<Lut3d> {
        self.lut
            .get_or_init(|| {
                let size = 33;
                let mut values = Vec::with_capacity(size * size * size);
                for b in 0..size {
                    for g in 0..size {
                        for r in 0..size {
                            values.push(self.apply([
                                r as f32 / 32.,
                                g as f32 / 32.,
                                b as f32 / 32.,
                            ]));
                        }
                    }
                }
                Arc::new(Lut3d { size, values })
            })
            .clone()
    }
    pub fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        let mut result = [[0.0; 3]];
        self.output.transform_pixels(&[rgb], &mut result);
        result[0]
    }
    pub fn gamut_warning(&self, rgb: [f32; 3]) -> GamutWarning {
        let [monitor, proof] = self.gamut_delta(rgb);
        GamutWarning {
            monitor: monitor > self.threshold,
            proof: proof > self.threshold,
        }
    }

    /// DeltaE76 to clipped monitor/proof round trips, before thresholding.
    /// The proof component is zero when no proof profile is selected.
    pub fn gamut_delta(&self, rgb: [f32; 3]) -> [f32; 2] {
        let mut original = [[0.; 3]];
        self.to_lab.transform_pixels(&[rgb], &mut original);
        let restored = self.monitor.lab(rgb);
        let delta = original[0]
            .iter()
            .zip(restored)
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f32>()
            .sqrt();
        let proof_delta = self.proof_gamut.as_ref().map_or(0.0, |proof| {
            original[0]
                .iter()
                .zip(proof.lab(rgb))
                .map(|(a, b)| (a - b).powi(2))
                .sum::<f32>()
                .sqrt()
        });
        [delta, proof_delta]
    }
}
