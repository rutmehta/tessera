//! ICC profiles, display transforms and soft proofing.
mod display;
mod lut;
mod printer;
mod transform;
pub use display::DisplayProfile;
pub use lut::Lut3d;
pub use printer::*;
use std::{collections::HashMap, path::Path, sync::Arc};
pub use transform::*;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("ICC engine: {0}")]
    Icc(#[from] lcms2::Error),
    #[error("profile I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("unsupported profile or platform: {0}")]
    Unsupported(&'static str),
}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Builtin {
    Srgb,
    DisplayP3,
    AdobeRgb,
    ProPhoto,
    Rec2020,
    LinearRec2020,
}

fn builtin_icc(builtin: Builtin) -> Result<Vec<u8>> {
    use lcms2::{CIExyY, CIExyYTRIPLE, ToneCurve};
    let xy = |x, y| CIExyY { x, y, Y: 1.0 };
    let srgb_curve =
        || ToneCurve::new_parametric(4, &[2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045]);
    let (white, red, green, blue, curve) = match builtin {
        Builtin::Srgb => return Ok(lcms2::Profile::new_srgb().icc()?),
        Builtin::DisplayP3 => (
            xy(0.3127, 0.3290),
            xy(0.68, 0.32),
            xy(0.265, 0.69),
            xy(0.15, 0.06),
            srgb_curve()?,
        ),
        Builtin::AdobeRgb => (
            xy(0.3127, 0.3290),
            xy(0.64, 0.33),
            xy(0.21, 0.71),
            xy(0.15, 0.06),
            ToneCurve::new(563.0 / 256.0),
        ),
        Builtin::ProPhoto => (
            xy(0.3457, 0.3585),
            xy(0.7347, 0.2653),
            xy(0.1596, 0.8404),
            xy(0.0366, 0.0001),
            ToneCurve::new_parametric(4, &[1.8, 1.0, 0.0, 1.0 / 16.0, 1.0 / 32.0])?,
        ),
        Builtin::Rec2020 | Builtin::LinearRec2020 => (
            xy(0.3127, 0.3290),
            xy(0.708, 0.292),
            xy(0.170, 0.797),
            xy(0.131, 0.046),
            if builtin == Builtin::LinearRec2020 {
                ToneCurve::new(1.0)
            } else {
                ToneCurve::new_parametric(
                    4,
                    &[
                        1.0 / 0.45,
                        1.0 / 1.09929682680944,
                        0.09929682680944 / 1.09929682680944,
                        1.0 / 4.5,
                        0.081242858298635,
                    ],
                )?
            },
        ),
    };
    Ok(lcms2::Profile::new_rgb(
        &white,
        &CIExyYTRIPLE {
            Red: red,
            Green: green,
            Blue: blue,
        },
        &[&curve, &curve, &curve],
    )?
    .icc()?)
}

#[derive(Debug)]
pub struct Profile {
    bytes: Vec<u8>,
    digest: [u8; 32],
    lut_cache: Arc<transform::LutCache>,
}
impl Profile {
    pub fn digest(&self) -> [u8; 32] {
        self.digest
    }
    pub fn icc_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// ICC profile registry and shared, lazy 33³ output-LUT cache.
///
/// Transforms use the source profile's originating registry cache, including
/// when destination/proof profiles come from another registry. Keys use ICC
/// content digests and every transform option, never profile pointer identity.
/// Entries are retained for the lifetime of the registry and its profiles;
/// there is no process-global cache or automatic eviction.
#[derive(Default)]
pub struct Registry {
    profiles: HashMap<[u8; 32], Arc<Profile>>,
    builtins: HashMap<Builtin, Arc<Profile>>,
    lut_cache: Arc<transform::LutCache>,
}
impl Registry {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn load_bytes(&mut self, bytes: &[u8]) -> Result<Arc<Profile>> {
        let digest = *blake3::hash(bytes).as_bytes();
        if let Some(profile) = self.profiles.get(&digest) {
            return Ok(profile.clone());
        }
        let _ = lcms2::Profile::new_icc(bytes)?;
        let profile = Arc::new(Profile {
            bytes: bytes.to_vec(),
            digest,
            lut_cache: self.lut_cache.clone(),
        });
        self.profiles.insert(digest, profile.clone());
        Ok(profile)
    }
    pub fn load_file(&mut self, path: impl AsRef<Path>) -> Result<Arc<Profile>> {
        self.load_bytes(&std::fs::read(path)?)
    }
    /// A matrix display's primaries/white with linear TRCs, for a 3D-LUT +
    /// one-dimensional transfer-curve GPU path. Never strip an ICC CLUT.
    /// Returns None for non-RGB or LUT-based profiles.
    pub fn linearized_rgb(&mut self, profile: &Profile) -> Result<Option<Arc<Profile>>> {
        use lcms2::{Tag, TagSignature::*, ToneCurve};
        let mut icc = lcms2::Profile::new_icc(profile.icc_bytes())?;
        if icc.color_space() != lcms2::ColorSpaceSignature::RgbData
            || !icc.is_matrix_shaper()
            || [
                AToB0Tag, AToB1Tag, AToB2Tag, BToA0Tag, BToA1Tag, BToA2Tag, DToB0Tag, DToB1Tag,
                DToB2Tag, DToB3Tag, BToD0Tag, BToD1Tag, BToD2Tag, BToD3Tag,
            ]
            .into_iter()
            .any(|tag| icc.has_tag(tag))
        {
            return Ok(None);
        }
        let curve = ToneCurve::new(1.0);
        for tag in [RedTRCTag, GreenTRCTag, BlueTRCTag] {
            if !icc.write_tag(tag, Tag::ToneCurve(&curve)) {
                return Err(Error::Unsupported("cannot linearize display TRCs"));
            }
        }
        self.load_bytes(&icc.icc()?).map(Some)
    }
    pub fn builtin(&mut self, builtin: Builtin) -> Result<Arc<Profile>> {
        if let Some(profile) = self.builtins.get(&builtin) {
            return Ok(profile.clone());
        }
        let icc = builtin_icc(builtin)?;
        let profile = self.load_bytes(&icc)?;
        self.builtins.insert(builtin, profile.clone());
        Ok(profile)
    }
}
