//! The Adobe Camera Raw `crs:` XMP namespace as a compatibility table
//! (spec 05 §3.2).
//!
//! Every key the spec names has a [`CrsKey`] variant carrying its XMP local
//! name, value type/range as Adobe writes it, and the JSON pointer of the
//! recipe field it maps to (relative to the serialized [`super::Recipe`]).
//! The translation math (PV curves, unit conversions, mask structures) lives
//! in the `recipe`/`sidecar` crates; this table is the single list they, the
//! importer and the exporter agree on.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::EngineError;
use crate::stage::StageId;

/// XMP namespace URI of `crs:`.
pub const CRS_NAMESPACE: &str = "http://ns.adobe.com/camera-raw-settings/1.0/";

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

macro_rules! crs_keys {
    ($($name:ident : $ty:expr => $path:literal),* $(,)?) => {
        /// A key of the Adobe `crs:` namespace that the recipe maps.
        ///
        /// Serialized as its XMP local name (e.g. `"Exposure2012"`).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum CrsKey {
            $(
                #[doc = concat!("`crs:", stringify!($name), "`")]
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

            /// Value type and range.
            pub const fn value_type(self) -> CrsValueType {
                match self { $(CrsKey::$name => $ty),* }
            }

            /// JSON pointer (into the serialized `Recipe`) of the field this
            /// key maps to, or `None` for legacy keys only converted
            /// best-effort on import.
            pub const fn recipe_path(self) -> Option<&'static str> {
                let p = match self { $(CrsKey::$name => $path),* };
                if p.is_empty() { None } else { Some(p) }
            }
        }
    };
}

crs_keys! {
    // Process / basic
    ProcessVersion: TEXT => "/process_version",
    WhiteBalance: CrsValueType::Choice(&["As Shot", "Auto", "Daylight", "Cloudy", "Shade", "Tungsten", "Fluorescent", "Flash", "Custom"]) => "/settings/white_balance/mode",
    Temperature: int(2000, 50000) => "/settings/white_balance/temperature",
    Tint: int(-150, 150) => "/settings/white_balance/tint",
    Exposure2012: real(-5.0, 5.0) => "/settings/tone/exposure",
    Contrast2012: SLIDER => "/settings/tone/contrast",
    Highlights2012: SLIDER => "/settings/tone/highlights",
    Shadows2012: SLIDER => "/settings/tone/shadows",
    Whites2012: SLIDER => "/settings/tone/whites",
    Blacks2012: SLIDER => "/settings/tone/blacks",
    Texture: SLIDER => "/settings/tone/texture",
    Clarity2012: SLIDER => "/settings/tone/clarity",
    Dehaze: SLIDER => "/settings/tone/dehaze",
    Vibrance: SLIDER => "/settings/color/vibrance",
    Saturation: SLIDER => "/settings/color/saturation",

    // Tone curve
    ToneCurvePV2012: CURVE => "/settings/tone/curves/rgb",
    ToneCurvePV2012Red: CURVE => "/settings/tone/curves/red",
    ToneCurvePV2012Green: CURVE => "/settings/tone/curves/green",
    ToneCurvePV2012Blue: CURVE => "/settings/tone/curves/blue",
    ParametricShadows: SLIDER => "/settings/tone/curves/parametric/shadows",
    ParametricDarks: SLIDER => "/settings/tone/curves/parametric/darks",
    ParametricLights: SLIDER => "/settings/tone/curves/parametric/lights",
    ParametricHighlights: SLIDER => "/settings/tone/curves/parametric/highlights",
    ParametricShadowSplit: AMOUNT => "/settings/tone/curves/parametric/shadow_split",
    ParametricMidtoneSplit: AMOUNT => "/settings/tone/curves/parametric/midtone_split",
    ParametricHighlightSplit: AMOUNT => "/settings/tone/curves/parametric/highlight_split",

    // HSL
    HueAdjustmentRed: SLIDER => "/settings/color/hsl/hue/red",
    HueAdjustmentOrange: SLIDER => "/settings/color/hsl/hue/orange",
    HueAdjustmentYellow: SLIDER => "/settings/color/hsl/hue/yellow",
    HueAdjustmentGreen: SLIDER => "/settings/color/hsl/hue/green",
    HueAdjustmentAqua: SLIDER => "/settings/color/hsl/hue/aqua",
    HueAdjustmentBlue: SLIDER => "/settings/color/hsl/hue/blue",
    HueAdjustmentPurple: SLIDER => "/settings/color/hsl/hue/purple",
    HueAdjustmentMagenta: SLIDER => "/settings/color/hsl/hue/magenta",
    SaturationAdjustmentRed: SLIDER => "/settings/color/hsl/saturation/red",
    SaturationAdjustmentOrange: SLIDER => "/settings/color/hsl/saturation/orange",
    SaturationAdjustmentYellow: SLIDER => "/settings/color/hsl/saturation/yellow",
    SaturationAdjustmentGreen: SLIDER => "/settings/color/hsl/saturation/green",
    SaturationAdjustmentAqua: SLIDER => "/settings/color/hsl/saturation/aqua",
    SaturationAdjustmentBlue: SLIDER => "/settings/color/hsl/saturation/blue",
    SaturationAdjustmentPurple: SLIDER => "/settings/color/hsl/saturation/purple",
    SaturationAdjustmentMagenta: SLIDER => "/settings/color/hsl/saturation/magenta",
    LuminanceAdjustmentRed: SLIDER => "/settings/color/hsl/luminance/red",
    LuminanceAdjustmentOrange: SLIDER => "/settings/color/hsl/luminance/orange",
    LuminanceAdjustmentYellow: SLIDER => "/settings/color/hsl/luminance/yellow",
    LuminanceAdjustmentGreen: SLIDER => "/settings/color/hsl/luminance/green",
    LuminanceAdjustmentAqua: SLIDER => "/settings/color/hsl/luminance/aqua",
    LuminanceAdjustmentBlue: SLIDER => "/settings/color/hsl/luminance/blue",
    LuminanceAdjustmentPurple: SLIDER => "/settings/color/hsl/luminance/purple",
    LuminanceAdjustmentMagenta: SLIDER => "/settings/color/hsl/luminance/magenta",

    // Split toning / colour grading (grading reuses the split-toning keys for
    // shadow/highlight hue and saturation)
    SplitToningShadowHue: HUE => "/settings/color/grading/shadows/hue",
    SplitToningShadowSaturation: AMOUNT => "/settings/color/grading/shadows/saturation",
    SplitToningHighlightHue: HUE => "/settings/color/grading/highlights/hue",
    SplitToningHighlightSaturation: AMOUNT => "/settings/color/grading/highlights/saturation",
    SplitToningBalance: SLIDER => "/settings/color/grading/balance",
    ColorGradeShadowLum: SLIDER => "/settings/color/grading/shadows/luminance",
    ColorGradeMidtoneHue: HUE => "/settings/color/grading/midtones/hue",
    ColorGradeMidtoneSat: AMOUNT => "/settings/color/grading/midtones/saturation",
    ColorGradeMidtoneLum: SLIDER => "/settings/color/grading/midtones/luminance",
    ColorGradeHighlightLum: SLIDER => "/settings/color/grading/highlights/luminance",
    ColorGradeGlobalHue: HUE => "/settings/color/grading/global/hue",
    ColorGradeGlobalSat: AMOUNT => "/settings/color/grading/global/saturation",
    ColorGradeGlobalLum: SLIDER => "/settings/color/grading/global/luminance",
    ColorGradeBlending: AMOUNT => "/settings/color/grading/blending",
    PointColors: STRUCT => "/settings/color/point_colors",

    // Detail
    Sharpness: int(0, 150) => "/settings/detail/sharpening/amount",
    SharpenRadius: real(0.5, 3.0) => "/settings/detail/sharpening/radius",
    SharpenDetail: AMOUNT => "/settings/detail/sharpening/detail",
    SharpenEdgeMasking: AMOUNT => "/settings/detail/sharpening/masking",
    LuminanceSmoothing: AMOUNT => "/settings/detail/noise_reduction/luminance",
    LuminanceNoiseReductionDetail: AMOUNT => "/settings/detail/noise_reduction/luminance_detail",
    LuminanceNoiseReductionContrast: AMOUNT => "/settings/detail/noise_reduction/luminance_contrast",
    ColorNoiseReduction: AMOUNT => "/settings/detail/noise_reduction/color",
    ColorNoiseReductionDetail: AMOUNT => "/settings/detail/noise_reduction/color_detail",
    ColorNoiseReductionSmoothness: AMOUNT => "/settings/detail/noise_reduction/color_smoothness",

    // AI enhance
    EnhanceDenoiseAlreadyApplied: BOOL => "/settings/denoise/method",
    EnhanceDenoiseVersion: TEXT => "/settings/denoise/method",
    EnhanceDenoiseLumAmount: AMOUNT => "/settings/denoise/amount",
    EnhanceDetailsAlreadyApplied: BOOL => "/settings/demosaic/method",
    EnhanceSuperResolutionAlreadyApplied: BOOL => "",

    // Lens corrections
    LensProfileEnable: FLAG => "/settings/lens/profile",
    LensProfileSetup: CrsValueType::Choice(&["LensDefaults", "Auto", "Custom"]) => "/settings/lens/profile",
    LensProfileName: TEXT => "/settings/lens/profile",
    LensProfileFilename: TEXT => "/settings/lens/profile",
    LensProfileDigest: TEXT => "/settings/lens/profile",
    LensProfileDistortionScale: int(0, 200) => "/settings/lens/distortion_scale",
    LensProfileChromaticAberrationScale: int(0, 200) => "/settings/lens/chromatic_aberration_scale",
    LensProfileVignettingScale: int(0, 200) => "/settings/lens/vignetting_scale",
    LensManualDistortionAmount: SLIDER => "/settings/lens/manual_distortion",
    VignetteAmount: SLIDER => "/settings/lens/manual_vignetting",
    VignetteMidpoint: AMOUNT => "/settings/lens/manual_vignetting_midpoint",
    AutoLateralCA: FLAG => "/settings/lens/remove_chromatic_aberration",
    ChromaticAberrationR: SLIDER => "",
    ChromaticAberrationB: SLIDER => "",
    DefringePurpleAmount: int(0, 20) => "/settings/lens/defringe_purple/amount",
    DefringePurpleHueLo: AMOUNT => "/settings/lens/defringe_purple/hue_range",
    DefringePurpleHueHi: AMOUNT => "/settings/lens/defringe_purple/hue_range",
    DefringeGreenAmount: int(0, 20) => "/settings/lens/defringe_green/amount",
    DefringeGreenHueLo: AMOUNT => "/settings/lens/defringe_green/hue_range",
    DefringeGreenHueHi: AMOUNT => "/settings/lens/defringe_green/hue_range",

    // Transform / Upright
    PerspectiveUpright: int(0, 5) => "/settings/geometry/upright/mode",
    PerspectiveVertical: SLIDER => "/settings/geometry/transform/vertical",
    PerspectiveHorizontal: SLIDER => "/settings/geometry/transform/horizontal",
    PerspectiveRotate: real(-10.0, 10.0) => "/settings/geometry/transform/rotate",
    PerspectiveAspect: SLIDER => "/settings/geometry/transform/aspect",
    PerspectiveScale: int(50, 150) => "/settings/geometry/transform/scale",
    PerspectiveX: real(-100.0, 100.0) => "/settings/geometry/transform/offset_x",
    PerspectiveY: real(-100.0, 100.0) => "/settings/geometry/transform/offset_y",

    // Crop
    HasCrop: BOOL => "/settings/geometry/crop/rect",
    CropTop: real(0.0, 1.0) => "/settings/geometry/crop/rect/top",
    CropLeft: real(0.0, 1.0) => "/settings/geometry/crop/rect/left",
    CropBottom: real(0.0, 1.0) => "/settings/geometry/crop/rect/bottom",
    CropRight: real(0.0, 1.0) => "/settings/geometry/crop/rect/right",
    CropAngle: real(-45.0, 45.0) => "/settings/geometry/crop/angle",
    CropConstrainToWarp: FLAG => "/settings/geometry/constrain_crop",

    // Effects
    PostCropVignetteAmount: SLIDER => "/settings/effects/vignette/amount",
    PostCropVignetteMidpoint: AMOUNT => "/settings/effects/vignette/midpoint",
    PostCropVignetteFeather: AMOUNT => "/settings/effects/vignette/feather",
    PostCropVignetteRoundness: SLIDER => "/settings/effects/vignette/roundness",
    PostCropVignetteStyle: int(1, 3) => "/settings/effects/vignette/style",
    PostCropVignetteHighlightContrast: AMOUNT => "/settings/effects/vignette/highlights",
    GrainAmount: AMOUNT => "/settings/effects/grain/amount",
    GrainSize: AMOUNT => "/settings/effects/grain/size",
    GrainFrequency: AMOUNT => "/settings/effects/grain/roughness",
    LensBlur: STRUCT => "/settings/effects/lens_blur",

    // Profile / calibration
    CameraProfile: TEXT => "/settings/camera_profile/profile",
    CameraProfileDigest: TEXT => "/settings/camera_profile/profile",
    Look: STRUCT => "/settings/camera_profile/look",
    ShadowTint: SLIDER => "/settings/camera_profile/calibration/shadow_tint",
    RedHue: SLIDER => "/settings/camera_profile/calibration/red_hue",
    RedSaturation: SLIDER => "/settings/camera_profile/calibration/red_saturation",
    GreenHue: SLIDER => "/settings/camera_profile/calibration/green_hue",
    GreenSaturation: SLIDER => "/settings/camera_profile/calibration/green_saturation",
    BlueHue: SLIDER => "/settings/camera_profile/calibration/blue_hue",
    BlueSaturation: SLIDER => "/settings/camera_profile/calibration/blue_saturation",

    // Locals / retouch
    MaskGroupBasedCorrections: STRUCT => "/settings/locals/adjustments",
    RetouchAreas: STRUCT => "/settings/locals/retouch",
    RetouchInfo: STRUCT => "/settings/locals/retouch",

    // HDR
    HDREditMode: FLAG => "/settings/output/hdr",
    HDRMaxValue: real(0.0, 16.0) => "/settings/output/hdr_headroom_stops",
}

impl CrsKey {
    /// Looks up a key by XMP local name (without the `crs:` prefix).
    pub fn from_xmp_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|k| k.xmp_name() == name)
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
        write!(f, "crs:{}", self.xmp_name())
    }
}

impl FromStr for CrsKey {
    type Err = EngineError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let local = s.strip_prefix("crs:").unwrap_or(s);
        Self::from_xmp_name(local).ok_or_else(|| EngineError::not_found("crs key", s))
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
            assert_eq!(
                format!("crs:{}", k.xmp_name()).parse::<CrsKey>().unwrap(),
                k
            );
            let json = serde_json::to_string(&k).unwrap();
            assert_eq!(serde_json::from_str::<CrsKey>(&json).unwrap(), k);
        }
        assert!(CrsKey::ALL.len() > 120);
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
