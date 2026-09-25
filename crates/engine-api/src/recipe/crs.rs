//! The Adobe Camera Raw `crs:` XMP namespace (plus the few `aux:` develop
//! properties) as a compatibility table (spec 05 §3.2).
//!
//! Every key the spec names has a [`CrsKey`] variant carrying its XMP
//! namespace and local name, value type/range as Adobe writes it, and its
//! [`CrsTarget`]: the JSON pointer of the recipe field it maps to (relative to
//! the serialized [`super::Recipe`]), or why it has none. The translation math
//! (PV curves, unit conversions, mask structures) lives in the
//! `recipe`/`sidecar` crates; this table is the single list they, the importer
//! and the exporter agree on.
//!
//! Names and types were checked against ExifTool's `XMP.pm` `crs`/`aux`
//! tables (contracts 1.1); see `CONTRACTS.md` for what remains unverified.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::EngineError;
use crate::stage::StageId;

/// XMP namespace URI of `crs:`.
pub const CRS_NAMESPACE: &str = "http://ns.adobe.com/camera-raw-settings/1.0/";

/// XMP namespace URI of `aux:` (Adobe auxiliary EXIF; hosts the `Enhance*`
/// already-applied properties).
pub const AUX_NAMESPACE: &str = "http://ns.adobe.com/exif/1.0/aux/";

/// XMP namespace URI of `xmp:` (XMP basic).
pub const XMP_NAMESPACE: &str = "http://ns.adobe.com/xap/1.0/";

/// XMP namespace URI of `xmpDM:` (Dynamic Media; hosts `pick` and `good`).
pub const XMP_DM_NAMESPACE: &str = "http://ns.adobe.com/xmp/1.0/DynamicMedia/";

/// XMP namespace URI of the engine's private `ts:` properties
/// (`ts:NativeRevision`, `ts:Mark`, `ts:MarkLabel`).
pub const TS_NAMESPACE: &str = "https://tessera.photo/ns/sidecar/1.0/";

/// `ts:` local name of the native-revision companion written next to
/// `crs:ProcessVersion` when a native recipe is exported (see
/// [`super::ProcessVersion::crs_value`]).
pub const NATIVE_REVISION_PROPERTY: &str = "NativeRevision";

/// The XMP namespace a table key lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum XmpNamespace {
    /// Camera Raw settings, `crs:`.
    Crs,
    /// Adobe auxiliary EXIF, `aux:`.
    Aux,
    /// XMP basic, `xmp:`. No develop key lives here today; reserved so the
    /// table can name one without another contract change.
    Xmp,
}

impl XmpNamespace {
    /// Conventional prefix (`"crs"`, `"aux"`, `"xmp"`).
    pub const fn prefix(self) -> &'static str {
        match self {
            Self::Crs => "crs",
            Self::Aux => "aux",
            Self::Xmp => "xmp",
        }
    }

    /// Namespace URI; matching must use this, never the prefix.
    pub const fn uri(self) -> &'static str {
        match self {
            Self::Crs => CRS_NAMESPACE,
            Self::Aux => AUX_NAMESPACE,
            Self::Xmp => XMP_NAMESPACE,
        }
    }
}

/// What a table key maps to in the recipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrsTarget {
    /// JSON pointer into the serialized `Recipe`. Several keys may share a
    /// pointer when they jointly encode one value (e.g. the lens profile).
    Field(&'static str),
    /// Legacy (PV1/2 manual CA): no native field; the importer flags the key
    /// and converts it best-effort.
    Legacy,
    /// Describes the file rather than instructing the renderer (Enhance
    /// already-applied metadata). Never exported from a recipe; the importer
    /// records the raw value in [`super::Provenance`].
    Informational,
}

/// Type and range of a `crs:` value as Adobe serializes it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CrsValueType {
    /// Real number in `[min, max]`.
    Real {
        /// Minimum.
        min: f64,
        /// Maximum.
        max: f64,
    },
    /// Integer in `[min, max]` (booleans stored as 0/1 included).
    Integer {
        /// Minimum.
        min: i64,
        /// Maximum.
        max: i64,
    },
    /// `True` / `False`.
    Boolean,
    /// Free text (names, digests, version strings).
    Text,
    /// One of a closed set of strings.
    Choice(&'static [&'static str]),
    /// `rdf:Seq` of `"x, y"` points on a 0–255 grid (tone curves).
    PointList,
    /// Structured XMP (`rdf:Seq`/`rdf:Description` trees: masks, retouch, looks).
    Structure,
}

impl CrsValueType {
    /// True if `v` lies within a numeric range (non-numeric types accept all).
    pub fn accepts(&self, v: f64) -> bool {
        match *self {
            Self::Real { min, max } => (min..=max).contains(&v),
            Self::Integer { min, max } => {
                v.fract() == 0.0 && (min as f64..=max as f64).contains(&v)
            }
            _ => true,
        }
    }
}

const fn real(min: f64, max: f64) -> CrsValueType {
    CrsValueType::Real { min, max }
}

const fn int(min: i64, max: i64) -> CrsValueType {
    CrsValueType::Integer { min, max }
}

const SLIDER: CrsValueType = int(-100, 100);
const AMOUNT: CrsValueType = int(0, 100);
const HUE: CrsValueType = int(0, 359);
const FLAG: CrsValueType = int(0, 1);
const TEXT: CrsValueType = CrsValueType::Text;
const BOOL: CrsValueType = CrsValueType::Boolean;
const CURVE: CrsValueType = CrsValueType::PointList;
const STRUCT: CrsValueType = CrsValueType::Structure;

use CrsTarget::{Field, Informational, Legacy};
use XmpNamespace::{Aux, Crs};

macro_rules! crs_keys {
    ($($name:ident : $ns:ident, $ty:expr => $target:expr),* $(,)?) => {
        /// A develop key of the Adobe XMP schema that the recipe maps (mostly
        /// `crs:`; see [`CrsKey::namespace`]).
        ///
        /// Serialized as its XMP local name (e.g. `"Exposure2012"`); local
        /// names are unique across namespaces.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum CrsKey {
            $(
                #[doc = concat!("`", stringify!($name), "` (namespace: ", stringify!($ns), ")")]
                $name,
            )*
        }

        impl CrsKey {
            /// Every key, in table order.
            pub const ALL: &'static [CrsKey] = &[$(CrsKey::$name),*];

            /// XMP local name.
            pub const fn xmp_name(self) -> &'static str {
                match self { $(CrsKey::$name => stringify!($name)),* }
            }

            /// XMP namespace the property lives in.
            pub const fn namespace(self) -> XmpNamespace {
                match self { $(CrsKey::$name => $ns),* }
            }

            /// Value type and range.
            pub const fn value_type(self) -> CrsValueType {
                match self { $(CrsKey::$name => $ty),* }
            }

            /// What the key maps to.
            pub const fn target(self) -> CrsTarget {
                match self { $(CrsKey::$name => $target),* }
            }
        }
    };
}

crs_keys! {
    // Process / basic
    ProcessVersion: Crs, TEXT => Field("/process_version"),
    WhiteBalance: Crs, CrsValueType::Choice(&["As Shot", "Auto", "Daylight", "Cloudy", "Shade", "Tungsten", "Fluorescent", "Flash", "Custom"]) => Field("/settings/white_balance/mode"),
    Temperature: Crs, int(2000, 50000) => Field("/settings/white_balance/temperature"),
    Tint: Crs, int(-150, 150) => Field("/settings/white_balance/tint"),
    Exposure2012: Crs, real(-5.0, 5.0) => Field("/settings/tone/exposure"),
    Contrast2012: Crs, SLIDER => Field("/settings/tone/contrast"),
    Highlights2012: Crs, SLIDER => Field("/settings/tone/highlights"),
    Shadows2012: Crs, SLIDER => Field("/settings/tone/shadows"),
    Whites2012: Crs, SLIDER => Field("/settings/tone/whites"),
    Blacks2012: Crs, SLIDER => Field("/settings/tone/blacks"),
    Texture: Crs, SLIDER => Field("/settings/tone/texture"),
    Clarity2012: Crs, SLIDER => Field("/settings/tone/clarity"),
    // ExifTool types Dehaze as real (1.1); integer values remain valid.
    Dehaze: Crs, real(-100.0, 100.0) => Field("/settings/tone/dehaze"),
    Vibrance: Crs, SLIDER => Field("/settings/color/vibrance"),
    Saturation: Crs, SLIDER => Field("/settings/color/saturation"),

    // Tone curve
    ToneCurvePV2012: Crs, CURVE => Field("/settings/tone/curves/rgb"),
    ToneCurvePV2012Red: Crs, CURVE => Field("/settings/tone/curves/red"),
    ToneCurvePV2012Green: Crs, CURVE => Field("/settings/tone/curves/green"),
    ToneCurvePV2012Blue: Crs, CURVE => Field("/settings/tone/curves/blue"),
    ParametricShadows: Crs, SLIDER => Field("/settings/tone/curves/parametric/shadows"),
    ParametricDarks: Crs, SLIDER => Field("/settings/tone/curves/parametric/darks"),
    ParametricLights: Crs, SLIDER => Field("/settings/tone/curves/parametric/lights"),
    ParametricHighlights: Crs, SLIDER => Field("/settings/tone/curves/parametric/highlights"),
    ParametricShadowSplit: Crs, AMOUNT => Field("/settings/tone/curves/parametric/shadow_split"),
    ParametricMidtoneSplit: Crs, AMOUNT => Field("/settings/tone/curves/parametric/midtone_split"),
    ParametricHighlightSplit: Crs, AMOUNT => Field("/settings/tone/curves/parametric/highlight_split"),

    // HSL
    HueAdjustmentRed: Crs, SLIDER => Field("/settings/color/hsl/hue/red"),
    HueAdjustmentOrange: Crs, SLIDER => Field("/settings/color/hsl/hue/orange"),
    HueAdjustmentYellow: Crs, SLIDER => Field("/settings/color/hsl/hue/yellow"),
    HueAdjustmentGreen: Crs, SLIDER => Field("/settings/color/hsl/hue/green"),
    HueAdjustmentAqua: Crs, SLIDER => Field("/settings/color/hsl/hue/aqua"),
    HueAdjustmentBlue: Crs, SLIDER => Field("/settings/color/hsl/hue/blue"),
    HueAdjustmentPurple: Crs, SLIDER => Field("/settings/color/hsl/hue/purple"),
    HueAdjustmentMagenta: Crs, SLIDER => Field("/settings/color/hsl/hue/magenta"),
    SaturationAdjustmentRed: Crs, SLIDER => Field("/settings/color/hsl/saturation/red"),
    SaturationAdjustmentOrange: Crs, SLIDER => Field("/settings/color/hsl/saturation/orange"),
    SaturationAdjustmentYellow: Crs, SLIDER => Field("/settings/color/hsl/saturation/yellow"),
    SaturationAdjustmentGreen: Crs, SLIDER => Field("/settings/color/hsl/saturation/green"),
    SaturationAdjustmentAqua: Crs, SLIDER => Field("/settings/color/hsl/saturation/aqua"),
    SaturationAdjustmentBlue: Crs, SLIDER => Field("/settings/color/hsl/saturation/blue"),
    SaturationAdjustmentPurple: Crs, SLIDER => Field("/settings/color/hsl/saturation/purple"),
    SaturationAdjustmentMagenta: Crs, SLIDER => Field("/settings/color/hsl/saturation/magenta"),
    LuminanceAdjustmentRed: Crs, SLIDER => Field("/settings/color/hsl/luminance/red"),
    LuminanceAdjustmentOrange: Crs, SLIDER => Field("/settings/color/hsl/luminance/orange"),
    LuminanceAdjustmentYellow: Crs, SLIDER => Field("/settings/color/hsl/luminance/yellow"),
    LuminanceAdjustmentGreen: Crs, SLIDER => Field("/settings/color/hsl/luminance/green"),
    LuminanceAdjustmentAqua: Crs, SLIDER => Field("/settings/color/hsl/luminance/aqua"),
    LuminanceAdjustmentBlue: Crs, SLIDER => Field("/settings/color/hsl/luminance/blue"),
    LuminanceAdjustmentPurple: Crs, SLIDER => Field("/settings/color/hsl/luminance/purple"),
    LuminanceAdjustmentMagenta: Crs, SLIDER => Field("/settings/color/hsl/luminance/magenta"),

    // Split toning / colour grading (grading reuses the split-toning keys for
    // shadow/highlight hue and saturation)
    SplitToningShadowHue: Crs, HUE => Field("/settings/color/grading/shadows/hue"),
    SplitToningShadowSaturation: Crs, AMOUNT => Field("/settings/color/grading/shadows/saturation"),
    SplitToningHighlightHue: Crs, HUE => Field("/settings/color/grading/highlights/hue"),
    SplitToningHighlightSaturation: Crs, AMOUNT => Field("/settings/color/grading/highlights/saturation"),
    SplitToningBalance: Crs, SLIDER => Field("/settings/color/grading/balance"),
    ColorGradeShadowLum: Crs, SLIDER => Field("/settings/color/grading/shadows/luminance"),
    ColorGradeMidtoneHue: Crs, HUE => Field("/settings/color/grading/midtones/hue"),
    ColorGradeMidtoneSat: Crs, AMOUNT => Field("/settings/color/grading/midtones/saturation"),
    ColorGradeMidtoneLum: Crs, SLIDER => Field("/settings/color/grading/midtones/luminance"),
    ColorGradeHighlightLum: Crs, SLIDER => Field("/settings/color/grading/highlights/luminance"),
    ColorGradeGlobalHue: Crs, HUE => Field("/settings/color/grading/global/hue"),
    ColorGradeGlobalSat: Crs, AMOUNT => Field("/settings/color/grading/global/saturation"),
    ColorGradeGlobalLum: Crs, SLIDER => Field("/settings/color/grading/global/luminance"),
    ColorGradeBlending: Crs, AMOUNT => Field("/settings/color/grading/blending"),
    PointColors: Crs, STRUCT => Field("/settings/color/point_colors"),

    // Detail
    Sharpness: Crs, int(0, 150) => Field("/settings/detail/sharpening/amount"),
    SharpenRadius: Crs, real(0.5, 3.0) => Field("/settings/detail/sharpening/radius"),
    SharpenDetail: Crs, AMOUNT => Field("/settings/detail/sharpening/detail"),
    SharpenEdgeMasking: Crs, AMOUNT => Field("/settings/detail/sharpening/masking"),
    LuminanceSmoothing: Crs, AMOUNT => Field("/settings/detail/noise_reduction/luminance"),
    LuminanceNoiseReductionDetail: Crs, AMOUNT => Field("/settings/detail/noise_reduction/luminance_detail"),
    LuminanceNoiseReductionContrast: Crs, AMOUNT => Field("/settings/detail/noise_reduction/luminance_contrast"),
    ColorNoiseReduction: Crs, AMOUNT => Field("/settings/detail/noise_reduction/color"),
    ColorNoiseReductionDetail: Crs, AMOUNT => Field("/settings/detail/noise_reduction/color_detail"),
    ColorNoiseReductionSmoothness: Crs, AMOUNT => Field("/settings/detail/noise_reduction/color_smoothness"),

    // AI enhance (1.1: `aux:`, not `crs:`). These describe pixels an Adobe
    // Enhance pass already baked into the file (an enhanced DNG); they are not
    // instructions to run anything, so they only go to `/provenance`.
    EnhanceDenoiseAlreadyApplied: Aux, BOOL => Informational,
    EnhanceDenoiseVersion: Aux, TEXT => Informational,
    EnhanceDenoiseLumaAmount: Aux, AMOUNT => Informational,
    EnhanceDetailsAlreadyApplied: Aux, BOOL => Informational,
    EnhanceDetailsVersion: Aux, TEXT => Informational,
    EnhanceSuperResolutionAlreadyApplied: Aux, BOOL => Informational,
    EnhanceSuperResolutionVersion: Aux, TEXT => Informational,
    EnhanceSuperResolutionScale: Aux, TEXT => Informational,

    // Lens corrections
    // The five profile keys together encode one `LensProfileSource`; a named
    // profile round-trips through `LensProfileRef { name, filename, digest, setup }`.
    LensProfileEnable: Crs, FLAG => Field("/settings/lens/profile"),
    LensProfileSetup: Crs, CrsValueType::Choice(&["LensDefaults", "Auto", "Custom"]) => Field("/settings/lens/profile"),
    LensProfileName: Crs, TEXT => Field("/settings/lens/profile"),
    LensProfileFilename: Crs, TEXT => Field("/settings/lens/profile"),
    LensProfileDigest: Crs, TEXT => Field("/settings/lens/profile"),
    LensProfileDistortionScale: Crs, int(0, 200) => Field("/settings/lens/distortion_scale"),
    LensProfileChromaticAberrationScale: Crs, int(0, 200) => Field("/settings/lens/chromatic_aberration_scale"),
    LensProfileVignettingScale: Crs, int(0, 200) => Field("/settings/lens/vignetting_scale"),
    LensManualDistortionAmount: Crs, SLIDER => Field("/settings/lens/manual_distortion"),
    VignetteAmount: Crs, SLIDER => Field("/settings/lens/manual_vignetting"),
    VignetteMidpoint: Crs, AMOUNT => Field("/settings/lens/manual_vignetting_midpoint"),
    AutoLateralCA: Crs, FLAG => Field("/settings/lens/remove_chromatic_aberration"),
    ChromaticAberrationR: Crs, SLIDER => Legacy,
    ChromaticAberrationB: Crs, SLIDER => Legacy,
    DefringePurpleAmount: Crs, int(0, 20) => Field("/settings/lens/defringe_purple/amount"),
    DefringePurpleHueLo: Crs, AMOUNT => Field("/settings/lens/defringe_purple/hue_range"),
    DefringePurpleHueHi: Crs, AMOUNT => Field("/settings/lens/defringe_purple/hue_range"),
    DefringeGreenAmount: Crs, int(0, 20) => Field("/settings/lens/defringe_green/amount"),
    DefringeGreenHueLo: Crs, AMOUNT => Field("/settings/lens/defringe_green/hue_range"),
    DefringeGreenHueHi: Crs, AMOUNT => Field("/settings/lens/defringe_green/hue_range"),

    // Transform / Upright
    PerspectiveUpright: Crs, int(0, 5) => Field("/settings/geometry/upright/mode"),
    PerspectiveVertical: Crs, SLIDER => Field("/settings/geometry/transform/vertical"),
    PerspectiveHorizontal: Crs, SLIDER => Field("/settings/geometry/transform/horizontal"),
    PerspectiveRotate: Crs, real(-10.0, 10.0) => Field("/settings/geometry/transform/rotate"),
    PerspectiveAspect: Crs, SLIDER => Field("/settings/geometry/transform/aspect"),
    PerspectiveScale: Crs, int(50, 150) => Field("/settings/geometry/transform/scale"),
    PerspectiveX: Crs, real(-100.0, 100.0) => Field("/settings/geometry/transform/offset_x"),
    PerspectiveY: Crs, real(-100.0, 100.0) => Field("/settings/geometry/transform/offset_y"),

    // Crop
    HasCrop: Crs, BOOL => Field("/settings/geometry/crop/rect"),
    CropTop: Crs, real(0.0, 1.0) => Field("/settings/geometry/crop/rect/top"),
    CropLeft: Crs, real(0.0, 1.0) => Field("/settings/geometry/crop/rect/left"),
    CropBottom: Crs, real(0.0, 1.0) => Field("/settings/geometry/crop/rect/bottom"),
    CropRight: Crs, real(0.0, 1.0) => Field("/settings/geometry/crop/rect/right"),
    CropAngle: Crs, real(-45.0, 45.0) => Field("/settings/geometry/crop/angle"),
    CropConstrainToWarp: Crs, FLAG => Field("/settings/geometry/constrain_crop"),

    // Effects
    PostCropVignetteAmount: Crs, SLIDER => Field("/settings/effects/vignette/amount"),
    PostCropVignetteMidpoint: Crs, AMOUNT => Field("/settings/effects/vignette/midpoint"),
    PostCropVignetteFeather: Crs, AMOUNT => Field("/settings/effects/vignette/feather"),
    PostCropVignetteRoundness: Crs, SLIDER => Field("/settings/effects/vignette/roundness"),
    // 1 Highlight Priority, 2 Color Priority, 3 Paint Overlay (ExifTool).
    PostCropVignetteStyle: Crs, int(1, 3) => Field("/settings/effects/vignette/style"),
    PostCropVignetteHighlightContrast: Crs, AMOUNT => Field("/settings/effects/vignette/highlights"),
    GrainAmount: Crs, AMOUNT => Field("/settings/effects/grain/amount"),
    GrainSize: Crs, AMOUNT => Field("/settings/effects/grain/size"),
    GrainFrequency: Crs, AMOUNT => Field("/settings/effects/grain/roughness"),
    LensBlur: Crs, STRUCT => Field("/settings/effects/lens_blur"),

    // Profile / calibration
    CameraProfile: Crs, TEXT => Field("/settings/camera_profile/profile/name"),
    CameraProfileDigest: Crs, TEXT => Field("/settings/camera_profile/profile/digest"),
    Look: Crs, STRUCT => Field("/settings/camera_profile/look"),
    ShadowTint: Crs, SLIDER => Field("/settings/camera_profile/calibration/shadow_tint"),
    RedHue: Crs, SLIDER => Field("/settings/camera_profile/calibration/red_hue"),
    RedSaturation: Crs, SLIDER => Field("/settings/camera_profile/calibration/red_saturation"),
    GreenHue: Crs, SLIDER => Field("/settings/camera_profile/calibration/green_hue"),
    GreenSaturation: Crs, SLIDER => Field("/settings/camera_profile/calibration/green_saturation"),
    BlueHue: Crs, SLIDER => Field("/settings/camera_profile/calibration/blue_hue"),
    BlueSaturation: Crs, SLIDER => Field("/settings/camera_profile/calibration/blue_saturation"),

    // Locals / retouch
    MaskGroupBasedCorrections: Crs, STRUCT => Field("/settings/locals/adjustments"),
    RetouchAreas: Crs, STRUCT => Field("/settings/locals/retouch"),
    RetouchInfo: Crs, STRUCT => Field("/settings/locals/retouch"),

    // HDR. ExifTool: HDREditMode integer (only 0 observed; 1 = HDR on is
    // assumed), HDRMaxValue real (unit and range not established by Adobe).
    HDREditMode: Crs, FLAG => Field("/settings/output/hdr"),
    HDRMaxValue: Crs, real(0.0, 16.0) => Field("/settings/output/hdr_headroom_stops"),
}

impl CrsKey {
    /// JSON pointer (into the serialized `Recipe`) of the field this key maps
    /// to, or `None` for legacy and informational keys.
    pub const fn recipe_path(self) -> Option<&'static str> {
        match self.target() {
            CrsTarget::Field(p) => Some(p),
            CrsTarget::Legacy | CrsTarget::Informational => None,
        }
    }

    /// True for keys that describe the file and only feed provenance.
    pub const fn is_informational(self) -> bool {
        matches!(self.target(), CrsTarget::Informational)
    }

    /// Qualified name with the conventional prefix, e.g. `"aux:EnhanceDenoiseVersion"`.
    /// This is also the key under which importers record informational
    /// values in [`super::Provenance::properties`].
    pub fn qualified_name(self) -> String {
        format!("{}:{}", self.namespace().prefix(), self.xmp_name())
    }

    /// Looks up a key by XMP local name (without a prefix).
    pub fn from_xmp_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|k| k.xmp_name() == name)
    }

    /// Looks up a key by namespace URI and local name.
    pub fn from_xmp(namespace_uri: &str, name: &str) -> Option<Self> {
        Self::from_xmp_name(name).filter(|k| k.namespace().uri() == namespace_uri)
    }

    /// Pipeline stage the target field belongs to, derived from the path.
    pub fn stage(self) -> Option<StageId> {
        let seg = self
            .recipe_path()?
            .strip_prefix("/settings/")?
            .split('/')
            .next()?;
        StageId::ALL.into_iter().find(|s| s.name() == seg)
    }
}

impl fmt::Display for CrsKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.namespace().prefix(), self.xmp_name())
    }
}

impl FromStr for CrsKey {
    type Err = EngineError;
    /// Accepts a bare local name or `prefix:Name` with the key's own prefix.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let key = match s.split_once(':') {
            Some((prefix, local)) => {
                Self::from_xmp_name(local).filter(|k| k.namespace().prefix() == prefix)
            }
            None => Self::from_xmp_name(s),
        };
        key.ok_or_else(|| EngineError::not_found("crs key", s))
    }
}

impl Serialize for CrsKey {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.xmp_name())
    }
}

impl<'de> Deserialize<'de> for CrsKey {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::Recipe;
    use std::collections::HashSet;

    #[test]
    fn names_unique_and_round_trip() {
        let mut seen = HashSet::new();
        for &k in CrsKey::ALL {
            assert!(seen.insert(k.xmp_name()), "duplicate {k}");
            assert_eq!(CrsKey::from_xmp_name(k.xmp_name()), Some(k));
            assert_eq!(k.qualified_name().parse::<CrsKey>().unwrap(), k);
            assert_eq!(k.to_string(), k.qualified_name());
            assert_eq!(CrsKey::from_xmp(k.namespace().uri(), k.xmp_name()), Some(k));
            let json = serde_json::to_string(&k).unwrap();
            assert_eq!(serde_json::from_str::<CrsKey>(&json).unwrap(), k);
        }
        assert!(CrsKey::ALL.len() > 120);
        assert!("crs:EnhanceDenoiseVersion".parse::<CrsKey>().is_err());
        assert!(CrsKey::from_xmp(CRS_NAMESPACE, "EnhanceDenoiseVersion").is_none());
        assert_eq!(
            "aux:EnhanceDenoiseLumaAmount".parse::<CrsKey>().unwrap(),
            CrsKey::EnhanceDenoiseLumaAmount
        );
    }

    #[test]
    fn enhance_keys_are_informational_aux() {
        for &k in CrsKey::ALL {
            let enhance = k.xmp_name().starts_with("Enhance");
            assert_eq!(enhance, k.namespace() == XmpNamespace::Aux, "{k}");
            assert_eq!(enhance, k.is_informational(), "{k}");
            if k.is_informational() {
                assert_eq!(k.recipe_path(), None);
            }
        }
        assert_eq!(CrsKey::ChromaticAberrationR.target(), CrsTarget::Legacy);
        assert_eq!(
            CrsKey::PostCropVignetteStyle.value_type(),
            CrsValueType::Integer { min: 1, max: 3 }
        );
        assert!(matches!(
            CrsKey::HDRMaxValue.value_type(),
            CrsValueType::Real { .. }
        ));
    }

    #[test]
    fn every_target_path_exists_in_the_recipe_schema() {
        let recipe = serde_json::to_value(Recipe::default()).unwrap();
        for &k in CrsKey::ALL {
            if let Some(path) = k.recipe_path() {
                assert!(
                    recipe.pointer(path).is_some(),
                    "{k} → {path} does not resolve"
                );
                if path.starts_with("/settings/") {
                    assert!(k.stage().is_some(), "{k} has no stage");
                }
            }
        }
        assert_eq!(CrsKey::Exposure2012.stage(), Some(StageId::Tone));
        assert_eq!(CrsKey::CropAngle.stage(), Some(StageId::Geometry));
        assert_eq!(CrsKey::ProcessVersion.stage(), None);
    }

    #[test]
    fn ranges() {
        assert!(CrsKey::Exposure2012.value_type().accepts(4.5));
        assert!(!CrsKey::Exposure2012.value_type().accepts(5.5));
        assert!(!CrsKey::Contrast2012.value_type().accepts(10.5));
        assert!(CrsKey::Contrast2012.value_type().accepts(-100.0));
    }
}
