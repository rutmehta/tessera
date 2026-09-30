//! Data-only reader for Lightroom Classic 15.5 develop settings.
//!
//! LrC 15.5 stores `Adobe_imageDevelopSettings.text` as a Lua table literal
//! (`s = { Exposure2012 = 0.35, ... }`) rather than XMP. This module never
//! evaluates Lua: it accepts only the `s =` assignment prefix followed by one
//! literal made of nested tables, quoted strings (with Lua escapes), decimal
//! numbers, booleans and `nil`. Identifiers are accepted only as table keys.
//! Everything else (calls, expressions, comments, long brackets, further
//! statements) is a decode error.
//!
//! The parser is iterative (an explicit stack, no recursion) and bounded by
//! [`MAX_INPUT_BYTES`], [`MAX_DEPTH`] and [`MAX_VALUES`], so hostile rows cannot
//! overflow the stack or allocate without limit.
//!
//! Mapped keys ([`KEY_MAP`]) are rendered into the Adobe XMP shape of the same
//! setting and decoded by [`crate::xmp::parse`], so a Lua row and the equivalent
//! XMP row produce the same recipe through one translation path. Keys missing
//! from the table, or values with no XMP shape, become warnings (plan report
//! entries) and the whole source literal is kept in
//! `recipe.unknown["lrcat_develop_lua"]`.
use std::collections::HashSet;

use engine_api::{EngineError, EngineResult, recipe::CrsKey, recipe::Recipe};
use serde_json::json;

/// Maximum table nesting (real LrC 15.5 rows nest at most 6 deep).
pub const MAX_DEPTH: usize = 32;
/// Maximum literal size in bytes (real LrC 15.5 rows are well under 1 MiB).
pub const MAX_INPUT_BYTES: usize = 4 << 20;
/// Maximum number of values (scalars and tables) in one literal.
pub const MAX_VALUES: usize = 500_000;

const FORMAT: &str = "lightroom-develop-lua";

/// Lua develop key and the `crs:` local name (without prefix) it maps to.
///
/// One-to-one. LrC uses Adobe's crs names verbatim as Lua keys; the table is
/// still explicit so an unmapped key is reported instead of guessed. Keys in
/// [`CrsKey`] that live in another namespace (`aux:`) keep that namespace.
pub const KEY_MAP: &[(&str, &str)] = &[
    // Every key the recipe maps (engine_api::recipe::CrsKey).
    ("ProcessVersion", "ProcessVersion"),
    ("WhiteBalance", "WhiteBalance"),
    ("Temperature", "Temperature"),
    ("Tint", "Tint"),
    ("Exposure2012", "Exposure2012"),
    ("Contrast2012", "Contrast2012"),
    ("Highlights2012", "Highlights2012"),
    ("Shadows2012", "Shadows2012"),
    ("Whites2012", "Whites2012"),
    ("Blacks2012", "Blacks2012"),
    ("Texture", "Texture"),
    ("Clarity2012", "Clarity2012"),
    ("Dehaze", "Dehaze"),
    ("Vibrance", "Vibrance"),
    ("Saturation", "Saturation"),
    ("ToneCurvePV2012", "ToneCurvePV2012"),
    ("ToneCurvePV2012Red", "ToneCurvePV2012Red"),
    ("ToneCurvePV2012Green", "ToneCurvePV2012Green"),
    ("ToneCurvePV2012Blue", "ToneCurvePV2012Blue"),
    ("ParametricShadows", "ParametricShadows"),
    ("ParametricDarks", "ParametricDarks"),
    ("ParametricLights", "ParametricLights"),
    ("ParametricHighlights", "ParametricHighlights"),
    ("ParametricShadowSplit", "ParametricShadowSplit"),
    ("ParametricMidtoneSplit", "ParametricMidtoneSplit"),
    ("ParametricHighlightSplit", "ParametricHighlightSplit"),
    ("HueAdjustmentRed", "HueAdjustmentRed"),
    ("HueAdjustmentOrange", "HueAdjustmentOrange"),
    ("HueAdjustmentYellow", "HueAdjustmentYellow"),
    ("HueAdjustmentGreen", "HueAdjustmentGreen"),
    ("HueAdjustmentAqua", "HueAdjustmentAqua"),
    ("HueAdjustmentBlue", "HueAdjustmentBlue"),
    ("HueAdjustmentPurple", "HueAdjustmentPurple"),
    ("HueAdjustmentMagenta", "HueAdjustmentMagenta"),
    ("SaturationAdjustmentRed", "SaturationAdjustmentRed"),
    ("SaturationAdjustmentOrange", "SaturationAdjustmentOrange"),
    ("SaturationAdjustmentYellow", "SaturationAdjustmentYellow"),
    ("SaturationAdjustmentGreen", "SaturationAdjustmentGreen"),
    ("SaturationAdjustmentAqua", "SaturationAdjustmentAqua"),
    ("SaturationAdjustmentBlue", "SaturationAdjustmentBlue"),
    ("SaturationAdjustmentPurple", "SaturationAdjustmentPurple"),
    ("SaturationAdjustmentMagenta", "SaturationAdjustmentMagenta"),
    ("LuminanceAdjustmentRed", "LuminanceAdjustmentRed"),
    ("LuminanceAdjustmentOrange", "LuminanceAdjustmentOrange"),
    ("LuminanceAdjustmentYellow", "LuminanceAdjustmentYellow"),
    ("LuminanceAdjustmentGreen", "LuminanceAdjustmentGreen"),
    ("LuminanceAdjustmentAqua", "LuminanceAdjustmentAqua"),
    ("LuminanceAdjustmentBlue", "LuminanceAdjustmentBlue"),
    ("LuminanceAdjustmentPurple", "LuminanceAdjustmentPurple"),
    ("LuminanceAdjustmentMagenta", "LuminanceAdjustmentMagenta"),
    ("SplitToningShadowHue", "SplitToningShadowHue"),
    ("SplitToningShadowSaturation", "SplitToningShadowSaturation"),
    ("SplitToningHighlightHue", "SplitToningHighlightHue"),
    (
        "SplitToningHighlightSaturation",
        "SplitToningHighlightSaturation",
    ),
    ("SplitToningBalance", "SplitToningBalance"),
    ("ColorGradeShadowLum", "ColorGradeShadowLum"),
    ("ColorGradeMidtoneHue", "ColorGradeMidtoneHue"),
    ("ColorGradeMidtoneSat", "ColorGradeMidtoneSat"),
    ("ColorGradeMidtoneLum", "ColorGradeMidtoneLum"),
    ("ColorGradeHighlightLum", "ColorGradeHighlightLum"),
    ("ColorGradeGlobalHue", "ColorGradeGlobalHue"),
    ("ColorGradeGlobalSat", "ColorGradeGlobalSat"),
    ("ColorGradeGlobalLum", "ColorGradeGlobalLum"),
    ("ColorGradeBlending", "ColorGradeBlending"),
    ("PointColors", "PointColors"),
    ("Sharpness", "Sharpness"),
    ("SharpenRadius", "SharpenRadius"),
    ("SharpenDetail", "SharpenDetail"),
    ("SharpenEdgeMasking", "SharpenEdgeMasking"),
    ("LuminanceSmoothing", "LuminanceSmoothing"),
    (
        "LuminanceNoiseReductionDetail",
        "LuminanceNoiseReductionDetail",
    ),
    (
        "LuminanceNoiseReductionContrast",
        "LuminanceNoiseReductionContrast",
    ),
    ("ColorNoiseReduction", "ColorNoiseReduction"),
    ("ColorNoiseReductionDetail", "ColorNoiseReductionDetail"),
    (
        "ColorNoiseReductionSmoothness",
        "ColorNoiseReductionSmoothness",
    ),
    (
        "EnhanceDenoiseAlreadyApplied",
        "EnhanceDenoiseAlreadyApplied",
    ),
    ("EnhanceDenoiseVersion", "EnhanceDenoiseVersion"),
    ("EnhanceDenoiseLumaAmount", "EnhanceDenoiseLumaAmount"),
    (
        "EnhanceDetailsAlreadyApplied",
        "EnhanceDetailsAlreadyApplied",
    ),
    ("EnhanceDetailsVersion", "EnhanceDetailsVersion"),
    (
        "EnhanceSuperResolutionAlreadyApplied",
        "EnhanceSuperResolutionAlreadyApplied",
    ),
    (
        "EnhanceSuperResolutionVersion",
        "EnhanceSuperResolutionVersion",
    ),
    ("EnhanceSuperResolutionScale", "EnhanceSuperResolutionScale"),
    ("LensProfileEnable", "LensProfileEnable"),
    ("LensProfileSetup", "LensProfileSetup"),
    ("LensProfileName", "LensProfileName"),
    ("LensProfileFilename", "LensProfileFilename"),
    ("LensProfileDigest", "LensProfileDigest"),
    ("LensProfileDistortionScale", "LensProfileDistortionScale"),
    (
        "LensProfileChromaticAberrationScale",
        "LensProfileChromaticAberrationScale",
    ),
    ("LensProfileVignettingScale", "LensProfileVignettingScale"),
    ("LensManualDistortionAmount", "LensManualDistortionAmount"),
    ("VignetteAmount", "VignetteAmount"),
    ("VignetteMidpoint", "VignetteMidpoint"),
    ("AutoLateralCA", "AutoLateralCA"),
    ("ChromaticAberrationR", "ChromaticAberrationR"),
    ("ChromaticAberrationB", "ChromaticAberrationB"),
    ("DefringePurpleAmount", "DefringePurpleAmount"),
    ("DefringePurpleHueLo", "DefringePurpleHueLo"),
    ("DefringePurpleHueHi", "DefringePurpleHueHi"),
    ("DefringeGreenAmount", "DefringeGreenAmount"),
    ("DefringeGreenHueLo", "DefringeGreenHueLo"),
    ("DefringeGreenHueHi", "DefringeGreenHueHi"),
    ("PerspectiveUpright", "PerspectiveUpright"),
    ("PerspectiveVertical", "PerspectiveVertical"),
    ("PerspectiveHorizontal", "PerspectiveHorizontal"),
    ("PerspectiveRotate", "PerspectiveRotate"),
    ("PerspectiveAspect", "PerspectiveAspect"),
    ("PerspectiveScale", "PerspectiveScale"),
    ("PerspectiveX", "PerspectiveX"),
    ("PerspectiveY", "PerspectiveY"),
    ("HasCrop", "HasCrop"),
    ("CropTop", "CropTop"),
    ("CropLeft", "CropLeft"),
    ("CropBottom", "CropBottom"),
    ("CropRight", "CropRight"),
    ("CropAngle", "CropAngle"),
    ("CropConstrainToWarp", "CropConstrainToWarp"),
    ("PostCropVignetteAmount", "PostCropVignetteAmount"),
    ("PostCropVignetteMidpoint", "PostCropVignetteMidpoint"),
    ("PostCropVignetteFeather", "PostCropVignetteFeather"),
    ("PostCropVignetteRoundness", "PostCropVignetteRoundness"),
    ("PostCropVignetteStyle", "PostCropVignetteStyle"),
    (
        "PostCropVignetteHighlightContrast",
        "PostCropVignetteHighlightContrast",
    ),
    ("GrainAmount", "GrainAmount"),
    ("GrainSize", "GrainSize"),
    ("GrainFrequency", "GrainFrequency"),
    ("LensBlur", "LensBlur"),
    ("CameraProfile", "CameraProfile"),
    ("CameraProfileDigest", "CameraProfileDigest"),
    ("Look", "Look"),
    ("ShadowTint", "ShadowTint"),
    ("RedHue", "RedHue"),
    ("RedSaturation", "RedSaturation"),
    ("GreenHue", "GreenHue"),
    ("GreenSaturation", "GreenSaturation"),
    ("BlueHue", "BlueHue"),
    ("BlueSaturation", "BlueSaturation"),
    ("MaskGroupBasedCorrections", "MaskGroupBasedCorrections"),
    ("RetouchAreas", "RetouchAreas"),
    ("RetouchInfo", "RetouchInfo"),
    ("HDREditMode", "HDREditMode"),
    ("HDRMaxValue", "HDRMaxValue"),
    // Documented crs properties the recipe does not map. They pass through the
    // XMP path exactly as they would in an XMP row: reported as unsupported
    // and retained per property.
    ("AutoTone", "AutoTone"),
    ("AutoToneDigest", "AutoToneDigest"),
    ("AutoToneDigestNoSat", "AutoToneDigestNoSat"),
    ("AutoWhiteVersion", "AutoWhiteVersion"),
    ("Brightness", "Brightness"),
    ("Clarity", "Clarity"),
    ("CompatibleVersion", "CompatibleVersion"),
    ("Contrast", "Contrast"),
    ("ConvertToGrayscale", "ConvertToGrayscale"),
    ("CurveRefineSaturation", "CurveRefineSaturation"),
    ("DepthBasedCorrections", "DepthBasedCorrections"),
    ("DepthMapInfo", "DepthMapInfo"),
    ("Exposure", "Exposure"),
    ("FillLight", "FillLight"),
    ("GrainSeed", "GrainSeed"),
    ("GrayMixerRed", "GrayMixerRed"),
    ("GrayMixerOrange", "GrayMixerOrange"),
    ("GrayMixerYellow", "GrayMixerYellow"),
    ("GrayMixerGreen", "GrayMixerGreen"),
    ("GrayMixerAqua", "GrayMixerAqua"),
    ("GrayMixerBlue", "GrayMixerBlue"),
    ("GrayMixerPurple", "GrayMixerPurple"),
    ("GrayMixerMagenta", "GrayMixerMagenta"),
    ("HighlightRecovery", "HighlightRecovery"),
    ("IncrementalTemperature", "IncrementalTemperature"),
    ("IncrementalTint", "IncrementalTint"),
    ("LensProfileIsEmbedded", "LensProfileIsEmbedded"),
    ("OverrideLookVignette", "OverrideLookVignette"),
    ("RangeMaskMapInfo", "RangeMaskMapInfo"),
    ("RedEyeInfo", "RedEyeInfo"),
    ("SDRBlend", "SDRBlend"),
    ("SDRBrightness", "SDRBrightness"),
    ("SDRClarity", "SDRClarity"),
    ("SDRContrast", "SDRContrast"),
    ("SDRHighlights", "SDRHighlights"),
    ("SDRShadows", "SDRShadows"),
    ("SDRWhites", "SDRWhites"),
    ("Shadows", "Shadows"),
    ("ToggleStyleAmount", "ToggleStyleAmount"),
    ("ToggleStyleDigest", "ToggleStyleDigest"),
    ("ToneCurveName2012", "ToneCurveName2012"),
    ("UprightCenterMode", "UprightCenterMode"),
    ("UprightCenterNormX", "UprightCenterNormX"),
    ("UprightCenterNormY", "UprightCenterNormY"),
    ("UprightDependentDigest", "UprightDependentDigest"),
    ("UprightFocalLength35mm", "UprightFocalLength35mm"),
    ("UprightFocalMode", "UprightFocalMode"),
    ("UprightPreview", "UprightPreview"),
    ("UprightTransform_0", "UprightTransform_0"),
    ("UprightTransform_1", "UprightTransform_1"),
    ("UprightTransform_2", "UprightTransform_2"),
    ("UprightTransform_3", "UprightTransform_3"),
    ("UprightTransform_4", "UprightTransform_4"),
    ("UprightTransform_5", "UprightTransform_5"),
    ("UprightVersion", "UprightVersion"),
    ("Version", "Version"),
];

/// A table key: an identifier or `["string"]` (both are string keys in Lua),
/// or `[number]`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LuaKey {
    Str(String),
    Num(String),
}

/// A literal value. Numbers keep their validated decimal source text.
#[derive(Debug, Clone, PartialEq)]
pub enum LuaValue {
    Nil,
    Bool(bool),
    Number(String),
    String(String),
    Table(LuaTable),
}

/// Positional entries in order, then keyed fields in source order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LuaTable {
    pub items: Vec<LuaValue>,
    pub fields: Vec<(LuaKey, LuaValue)>,
}

fn error(message: impl std::fmt::Display) -> EngineError {
    EngineError::Decode {
        format: FORMAT.into(),
        message: message.to_string(),
    }
}

/// Read one `s = <literal>` develop-settings row (an optional final `;` and
/// surrounding whitespace are allowed).
pub fn read(text: &str) -> EngineResult<LuaValue> {
    if text.len() > MAX_INPUT_BYTES {
        return Err(error(format!(
            "literal is {} bytes; limit is {MAX_INPUT_BYTES}",
            text.len()
        )));
    }
    let mut p = Parser {
        s: text.as_bytes(),
        pos: 0,
        values: 0,
    };
    p.ws();
    if p.ident().as_deref() != Some("s") {
        return Err(p.err("expected the `s =` assignment"));
    }
    p.ws();
    if !p.eat(b'=') || p.peek() == Some(b'=') {
        return Err(p.err("expected the `s =` assignment"));
    }
    let value = p.literal()?;
    p.ws();
    p.eat(b';');
    p.ws();
    if p.pos != p.s.len() {
        return Err(p.err("trailing content after the literal"));
    }
    Ok(value)
}

struct Parser<'a> {
    s: &'a [u8],
    pos: usize,
    values: usize,
}

struct Frame {
    table: LuaTable,
    /// Key this table is stored under in its parent (`None`: positional/root).
    key: Option<LuaKey>,
    seen: HashSet<LuaKey>,
}

impl Parser<'_> {
    fn err(&self, message: &str) -> EngineError {
        error(format!("{message} at byte {}", self.pos))
    }
    fn peek(&self) -> Option<u8> {
        self.s.get(self.pos).copied()
    }
    fn eat(&mut self, b: u8) -> bool {
        let hit = self.peek() == Some(b);
        self.pos += usize::from(hit);
        hit
    }
    fn ws(&mut self) {
        while self.peek().is_some_and(|b| b.is_ascii_whitespace()) {
            self.pos += 1;
        }
    }
    fn ident(&mut self) -> Option<String> {
        let start = self.pos;
        if !self
            .peek()
            .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        {
            return None;
        }
        while self
            .peek()
            .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            self.pos += 1;
        }
        Some(String::from_utf8_lossy(&self.s[start..self.pos]).into_owned())
    }
    fn count(&mut self) -> EngineResult<()> {
        self.values += 1;
        if self.values > MAX_VALUES {
            return Err(self.err(&format!("more than {MAX_VALUES} values; limit exceeded")));
        }
        Ok(())
    }

    /// Iterative literal parser; the explicit stack holds open tables.
    fn literal(&mut self) -> EngineResult<LuaValue> {
        let mut stack: Vec<Frame> = Vec::new();
        // The key for the value about to be read in the innermost table.
        let mut pending: Option<LuaKey> = None;
        loop {
            // Expect a value.
            self.ws();
            self.count()?;
            let mut done = if self.eat(b'{') {
                if stack.len() >= MAX_DEPTH {
                    return Err(self.err(&format!("table nesting deeper than {MAX_DEPTH}")));
                }
                stack.push(Frame {
                    table: LuaTable::default(),
                    key: pending.take(),
                    seen: HashSet::new(),
                });
                None
            } else {
                Some(self.scalar()?)
            };
            loop {
                if let Some(value) = done.take() {
                    // Attach a finished value to its parent (or return it).
                    let Some(frame) = stack.last_mut() else {
                        return Ok(value);
                    };
                    match pending.take() {
                        None => frame.table.items.push(value),
                        Some(key) => frame.table.fields.push((key, value)),
                    }
                    self.ws();
                    if !(self.eat(b',') || self.eat(b';')) {
                        if self.eat(b'}') {
                            done = Some(self.close(&mut stack, &mut pending));
                            continue;
                        }
                        return Err(self.err("expected `,` or `}`"));
                    }
                }
                // Inside a table: an entry or the closing brace.
                self.ws();
                if self.eat(b'}') {
                    done = Some(self.close(&mut stack, &mut pending));
                    continue;
                }
                let key = self.key()?;
                if let Some(key) = key {
                    let frame = stack.last_mut().expect("inside a table");
                    let n = frame.table.items.len();
                    let clashes_positional = matches!(&key, LuaKey::Num(k)
                        if k.parse::<usize>().is_ok_and(|i| (1..=n).contains(&i)));
                    if clashes_positional || !frame.seen.insert(key.clone()) {
                        return Err(self.err("duplicate table key"));
                    }
                    pending = Some(key);
                }
                break;
            }
        }
    }

    fn close(&mut self, stack: &mut Vec<Frame>, pending: &mut Option<LuaKey>) -> LuaValue {
        let frame = stack.pop().expect("inside a table");
        *pending = frame.key;
        LuaValue::Table(frame.table)
    }

    /// `name =`, `["s"] =`, `[n] =`, or nothing (a positional value follows).
    fn key(&mut self) -> EngineResult<Option<LuaKey>> {
        let start = self.pos;
        let key = if self.eat(b'[') {
            if self.peek() == Some(b'[') || self.peek() == Some(b'=') {
                return Err(self.err("long brackets are not supported"));
            }
            self.ws();
            let key = match self.peek() {
                Some(b'"' | b'\'') => LuaKey::Str(self.string()?),
                _ => LuaKey::Num(self.number()?),
            };
            self.ws();
            if !self.eat(b']') {
                return Err(self.err("expected `]`"));
            }
            key
        } else if let Some(name) = self.ident() {
            if matches!(name.as_str(), "true" | "false" | "nil") {
                self.pos = start;
                return Ok(None);
            }
            LuaKey::Str(name)
        } else {
            return Ok(None);
        };
        self.ws();
        if !self.eat(b'=') || self.peek() == Some(b'=') {
            return Err(self.err("identifiers are only allowed as table keys"));
        }
        Ok(Some(key))
    }

    fn scalar(&mut self) -> EngineResult<LuaValue> {
        match self.peek() {
            Some(b'"' | b'\'') => Ok(LuaValue::String(self.string()?)),
            Some(b'-' | b'.' | b'0'..=b'9') => Ok(LuaValue::Number(self.number()?)),
            Some(b'[') => Err(self.err("long brackets are not supported")),
            _ => match self.ident().as_deref() {
                Some("true") => Ok(LuaValue::Bool(true)),
                Some("false") => Ok(LuaValue::Bool(false)),
                Some("nil") => Ok(LuaValue::Nil),
                Some(_) => Err(self.err("identifiers are only allowed as table keys")),
                None => Err(self.err("unexpected token")),
            },
        }
    }

    /// Decimal number: `-?(digits[.digits]|.digits)([eE][+-]?digits)?`.
    fn number(&mut self) -> EngineResult<String> {
        let start = self.pos;
        self.eat(b'-');
        let digits = |p: &mut Self| {
            let s = p.pos;
            while p.peek().is_some_and(|b| b.is_ascii_digit()) {
                p.pos += 1;
            }
            p.pos - s
        };
        let mut n = digits(self);
        if self.eat(b'.') {
            n += digits(self);
        }
        if n == 0 {
            return Err(self.err("invalid number"));
        }
        if self.eat(b'e') || self.eat(b'E') {
            if !self.eat(b'+') {
                self.eat(b'-');
            }
            if digits(self) == 0 {
                return Err(self.err("invalid number exponent"));
            }
        }
        if self
            .peek()
            .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.')
        {
            return Err(self.err("invalid number"));
        }
        let text = std::str::from_utf8(&self.s[start..self.pos]).expect("ASCII");
        match text.parse::<f64>() {
            Ok(v) if v.is_finite() => Ok(text.to_string()),
            _ => Err(self.err("number is not finite")),
        }
    }

    /// Quoted string with Lua 5.4 escapes; the result must be valid UTF-8.
    fn string(&mut self) -> EngineResult<String> {
        let quote = self.peek().expect("quote");
        self.pos += 1;
        let mut out = Vec::new();
        loop {
            let Some(b) = self.peek() else {
                return Err(self.err("unterminated string"));
            };
            self.pos += 1;
            match b {
                _ if b == quote => break,
                b'\n' | b'\r' => return Err(self.err("unescaped newline in string")),
                b'\\' => self.escape(&mut out)?,
                _ => out.push(b),
            }
        }
        String::from_utf8(out).map_err(|_| self.err("string is not valid UTF-8"))
    }

    fn escape(&mut self, out: &mut Vec<u8>) -> EngineResult<()> {
        let Some(b) = self.peek() else {
            return Err(self.err("unterminated string"));
        };
        self.pos += 1;
        let simple = match b {
            b'a' => Some(7),
            b'b' => Some(8),
            b'f' => Some(12),
            b'n' | b'\n' => Some(b'\n'),
            b'r' => Some(b'\r'),
            b't' => Some(b'\t'),
            b'v' => Some(11),
            b'\\' | b'"' | b'\'' => Some(b),
            _ => None,
        };
        if let Some(c) = simple {
            // `\` + CR LF (or LF CR) is one escaped newline.
            if b == b'\n' && self.peek() == Some(b'\r') {
                self.pos += 1;
            }
            out.push(c);
            return Ok(());
        }
        match b {
            b'\r' => {
                self.eat(b'\n');
                out.push(b'\n');
            }
            b'z' => self.ws(),
            b'x' => {
                let hex = self
                    .s
                    .get(self.pos..self.pos + 2)
                    .and_then(|h| std::str::from_utf8(h).ok())
                    .and_then(|h| u8::from_str_radix(h, 16).ok())
                    .ok_or_else(|| self.err("invalid \\x escape"))?;
                self.pos += 2;
                out.push(hex);
            }
            b'0'..=b'9' => {
                let mut v = u32::from(b - b'0');
                for _ in 0..2 {
                    match self.peek() {
                        Some(d @ b'0'..=b'9') => {
                            v = v * 10 + u32::from(d - b'0');
                            self.pos += 1;
                        }
                        _ => break,
                    }
                }
                out.push(u8::try_from(v).map_err(|_| self.err("decimal escape too large"))?);
            }
            b'u' => {
                if !self.eat(b'{') {
                    return Err(self.err("invalid \\u escape"));
                }
                let start = self.pos;
                while self.peek().is_some_and(|b| b.is_ascii_hexdigit()) && self.pos - start < 8 {
                    self.pos += 1;
                }
                let c = std::str::from_utf8(&self.s[start..self.pos])
                    .ok()
                    .and_then(|h| u32::from_str_radix(h, 16).ok())
                    .and_then(char::from_u32)
                    .filter(|_| self.eat(b'}'))
                    .ok_or_else(|| self.err("invalid \\u escape"))?;
                out.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
            }
            _ => return Err(self.err("invalid escape")),
        }
        Ok(())
    }
}

/// Parse a Lua develop row into the recipe [`crate::xmp::parse`] would build
/// from the same settings written as XMP.
pub fn parse(text: &str, process_version: &str) -> EngineResult<(Recipe, Vec<String>)> {
    let LuaValue::Table(table) = read(text)? else {
        return Err(error("develop settings are not a table"));
    };
    let (packet, notes) = to_xmp(&table);
    let (mut recipe, mut warnings) = crate::xmp::parse(&packet, process_version)?;
    if !notes.is_empty() {
        recipe
            .unknown
            .insert("lrcat_develop_lua".into(), json!(text));
        warnings.extend(notes);
    }
    Ok((recipe, warnings))
}

const CRS_URI: &str = engine_api::recipe::crs::CRS_NAMESPACE;
const AUX_URI: &str = engine_api::recipe::crs::AUX_NAMESPACE;

/// Render mapped keys as an XMP packet in Adobe's shape. Returns the packet and
/// one note per key that is unknown or has no XMP shape.
fn to_xmp(table: &LuaTable) -> (String, Vec<String>) {
    let mut notes = Vec::new();
    if !table.items.is_empty() {
        notes.push("positional entries in the develop table; source preserved".into());
    }
    let (mut attrs, mut body) = (String::new(), String::new());
    for (key, value) in &table.fields {
        let LuaKey::Str(key) = key else {
            notes.push("numeric develop key: unknown Lua develop key; source preserved".into());
            continue;
        };
        let Some((_, crs)) = KEY_MAP.iter().find(|(lua, _)| lua == key) else {
            notes.push(format!("{key}: unknown Lua develop key; source preserved"));
            continue;
        };
        let prefix = CrsKey::from_xmp_name(crs).map_or("crs", |k| k.namespace().prefix());
        let name = format!("{prefix}:{crs}");
        let rendered = match value {
            LuaValue::Nil => continue,
            LuaValue::Table(t) => element(&name, t).map(|e| body.push_str(&e)),
            scalar => scalar_text(scalar).map(|v| attrs.push_str(&format!(" {name}=\"{v}\""))),
        };
        if let Err(reason) = rendered {
            notes.push(format!("{key}: {reason}; source preserved"));
        }
    }
    let packet = format!(
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:crs="{CRS_URI}" xmlns:aux="{AUX_URI}"{attrs}>{body}</rdf:Description></rdf:RDF></x:xmpmeta>"#
    );
    (packet, notes)
}

fn is_ident(s: &str) -> bool {
    let mut b = s.bytes();
    b.next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
        && b.all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

fn is_lang(s: &str) -> bool {
    !s.is_empty()
        && s.split('-')
            .all(|p| (1..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphanumeric()))
}

/// XML-escaped scalar text (`True`/`False` for booleans, as Adobe writes).
fn scalar_text(v: &LuaValue) -> Result<String, String> {
    match v {
        LuaValue::Bool(true) => Ok("True".into()),
        LuaValue::Bool(false) => Ok("False".into()),
        LuaValue::Number(n) => Ok(n.clone()),
        LuaValue::String(s) => {
            let mut out = String::with_capacity(s.len());
            for c in s.chars() {
                match c {
                    '&' => out.push_str("&amp;"),
                    '<' => out.push_str("&lt;"),
                    '>' => out.push_str("&gt;"),
                    '"' => out.push_str("&quot;"),
                    '\t' => out.push_str("&#9;"),
                    '\n' => out.push_str("&#10;"),
                    '\r' => out.push_str("&#13;"),
                    c if (c as u32) < 0x20 || matches!(c, '\u{FFFE}' | '\u{FFFF}') => {
                        return Err("control character has no XMP form".into());
                    }
                    c => out.push(c),
                }
            }
            Ok(out)
        }
        LuaValue::Nil => Err("nil list item has no XMP form".into()),
        LuaValue::Table(_) => Err("nested list has no XMP form".into()),
    }
}

/// A table as XMP: positional → `rdf:Seq` (numeric tone curves → `"x, y"`
/// points), identifier keys → `rdf:Description`, language keys → `rdf:Alt`.
/// Depth is bounded by the parser's [`MAX_DEPTH`].
fn element(name: &str, t: &LuaTable) -> Result<String, String> {
    let fields: Vec<_> = t
        .fields
        .iter()
        .filter(|(_, v)| *v != LuaValue::Nil)
        .collect();
    if fields.iter().any(|(k, _)| matches!(k, LuaKey::Num(_))) {
        return Err("numeric table key has no XMP form".into());
    }
    let key = |k: &LuaKey| match k {
        LuaKey::Str(s) | LuaKey::Num(s) => s.clone(),
    };
    if fields.is_empty() {
        if t.items.is_empty() {
            return Ok(format!("<{name}><rdf:Seq/></{name}>"));
        }
        let numbers = t.items.iter().all(|v| matches!(v, LuaValue::Number(_)));
        let items = if numbers && name.contains("Curve") {
            if !t.items.len().is_multiple_of(2) {
                return Err("curve has an odd number of coordinates".into());
            }
            t.items
                .chunks(2)
                .map(|p| {
                    Ok(format!(
                        "<rdf:li>{}, {}</rdf:li>",
                        scalar_text(&p[0])?,
                        scalar_text(&p[1])?
                    ))
                })
                .collect::<Result<String, String>>()?
        } else {
            t.items
                .iter()
                .map(|v| match v {
                    LuaValue::Table(t) if t.items.is_empty() => {
                        Ok(format!("<rdf:li>{}</rdf:li>", description(t)?))
                    }
                    v => Ok(format!("<rdf:li>{}</rdf:li>", scalar_text(v)?)),
                })
                .collect::<Result<String, String>>()?
        };
        return Ok(format!("<{name}><rdf:Seq>{items}</rdf:Seq></{name}>"));
    }
    if !t.items.is_empty() {
        return Err("table mixes list and keyed entries".into());
    }
    if fields.iter().all(|(k, _)| is_ident(&key(k))) {
        return Ok(format!("<{name}>{}</{name}>", description(t)?));
    }
    if fields
        .iter()
        .all(|(k, v)| is_lang(&key(k)) && matches!(v, LuaValue::String(_)))
    {
        let items = fields
            .iter()
            .map(|(k, v)| {
                Ok(format!(
                    "<rdf:li xml:lang=\"{}\">{}</rdf:li>",
                    key(k),
                    scalar_text(v)?
                ))
            })
            .collect::<Result<String, String>>()?;
        return Ok(format!("<{name}><rdf:Alt>{items}</rdf:Alt></{name}>"));
    }
    Err("table key has no XMP form".into())
}

/// Keyed table → `rdf:Description`: scalars as attributes, tables as children.
fn description(t: &LuaTable) -> Result<String, String> {
    let (mut attrs, mut body) = (String::new(), String::new());
    for (k, v) in &t.fields {
        let LuaKey::Str(k) = k else {
            return Err("numeric table key has no XMP form".into());
        };
        if !is_ident(k) {
            return Err("table key has no XMP form".into());
        }
        match v {
            LuaValue::Nil => (),
            LuaValue::Table(t) => body.push_str(&element(&format!("crs:{k}"), t)?),
            v => attrs.push_str(&format!(" crs:{k}=\"{}\"", scalar_text(v)?)),
        }
    }
    Ok(if body.is_empty() {
        format!("<rdf:Description{attrs}/>")
    } else {
        format!("<rdf:Description{attrs}>{body}</rdf:Description>")
    })
}
