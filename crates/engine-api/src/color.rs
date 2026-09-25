//! Colour contracts: working spaces, 3×3 matrices, white points, illuminants
//! and ICC profile handles.
//!
//! The raw pipeline works in scene-referred **linear Rec.2020** float
//! ([`WorkingSpace::default`]). Colour matrices use `f64` so chained
//! conversions (camera → XYZ → working, chromatic adaptation) do not
//! accumulate `f32` error before being baked into a kernel.

use std::ops::Mul;

use serde::{Deserialize, Serialize};

use crate::error::{EngineError, EngineResult};
use crate::id::Digest;

/// CIE 1931 xy chromaticity of a white point.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WhitePoint {
    /// x chromaticity.
    pub x: f64,
    /// y chromaticity.
    pub y: f64,
}

impl WhitePoint {
    /// CIE D50 (ICC profile connection space white).
    pub const D50: Self = Self::new(0.34567, 0.35850);
    /// CIE D55.
    pub const D55: Self = Self::new(0.33242, 0.34743);
    /// CIE D65 (sRGB, Rec.709, Rec.2020, Display P3 white).
    pub const D65: Self = Self::new(0.31270, 0.32900);
    /// CIE D75.
    pub const D75: Self = Self::new(0.29902, 0.31485);
    /// CIE standard illuminant A (tungsten, 2856 K).
    pub const A: Self = Self::new(0.44757, 0.40745);
    /// CIE F2 (cool white fluorescent).
    pub const F2: Self = Self::new(0.37208, 0.37529);
    /// CIE F7 (broadband daylight fluorescent).
    pub const F7: Self = Self::new(0.31292, 0.32933);
    /// CIE F11 (narrow tri-band, TL84).
    pub const F11: Self = Self::new(0.38052, 0.37713);
    /// ACES white (≈ D60).
    pub const ACES: Self = Self::new(0.32168, 0.33767);

    /// Creates a white point from xy chromaticity.
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// XYZ tristimulus normalised to Y = 1.
    pub fn to_xyz(self) -> [f64; 3] {
        [self.x / self.y, 1.0, (1.0 - self.x - self.y) / self.y]
    }
}

/// RGB primaries plus white, as xy chromaticities.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Primaries {
    /// Red primary.
    pub red: [f64; 2],
    /// Green primary.
    pub green: [f64; 2],
    /// Blue primary.
    pub blue: [f64; 2],
    /// Reference white.
    pub white: WhitePoint,
}

/// A 3×3 colour transform in row-major order, applied as `out = M · rgb`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ColorMatrix3(pub [[f64; 3]; 3]);

impl Default for ColorMatrix3 {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl ColorMatrix3 {
    /// The identity transform.
    pub const IDENTITY: Self = Self([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);

    /// Diagonal scaling (e.g. white-balance multipliers).
    pub const fn diagonal(d: [f64; 3]) -> Self {
        Self([[d[0], 0.0, 0.0], [0.0, d[1], 0.0], [0.0, 0.0, d[2]]])
    }

    /// Applies the matrix to a colour vector.
    pub fn apply(&self, v: [f64; 3]) -> [f64; 3] {
        let m = &self.0;
        [
            m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
            m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
            m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
        ]
    }

    /// Transpose.
    pub fn transpose(&self) -> Self {
        let m = &self.0;
        Self([
            [m[0][0], m[1][0], m[2][0]],
            [m[0][1], m[1][1], m[2][1]],
            [m[0][2], m[1][2], m[2][2]],
        ])
    }

    /// Determinant.
    pub fn determinant(&self) -> f64 {
        let m = &self.0;
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    }

    /// Inverse, or [`EngineError::Color`] if the matrix is (near-)singular.
    pub fn inverse(&self) -> EngineResult<Self> {
        let det = self.determinant();
        if !det.is_finite() || det.abs() < 1e-12 {
            return Err(EngineError::Color {
                message: format!("singular colour matrix (det = {det:e})"),
            });
        }
        let m = &self.0;
        let c = |r0: usize, c0: usize, r1: usize, c1: usize| {
            m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0]
        };
        let adj = [
            [c(1, 1, 2, 2), -c(0, 1, 2, 2), c(0, 1, 1, 2)],
            [-c(1, 0, 2, 2), c(0, 0, 2, 2), -c(0, 0, 1, 2)],
            [c(1, 0, 2, 1), -c(0, 0, 2, 1), c(0, 0, 1, 1)],
        ];
        Ok(Self(adj.map(|row| row.map(|v| v / det))))
    }

    /// Largest absolute element-wise difference; used for tolerance tests.
    pub fn max_abs_diff(&self, other: &Self) -> f64 {
        let mut d: f64 = 0.0;
        for r in 0..3 {
            for c in 0..3 {
                d = d.max((self.0[r][c] - other.0[r][c]).abs());
            }
        }
        d
    }

    /// Row-major `f32` copy for upload to GPU kernels.
    pub fn to_f32(&self) -> [[f32; 3]; 3] {
        self.0.map(|row| row.map(|v| v as f32))
    }

    /// RGB → XYZ matrix for the given primaries (white maps to `white.to_xyz()`).
    pub fn rgb_to_xyz(p: &Primaries) -> EngineResult<Self> {
        let col = |xy: [f64; 2]| [xy[0] / xy[1], 1.0, (1.0 - xy[0] - xy[1]) / xy[1]];
        let (r, g, b) = (col(p.red), col(p.green), col(p.blue));
        let m = Self([[r[0], g[0], b[0]], [r[1], g[1], b[1]], [r[2], g[2], b[2]]]);
        let s = m.inverse()?.apply(p.white.to_xyz());
        Ok(m * Self::diagonal(s))
    }
}

impl Mul for ColorMatrix3 {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        let mut out = [[0.0; 3]; 3];
        for (r, row) in out.iter_mut().enumerate() {
            for (c, cell) in row.iter_mut().enumerate() {
                *cell = (0..3).map(|k| self.0[r][k] * rhs.0[k][c]).sum();
            }
        }
        Self(out)
    }
}

/// Chromatic adaptation transform used to move colours between whites.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChromaticAdaptation {
    /// CAT16 (CIECAM16); the engine default for white balance.
    #[default]
    Cat16,
    /// Bradford; what ICC v4 and DNG use, kept for compatibility paths.
    Bradford,
}

impl ChromaticAdaptation {
    fn cone_matrix(self) -> ColorMatrix3 {
        match self {
            Self::Cat16 => ColorMatrix3([
                [0.401288, 0.650173, -0.051461],
                [-0.250268, 1.204414, 0.045854],
                [-0.002079, 0.048952, 0.953127],
            ]),
            Self::Bradford => ColorMatrix3([
                [0.8951, 0.2664, -0.1614],
                [-0.7502, 1.7135, 0.0367],
                [0.0389, -0.0685, 1.0296],
            ]),
        }
    }

    /// von Kries-style XYZ → XYZ matrix adapting `from` white to `to` white.
    pub fn matrix(self, from: WhitePoint, to: WhitePoint) -> EngineResult<ColorMatrix3> {
        let m = self.cone_matrix();
        let (s, d) = (m.apply(from.to_xyz()), m.apply(to.to_xyz()));
        let scale = ColorMatrix3::diagonal([d[0] / s[0], d[1] / s[1], d[2] / s[2]]);
        Ok(m.inverse()? * scale * m)
    }
}

/// Linear RGB working space for pipeline math.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkingSpace {
    /// Linear Rec.2020 primaries, D65. The raw pipeline default.
    #[default]
    LinearRec2020,
    /// Linear ProPhoto / ROMM, D50. Used by the Adobe-compatibility path.
    LinearProPhoto,
    /// Linear Display P3, D65.
    LinearDisplayP3,
    /// Linear sRGB / Rec.709, D65.
    LinearSrgb,
    /// ACEScg (AP1), ACES white.
    AcesCg,
}

impl WorkingSpace {
    /// Primaries and white.
    pub const fn primaries(self) -> Primaries {
        match self {
            Self::LinearRec2020 => Primaries {
                red: [0.708, 0.292],
                green: [0.170, 0.797],
                blue: [0.131, 0.046],
                white: WhitePoint::D65,
            },
            Self::LinearProPhoto => Primaries {
                red: [0.7347, 0.2653],
                green: [0.1596, 0.8404],
                blue: [0.0366, 0.0001],
                white: WhitePoint::D50,
            },
            Self::LinearDisplayP3 => Primaries {
                red: [0.680, 0.320],
                green: [0.265, 0.690],
                blue: [0.150, 0.060],
                white: WhitePoint::D65,
            },
            Self::LinearSrgb => Primaries {
                red: [0.640, 0.330],
                green: [0.300, 0.600],
                blue: [0.150, 0.060],
                white: WhitePoint::D65,
            },
            Self::AcesCg => Primaries {
                red: [0.713, 0.293],
                green: [0.165, 0.830],
                blue: [0.128, 0.044],
                white: WhitePoint::ACES,
            },
        }
    }

    /// Reference white.
    pub const fn white(self) -> WhitePoint {
        self.primaries().white
    }

    /// RGB → XYZ (relative to this space's own white).
    pub fn to_xyz(self) -> ColorMatrix3 {
        // The built-in primaries are well conditioned; failure would be a bug.
        ColorMatrix3::rgb_to_xyz(&self.primaries()).unwrap_or(ColorMatrix3::IDENTITY)
    }

    /// Matrix taking linear RGB in `self` to linear RGB in `target`,
    /// adapting whites with `cat` when they differ.
    pub fn conversion_to(
        self,
        target: WorkingSpace,
        cat: ChromaticAdaptation,
    ) -> EngineResult<ColorMatrix3> {
        let adapt = if self.white() == target.white() {
            ColorMatrix3::IDENTITY
        } else {
            cat.matrix(self.white(), target.white())?
        };
        Ok(target.to_xyz().inverse()? * adapt * self.to_xyz())
    }
}

/// A calibration or scene illuminant, as used by DCP dual-illuminant
/// profiles and white-balance presets.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Illuminant {
    /// CIE standard illuminant A (tungsten).
    StandardA,
    /// CIE D50.
    D50,
    /// CIE D55.
    D55,
    /// CIE D65.
    D65,
    /// CIE D75.
    D75,
    /// CIE F2, cool white fluorescent.
    F2,
    /// CIE F7, broadband fluorescent.
    F7,
    /// CIE F11, tri-band fluorescent.
    F11,
    /// An EXIF `LightSource` code with no single standard white (e.g. 10 = cloudy).
    Exif(u16),
    /// A measured white, e.g. DNG 1.6 `IlluminantData` or a ColorChecker shot.
    Measured(WhitePoint),
}

impl Illuminant {
    /// White point, when the illuminant defines one.
    pub fn white_point(self) -> Option<WhitePoint> {
        Some(match self {
            Self::StandardA => WhitePoint::A,
            Self::D50 => WhitePoint::D50,
            Self::D55 => WhitePoint::D55,
            Self::D65 => WhitePoint::D65,
            Self::D75 => WhitePoint::D75,
            Self::F2 => WhitePoint::F2,
            Self::F7 => WhitePoint::F7,
            Self::F11 => WhitePoint::F11,
            Self::Measured(w) => w,
            Self::Exif(_) => return None,
        })
    }

    /// Nominal correlated colour temperature in kelvin, when standard.
    pub fn cct_kelvin(self) -> Option<f64> {
        Some(match self {
            Self::StandardA => 2856.0,
            Self::D50 => 5003.0,
            Self::D55 => 5503.0,
            Self::D65 => 6504.0,
            Self::D75 => 7504.0,
            Self::F2 => 4230.0,
            Self::F7 => 6500.0,
            Self::F11 => 4000.0,
            Self::Exif(_) | Self::Measured(_) => return None,
        })
    }

    /// Maps an EXIF/DNG `LightSource` / `CalibrationIlluminant` code.
    pub fn from_exif_code(code: u16) -> Self {
        match code {
            17 => Self::StandardA,
            14 => Self::F2,
            20 => Self::D55,
            21 => Self::D65,
            22 => Self::D75,
            23 => Self::D50,
            other => Self::Exif(other),
        }
    }

    /// EXIF/DNG `LightSource` code (255 = "other" for measured/unnamed whites).
    pub fn exif_code(self) -> u16 {
        match self {
            Self::StandardA => 17,
            Self::F2 => 14,
            Self::D55 => 20,
            Self::D65 => 21,
            Self::D75 => 22,
            Self::D50 => 23,
            Self::Exif(code) => code,
            Self::F7 | Self::F11 | Self::Measured(_) => 255,
        }
    }
}

/// Content-addressed handle to an ICC profile.
///
/// The handle is the BLAKE3 digest of the profile bytes, so it is stable
/// across machines and can be stored in recipes and export settings; a
/// profile registry in the colour-management crate resolves it to bytes and a
/// CMM transform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct IccProfileHandle(pub Digest);

impl IccProfileHandle {
    /// Handle for the given profile bytes.
    pub fn from_profile_bytes(bytes: &[u8]) -> Self {
        Self(Digest::derive("engine-api 2026 icc-profile v1", bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f64; 3], b: [f64; 3], tol: f64) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < tol)
    }

    #[test]
    fn rec2020_to_xyz_matches_reference() {
        let m = WorkingSpace::LinearRec2020.to_xyz();
        let reference = ColorMatrix3([
            [0.636958, 0.144617, 0.168881],
            [0.262700, 0.677998, 0.059302],
            [0.000000, 0.028073, 1.060985],
        ]);
        assert!(m.max_abs_diff(&reference) < 1e-5, "{m:?}");
        assert!(close(m.apply([1.0; 3]), WhitePoint::D65.to_xyz(), 1e-12));
    }

    #[test]
    fn inverse_round_trips() {
        let m = WorkingSpace::LinearProPhoto.to_xyz();
        let id = m * m.inverse().unwrap();
        assert!(id.max_abs_diff(&ColorMatrix3::IDENTITY) < 1e-12);
        assert!(
            ColorMatrix3([[1.0, 2.0, 3.0], [2.0, 4.0, 6.0], [0.0, 0.0, 1.0]])
                .inverse()
                .is_err()
        );
    }

    #[test]
    fn adaptation_maps_white_to_white() {
        for cat in [ChromaticAdaptation::Cat16, ChromaticAdaptation::Bradford] {
            let m = cat.matrix(WhitePoint::D50, WhitePoint::D65).unwrap();
            assert!(close(
                m.apply(WhitePoint::D50.to_xyz()),
                WhitePoint::D65.to_xyz(),
                1e-12
            ));
        }
    }

    #[test]
    fn working_space_conversion_preserves_white() {
        let m = WorkingSpace::LinearProPhoto
            .conversion_to(WorkingSpace::LinearRec2020, ChromaticAdaptation::Bradford)
            .unwrap();
        assert!(close(m.apply([1.0; 3]), [1.0; 3], 1e-9));
        let same = WorkingSpace::LinearSrgb
            .conversion_to(WorkingSpace::LinearSrgb, ChromaticAdaptation::Cat16)
            .unwrap();
        assert!(same.max_abs_diff(&ColorMatrix3::IDENTITY) < 1e-12);
    }

    #[test]
    fn illuminant_exif_codes() {
        for il in [
            Illuminant::StandardA,
            Illuminant::D50,
            Illuminant::D55,
            Illuminant::D65,
            Illuminant::D75,
            Illuminant::F2,
        ] {
            assert_eq!(Illuminant::from_exif_code(il.exif_code()), il);
        }
        assert_eq!(Illuminant::from_exif_code(10), Illuminant::Exif(10));
        assert!(Illuminant::Exif(10).white_point().is_none());
        assert_eq!(WorkingSpace::default(), WorkingSpace::LinearRec2020);
    }

    #[test]
    fn icc_handle_is_content_addressed() {
        let a = IccProfileHandle::from_profile_bytes(b"profile-a");
        assert_eq!(a, IccProfileHandle::from_profile_bytes(b"profile-a"));
        assert_ne!(a, IccProfileHandle::from_profile_bytes(b"profile-b"));
        let json = serde_json::to_string(&a).unwrap();
        assert_eq!(serde_json::from_str::<IccProfileHandle>(&json).unwrap(), a);
    }
}
