//! Develop settings: one parameter struct per pipeline stage, in pipeline order.
//!
//! Units follow the user-facing controls so recipes are readable and so the
//! `crs:` mapping is mostly one-to-one: exposure in EV, most sliders on
//! `-100..=100` (or `0..=100` for amounts), temperatures in kelvin, hues in
//! degrees, geometry in normalised image coordinates (`0..=1`, origin top-left).
//! Every struct is `#[serde(default)]`: a missing field means "neutral".

use serde::{Deserialize, Serialize};

use super::mask::{LocalAdjustment, RetouchOperation};
use crate::color::{IccProfileHandle, WorkingSpace};
use crate::id::{LensProfileId, ModelRef, ProfileId, StyleId};
use crate::stage::{ParamHash, StageId, StageParams};

/// The complete render-affecting state of an image, stage by stage.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DevelopSettings {
    /// [`StageId::Decode`].
    pub decode: DecodeSettings,
    /// [`StageId::Linearize`].
    pub linearize: LinearizeSettings,
    /// [`StageId::Denoise`].
    pub denoise: DenoiseSettings,
    /// [`StageId::Demosaic`].
    pub demosaic: DemosaicSettings,
    /// [`StageId::Lens`].
    pub lens: LensSettings,
    /// [`StageId::CameraProfile`].
    pub camera_profile: CameraProfileSettings,
    /// [`StageId::WhiteBalance`].
    pub white_balance: WhiteBalanceSettings,
    /// [`StageId::Detail`].
    pub detail: DetailSettings,
    /// [`StageId::Tone`].
    pub tone: ToneSettings,
    /// [`StageId::Color`].
    pub color: ColorSettings,
    /// [`StageId::Locals`].
    pub locals: LocalsSettings,
    /// [`StageId::Effects`].
    pub effects: EffectsSettings,
    /// [`StageId::Geometry`].
    pub geometry: GeometrySettings,
    /// [`StageId::Output`].
    pub output: OutputSettings,
}

impl DevelopSettings {
    /// Per-stage parameter hashes, in pipeline order.
    pub fn stage_hashes(&self) -> [(StageId, ParamHash); StageId::COUNT] {
        [
            (StageId::Decode, self.decode.param_hash()),
            (StageId::Linearize, self.linearize.param_hash()),
            (StageId::Denoise, self.denoise.param_hash()),
            (StageId::Demosaic, self.demosaic.param_hash()),
            (StageId::Lens, self.lens.param_hash()),
            (StageId::CameraProfile, self.camera_profile.param_hash()),
            (StageId::WhiteBalance, self.white_balance.param_hash()),
            (StageId::Detail, self.detail.param_hash()),
            (StageId::Tone, self.tone.param_hash()),
            (StageId::Color, self.color.param_hash()),
            (StageId::Locals, self.locals.param_hash()),
            (StageId::Effects, self.effects.param_hash()),
            (StageId::Geometry, self.geometry.param_hash()),
            (StageId::Output, self.output.param_hash()),
        ]
    }

    /// Cumulative hashes for memoization: entry `i` covers `seed` (the
    /// process version) and stages `0..=i`. Use entry `stage.index()` as
    /// [`crate::stage::MemoKey::params_hash`].
    pub fn stage_chain(&self, seed: ParamHash) -> [(StageId, ParamHash); StageId::COUNT] {
        let mut acc = seed;
        self.stage_hashes().map(|(stage, h)| {
            acc = ParamHash::chain(acc, h);
            (stage, acc)
        })
    }

    /// First stage whose parameters differ from `other`, i.e. the earliest
    /// stage whose cached output is invalidated by moving between the two.
    pub fn first_dirty_stage(&self, other: &DevelopSettings) -> Option<StageId> {
        self.stage_hashes()
            .iter()
            .zip(other.stage_hashes())
            .find(|(a, b)| a.1 != b.1)
            .map(|(a, _)| a.0)
    }
}

macro_rules! stage_params {
    ($($ty:ty => $stage:ident),* $(,)?) => {
        $(impl StageParams for $ty { const STAGE: StageId = StageId::$stage; })*
    };
}

stage_params! {
    DecodeSettings => Decode,
    LinearizeSettings => Linearize,
    DenoiseSettings => Denoise,
    DemosaicSettings => Demosaic,
    LensSettings => Lens,
    CameraProfileSettings => CameraProfile,
    WhiteBalanceSettings => WhiteBalance,
    DetailSettings => Detail,
    ToneSettings => Tone,
    ColorSettings => Color,
    LocalsSettings => Locals,
    EffectsSettings => Effects,
    GeometrySettings => Geometry,
    OutputSettings => Output,
}

// ───────────────────────────── decode / linearize ─────────────────────────────

/// Raw container decode options.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DecodeSettings {
    /// Frame to decode from multi-frame containers.
    pub frame_index: u32,
    /// Merge pixel-shift / high-resolution multi-shot frames when present.
    pub pixel_shift_merge: bool,
}

/// How clipped highlights are reconstructed (spec 07 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HighlightReconstruction {
    /// Clip to the sensor white level.
    Clip,
    /// Propagate colour from unclipped channels.
    #[default]
    ReconstructColor,
    /// Reconstruct in LCh.
    ReconstructLch,
    /// Gradient-guided inpainting for fully clipped regions.
    Inpaint,
}

/// Linearisation options.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LinearizeSettings {
    /// Highlight reconstruction method.
    pub highlight_reconstruction: HighlightReconstruction,
}

// ───────────────────────────── denoise / demosaic ─────────────────────────────

/// Raw-domain noise reduction method.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DenoiseMethod {
    /// No raw-domain denoise (detail-stage NR still applies).
    #[default]
    Off,
    /// Neural denoise on the CFA, optionally joint with demosaic.
    Neural {
        /// Model and version used; stored for reproducibility.
        model: ModelRef,
        /// Also performs demosaicing (the demosaic stage becomes a no-op).
        joint_demosaic: bool,
    },
}

/// Raw-domain denoise ("AI Denoise").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DenoiseSettings {
    /// Method.
    pub method: DenoiseMethod,
    /// Blend amount, `0..=100`.
    pub amount: f32,
    /// Restrict to chroma noise.
    pub chroma_only: bool,
}

impl Default for DenoiseSettings {
    fn default() -> Self {
        Self {
            method: DenoiseMethod::Off,
            amount: 50.0,
            chroma_only: false,
        }
    }
}

/// Demosaic algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DemosaicMethod {
    /// Engine choice per CFA family (learned where available).
    #[default]
    Auto,
    /// Learned CNN demosaic.
    Learned,
    /// Ratio-corrected demosaicing.
    Rcd,
    /// AMaZE.
    Amaze,
    /// LMMSE.
    Lmmse,
    /// DCB.
    Dcb,
    /// Markesteijn (X-Trans).
    Markesteijn,
    /// Bilinear (reference / tests).
    Bilinear,
}

/// Demosaic options.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DemosaicSettings {
    /// Algorithm.
    pub method: DemosaicMethod,
    /// Model used when `method` is learned (or `Auto` resolved to learned).
    pub model: Option<ModelRef>,
}

// ─────────────────────────────────── lens ───────────────────────────────────

/// Source of lens correction data (spec 07 §3, in priority order).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LensProfileSource {
    /// No profile correction.
    None,
    /// Best available: embedded opcodes, then database, then auto-calibration.
    #[default]
    Auto,
    /// Manufacturer opcodes embedded in the raw.
    Embedded,
    /// A lens database profile.
    Database {
        /// Profile id.
        profile: LensProfileId,
    },
    /// Estimated from image content.
    AutoCalibrated,
}

/// Defringe (axial CA) controls for one hue band.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DefringeBand {
    /// Amount, `0..=20`.
    pub amount: f32,
    /// Hue range in degrees, `[low, high]`.
    pub hue_range: [f32; 2],
}

impl Default for DefringeBand {
    fn default() -> Self {
        Self {
            amount: 0.0,
            hue_range: [0.0, 0.0],
        }
    }
}

/// Lens corrections applied before colour.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LensSettings {
    /// Profile source.
    pub profile: LensProfileSource,
    /// Profile distortion scale, `0..=200` (%). Applied in the geometry map.
    pub distortion_scale: f32,
    /// Profile vignetting scale, `0..=200` (%).
    pub vignetting_scale: f32,
    /// Profile lateral CA scale, `0..=200` (%).
    pub chromatic_aberration_scale: f32,
    /// Remove lateral chromatic aberration (auto if no profile).
    pub remove_chromatic_aberration: bool,
    /// Manual distortion, `-100..=100`.
    pub manual_distortion: f32,
    /// Manual vignetting amount, `-100..=100`.
    pub manual_vignetting: f32,
    /// Manual vignetting midpoint, `0..=100`.
    pub manual_vignetting_midpoint: f32,
    /// Purple fringe removal.
    pub defringe_purple: DefringeBand,
    /// Green fringe removal.
    pub defringe_green: DefringeBand,
    /// Lens softness deconvolution amount, `0..=100`.
    pub softness_correction: f32,
}

impl Default for LensSettings {
    fn default() -> Self {
        Self {
            profile: LensProfileSource::Auto,
            distortion_scale: 100.0,
            vignetting_scale: 100.0,
            chromatic_aberration_scale: 100.0,
            remove_chromatic_aberration: true,
            manual_distortion: 0.0,
            manual_vignetting: 0.0,
            manual_vignetting_midpoint: 50.0,
            defringe_purple: DefringeBand {
                amount: 0.0,
                hue_range: [270.0, 330.0],
            },
            defringe_green: DefringeBand {
                amount: 0.0,
                hue_range: [60.0, 110.0],
            },
            softness_correction: 0.0,
        }
    }
}

// ───────────────────────────── profile / white balance ─────────────────────────────

/// Camera calibration hue/saturation shifts, `-100..=100`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Calibration {
    /// Shadow tint.
    pub shadow_tint: f32,
    /// Red primary hue.
    pub red_hue: f32,
    /// Red primary saturation.
    pub red_saturation: f32,
    /// Green primary hue.
    pub green_hue: f32,
    /// Green primary saturation.
    pub green_saturation: f32,
    /// Blue primary hue.
    pub blue_hue: f32,
    /// Blue primary saturation.
    pub blue_saturation: f32,
}

/// A creative look layered on the camera profile.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LookSettings {
    /// Look id.
    pub style: StyleId,
    /// Amount, `0..=200` (%).
    pub amount: f32,
}

/// Camera profile into the working space.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CameraProfileSettings {
    /// Profile id; empty means the engine default for the camera.
    pub profile: ProfileId,
    /// Profile amount for creative profiles, `0..=200` (%).
    pub amount: f32,
    /// Working space the profile targets.
    pub working_space: WorkingSpace,
    /// Optional look.
    pub look: Option<LookSettings>,
    /// Calibration adjustments.
    pub calibration: Calibration,
}

impl Default for CameraProfileSettings {
    fn default() -> Self {
        Self {
            profile: ProfileId::default(),
            amount: 100.0,
            working_space: WorkingSpace::LinearRec2020,
            look: None,
            calibration: Calibration::default(),
        }
    }
}

/// White balance source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WhiteBalanceMode {
    /// Camera as-shot multipliers.
    #[default]
    AsShot,
    /// Learned illuminant estimate.
    Auto,
    /// Daylight preset.
    Daylight,
    /// Cloudy preset.
    Cloudy,
    /// Shade preset.
    Shade,
    /// Tungsten preset.
    Tungsten,
    /// Fluorescent preset.
    Fluorescent,
    /// Flash preset.
    Flash,
    /// Explicit temperature/tint.
    Custom,
}

/// White balance. `temperature`/`tint` are authoritative only in `Custom`
/// mode; in other modes they record the resolved values for display.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WhiteBalanceSettings {
    /// Mode.
    pub mode: WhiteBalanceMode,
    /// Correlated colour temperature in kelvin.
    pub temperature: f32,
    /// Tint (Duv-like, green −/magenta +), `-150..=150`.
    pub tint: f32,
}

impl Default for WhiteBalanceSettings {
    fn default() -> Self {
        Self {
            mode: WhiteBalanceMode::AsShot,
            temperature: 5500.0,
            tint: 0.0,
        }
    }
}

// ───────────────────────────────── detail ─────────────────────────────────

/// Output-style sharpening (unsharp/deconvolution hybrid).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sharpening {
    /// Amount, `0..=150`.
    pub amount: f32,
    /// Radius in pixels, `0.5..=3`.
    pub radius: f32,
    /// Detail, `0..=100`.
    pub detail: f32,
    /// Edge masking, `0..=100`.
    pub masking: f32,
}

impl Default for Sharpening {
    fn default() -> Self {
        Self {
            amount: 40.0,
            radius: 1.0,
            detail: 25.0,
            masking: 0.0,
        }
    }
}

/// Detail-stage (post-demosaic) noise reduction, `0..=100`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NoiseReduction {
    /// Luminance NR.
    pub luminance: f32,
    /// Luminance detail.
    pub luminance_detail: f32,
    /// Luminance contrast.
    pub luminance_contrast: f32,
    /// Colour NR.
    pub color: f32,
    /// Colour detail.
    pub color_detail: f32,
    /// Colour smoothness.
    pub color_smoothness: f32,
}

impl Default for NoiseReduction {
    fn default() -> Self {
        Self {
            luminance: 0.0,
            luminance_detail: 50.0,
            luminance_contrast: 0.0,
            color: 25.0,
            color_detail: 50.0,
            color_smoothness: 50.0,
        }
    }
}

/// Detail stage.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DetailSettings {
    /// Sharpening.
    pub sharpening: Sharpening,
    /// Noise reduction.
    pub noise_reduction: NoiseReduction,
}

// ────────────────────────────────── tone ──────────────────────────────────

/// A curve control point, both axes normalised to `0..=1`.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct CurvePoint {
    /// Input.
    pub x: f32,
    /// Output.
    pub y: f32,
}

/// A monotone spline curve. Empty means identity.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Curve(pub Vec<CurvePoint>);

impl Curve {
    /// True if the curve is the identity.
    pub fn is_identity(&self) -> bool {
        self.0.iter().all(|p| p.x == p.y)
    }
}

/// Parametric (region) curve. Region amounts `-100..=100`; splits `0..=100`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ParametricCurve {
    /// Shadows region.
    pub shadows: f32,
    /// Darks region.
    pub darks: f32,
    /// Lights region.
    pub lights: f32,
    /// Highlights region.
    pub highlights: f32,
    /// Shadows/darks split.
    pub shadow_split: f32,
    /// Darks/lights split.
    pub midtone_split: f32,
    /// Lights/highlights split.
    pub highlight_split: f32,
}

impl Default for ParametricCurve {
    fn default() -> Self {
        Self {
            shadows: 0.0,
            darks: 0.0,
            lights: 0.0,
            highlights: 0.0,
            shadow_split: 25.0,
            midtone_split: 50.0,
            highlight_split: 75.0,
        }
    }
}

/// Point curves.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ToneCurves {
    /// Parametric curve (applied first).
    pub parametric: ParametricCurve,
    /// Master RGB curve.
    pub rgb: Curve,
    /// Red channel curve.
    pub red: Curve,
    /// Green channel curve.
    pub green: Curve,
    /// Blue channel curve.
    pub blue: Curve,
    /// Luminance-only curve (hue/saturation preserving).
    pub luminance: Curve,
}

/// Scene-to-display tone mapper (spec 07 §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayTransform {
    /// Engine default filmic-style sigmoid with hue preservation.
    #[default]
    Native,
    /// AgX.
    Agx,
    /// Plain sigmoid.
    Sigmoid,
    /// Filmic.
    Filmic,
    /// Adobe Process Version 6 compatible mapper, for imported edits.
    AdobePv6Compat,
}

/// Global tone.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ToneSettings {
    /// Exposure in EV, `-10..=10`.
    pub exposure: f32,
    /// Contrast, `-100..=100`.
    pub contrast: f32,
    /// Highlights, `-100..=100`.
    pub highlights: f32,
    /// Shadows, `-100..=100`.
    pub shadows: f32,
    /// Whites, `-100..=100`.
    pub whites: f32,
    /// Blacks, `-100..=100`.
    pub blacks: f32,
    /// Texture (fine local contrast), `-100..=100`.
    pub texture: f32,
    /// Clarity (mid-scale local contrast), `-100..=100`.
    pub clarity: f32,
    /// Dehaze, `-100..=100`.
    pub dehaze: f32,
    /// Curves.
    pub curves: ToneCurves,
    /// Display transform.
    pub display_transform: DisplayTransform,
}

// ────────────────────────────────── colour ──────────────────────────────────

/// Per-band values for the eight HSL hue bands, `-100..=100`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HueBands {
    /// Red.
    pub red: f32,
    /// Orange.
    pub orange: f32,
    /// Yellow.
    pub yellow: f32,
    /// Green.
    pub green: f32,
    /// Aqua.
    pub aqua: f32,
    /// Blue.
    pub blue: f32,
    /// Purple.
    pub purple: f32,
    /// Magenta.
    pub magenta: f32,
}

/// HSL / colour mixer, evaluated in OkLCh.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HslSettings {
    /// Hue shifts.
    pub hue: HueBands,
    /// Saturation.
    pub saturation: HueBands,
    /// Luminance.
    pub luminance: HueBands,
}

/// One colour-grading wheel.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GradeWheel {
    /// Hue in degrees, `0..360`.
    pub hue: f32,
    /// Saturation, `0..=100`.
    pub saturation: f32,
    /// Luminance, `-100..=100`.
    pub luminance: f32,
}

/// Three-way colour grading.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorGrading {
    /// Shadows wheel.
    pub shadows: GradeWheel,
    /// Midtones wheel.
    pub midtones: GradeWheel,
    /// Highlights wheel.
    pub highlights: GradeWheel,
    /// Global wheel.
    pub global: GradeWheel,
    /// Blending, `0..=100`.
    pub blending: f32,
    /// Balance, `-100..=100`.
    pub balance: f32,
}

impl Default for ColorGrading {
    fn default() -> Self {
        Self {
            shadows: GradeWheel::default(),
            midtones: GradeWheel::default(),
            highlights: GradeWheel::default(),
            global: GradeWheel::default(),
            blending: 50.0,
            balance: 0.0,
        }
    }
}

/// A Point Color sample and its shifts.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PointColor {
    /// Sampled colour in OkLCh `[L, C, h°]`.
    pub source_lch: [f32; 3],
    /// Hue shift, degrees.
    pub hue_shift: f32,
    /// Saturation shift, `-100..=100`.
    pub saturation_shift: f32,
    /// Luminance shift, `-100..=100`.
    pub luminance_shift: f32,
    /// Range width, `0..=100`.
    pub range: f32,
}

/// A 3D LUT applied with a strength.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LutSettings {
    /// LUT style id.
    pub style: StyleId,
    /// Strength, `0..=100`.
    pub amount: f32,
}

/// Global colour.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorSettings {
    /// Vibrance, `-100..=100`.
    pub vibrance: f32,
    /// Saturation, `-100..=100`.
    pub saturation: f32,
    /// HSL mixer.
    pub hsl: HslSettings,
    /// Colour grading.
    pub grading: ColorGrading,
    /// Point Color entries.
    pub point_colors: Vec<PointColor>,
    /// Optional LUT.
    pub lut: Option<LutSettings>,
}

// ───────────────────────────────── locals ─────────────────────────────────

/// Masked local adjustments and retouching, applied in list order.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LocalsSettings {
    /// Retouch operations (heal, clone, remove, skin), applied before adjustments.
    pub retouch: Vec<RetouchOperation>,
    /// Local adjustments.
    pub adjustments: Vec<LocalAdjustment>,
}

// ───────────────────────────────── effects ─────────────────────────────────

/// Post-crop vignette style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VignetteStyle {
    /// Preserve highlights.
    #[default]
    HighlightPriority,
    /// Preserve colour.
    ColorPriority,
    /// Paint overlay.
    PaintOverlay,
}

/// Post-crop vignette.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Vignette {
    /// Amount, `-100..=100`.
    pub amount: f32,
    /// Midpoint, `0..=100`.
    pub midpoint: f32,
    /// Roundness, `-100..=100`.
    pub roundness: f32,
    /// Feather, `0..=100`.
    pub feather: f32,
    /// Highlight protection, `0..=100`.
    pub highlights: f32,
    /// Style.
    pub style: VignetteStyle,
}

impl Default for Vignette {
    fn default() -> Self {
        Self {
            amount: 0.0,
            midpoint: 50.0,
            roundness: 0.0,
            feather: 50.0,
            highlights: 0.0,
            style: VignetteStyle::default(),
        }
    }
}

/// Film grain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Grain {
    /// Amount, `0..=100`.
    pub amount: f32,
    /// Size, `0..=100`.
    pub size: f32,
    /// Roughness, `0..=100`.
    pub roughness: f32,
}

impl Default for Grain {
    fn default() -> Self {
        Self {
            amount: 0.0,
            size: 25.0,
            roughness: 50.0,
        }
    }
}

/// Depth-based lens blur.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LensBlur {
    /// Blur amount, `0..=100`.
    pub amount: f32,
    /// In-focus depth range, normalised `[near, far]`.
    pub focus_range: [f32; 2],
    /// Bokeh shape id.
    pub bokeh: String,
    /// Depth model used.
    pub depth_model: Option<ModelRef>,
}

impl Default for LensBlur {
    fn default() -> Self {
        Self {
            amount: 50.0,
            focus_range: [0.0, 0.1],
            bokeh: "circle".into(),
            depth_model: None,
        }
    }
}

/// Creative effects.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct EffectsSettings {
    /// Post-crop vignette.
    pub vignette: Vignette,
    /// Grain.
    pub grain: Grain,
    /// Lens blur, when enabled.
    pub lens_blur: Option<LensBlur>,
}

// ──────────────────────────────── geometry ────────────────────────────────

/// Rectangle in normalised image coordinates (`0..=1`, origin top-left).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NormalizedRect {
    /// Left edge.
    pub left: f32,
    /// Top edge.
    pub top: f32,
    /// Right edge.
    pub right: f32,
    /// Bottom edge.
    pub bottom: f32,
}

impl Default for NormalizedRect {
    fn default() -> Self {
        Self::FULL
    }
}

impl NormalizedRect {
    /// The whole image.
    pub const FULL: Self = Self {
        left: 0.0,
        top: 0.0,
        right: 1.0,
        bottom: 1.0,
    };

    /// True if edges are ordered and within `0..=1`.
    pub fn is_valid(&self) -> bool {
        (0.0..=1.0).contains(&self.left)
            && (0.0..=1.0).contains(&self.top)
            && (0.0..=1.0).contains(&self.right)
            && (0.0..=1.0).contains(&self.bottom)
            && self.left < self.right
            && self.top < self.bottom
    }
}

/// Crop.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Crop {
    /// Crop rectangle, before rotation.
    pub rect: NormalizedRect,
    /// Straighten angle in degrees, `-45..=45`.
    pub angle: f32,
    /// Locked aspect ratio `[w, h]`, if any.
    pub aspect: Option<[u32; 2]>,
}

/// Upright mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UprightMode {
    /// Off.
    #[default]
    Off,
    /// Balanced automatic.
    Auto,
    /// Level only.
    Level,
    /// Level and vertical.
    Vertical,
    /// Full perspective.
    Full,
    /// From user guides.
    Guided,
}

/// A guide line in normalised coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct GuideLine {
    /// Start `[x, y]`.
    pub start: [f32; 2],
    /// End `[x, y]`.
    pub end: [f32; 2],
}

/// Manual transform, `-100..=100` unless noted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Transform {
    /// Vertical perspective.
    pub vertical: f32,
    /// Horizontal perspective.
    pub horizontal: f32,
    /// Rotation in degrees, `-10..=10`.
    pub rotate: f32,
    /// Aspect.
    pub aspect: f32,
    /// Scale, `50..=150` (%).
    pub scale: f32,
    /// Horizontal offset.
    pub offset_x: f32,
    /// Vertical offset.
    pub offset_y: f32,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            vertical: 0.0,
            horizontal: 0.0,
            rotate: 0.0,
            aspect: 0.0,
            scale: 100.0,
            offset_x: 0.0,
            offset_y: 0.0,
        }
    }
}

/// Upright + guides.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Upright {
    /// Mode.
    pub mode: UprightMode,
    /// Guides for [`UprightMode::Guided`].
    pub guides: Vec<GuideLine>,
}

/// Composed geometry: orientation, distortion (from lens), Upright, transform
/// and crop become one inverse map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GeometrySettings {
    /// EXIF orientation `1..=8` applied on top of the file's own.
    pub orientation: u8,
    /// Upright.
    pub upright: Upright,
    /// Manual transform.
    pub transform: Transform,
    /// Constrain crop to the warped image.
    pub constrain_crop: bool,
    /// Crop.
    pub crop: Crop,
}

impl Default for GeometrySettings {
    fn default() -> Self {
        Self {
            orientation: 1,
            upright: Upright::default(),
            transform: Transform::default(),
            constrain_crop: false,
            crop: Crop::default(),
        }
    }
}

// ────────────────────────────────── output ──────────────────────────────────

/// Gamut mapping from working to output space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GamutMapping {
    /// Hue-preserving chroma compression.
    #[default]
    Perceptual,
    /// Hard clip.
    Clip,
}

/// Render-affecting output settings (per-export options live in the export request).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct OutputSettings {
    /// Gamut mapping.
    pub gamut_mapping: GamutMapping,
    /// Edit in HDR (unbounded highlights to an HDR display).
    pub hdr: bool,
    /// Maximum HDR headroom in stops above SDR white.
    pub hdr_headroom_stops: f32,
    /// Soft-proof profile, if proofing.
    pub proof_profile: Option<IccProfileHandle>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_changes_downstream_only() {
        let a = DevelopSettings::default();
        let mut b = a.clone();
        b.tone.exposure = 0.5;
        let seed = ParamHash::default();
        let (ca, cb) = (a.stage_chain(seed), b.stage_chain(seed));
        for i in 0..StageId::COUNT {
            if i < StageId::Tone.index() {
                assert_eq!(ca[i], cb[i]);
            } else {
                assert_ne!(ca[i].1, cb[i].1);
            }
        }
        assert_eq!(a.first_dirty_stage(&b), Some(StageId::Tone));
        assert_eq!(a.first_dirty_stage(&a.clone()), None);
    }

    #[test]
    fn empty_object_is_default() {
        let s: DevelopSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s, DevelopSettings::default());
        let partial: DevelopSettings =
            serde_json::from_str(r#"{"tone":{"exposure":1.25}}"#).unwrap();
        assert_eq!(partial.tone.exposure, 1.25);
        assert_eq!(partial.lens, LensSettings::default());
    }
}
