//! Native photo-filter swatches; not an Adobe colorimetric match.
use super::Adjustment;
use engine_api::{EngineError, EngineResult};
use serde::{Deserialize, Serialize};

/// Named approximate sRGB-encoded filter swatches. RGB byte constants are
/// documented on each option and normalized by 255, never linearized. These
/// native display-referred approximations are not measured optical filters.
/// Non-sRGB documents must convert the resolved color to their own encoding.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhotoFilterPreset {
    /// Approximate sRGB bytes [236, 138, 0].
    Warming85,
    /// Approximate sRGB bytes [250, 150, 0].
    WarmingLba,
    /// Approximate sRGB bytes [235, 177, 19].
    Warming81,
    /// Approximate sRGB bytes [0, 109, 255].
    Cooling80,
    /// Approximate sRGB bytes [0, 93, 255].
    CoolingLbb,
    /// Approximate sRGB bytes [0, 181, 255].
    Cooling82,
    /// Approximate sRGB bytes [234, 26, 26].
    Red,
    /// Approximate sRGB bytes [243, 132, 23].
    Orange,
    /// Approximate sRGB bytes [249, 227, 28].
    Yellow,
    /// Approximate sRGB bytes [25, 201, 25].
    Green,
    /// Approximate sRGB bytes [29, 201, 201].
    Cyan,
    /// Approximate sRGB bytes [29, 53, 234].
    Blue,
    /// Approximate sRGB bytes [111, 29, 234].
    Violet,
    /// Approximate sRGB bytes [201, 29, 201].
    Magenta,
    /// Approximate sRGB bytes [172, 122, 51].
    Sepia,
    /// Approximate sRGB bytes [158, 0, 0].
    DeepRed,
    /// Approximate sRGB bytes [0, 0, 158].
    DeepBlue,
    /// Approximate sRGB bytes [0, 102, 51].
    DeepEmerald,
    /// Approximate sRGB bytes [255, 204, 0].
    DeepYellow,
    /// Approximate sRGB bytes [0, 194, 177].
    Underwater,
    /// Caller-provided encoded RGB in [0,1], used without conversion.
    Custom([f32; 3]),
}
impl PhotoFilterPreset {
    /// Resolve to the existing pixel operator. Density is a percent [0,100].
    /// Reject invalid controls rather than silently clamping them.
    pub fn resolve(self, density: f32, preserve_luminosity: bool) -> EngineResult<Adjustment> {
        let color = match self {
            Self::Warming85 => [236, 138, 0].map(|v| v as f32 / 255.0),
            Self::WarmingLba => [250, 150, 0].map(|v| v as f32 / 255.0),
            Self::Warming81 => [235, 177, 19].map(|v| v as f32 / 255.0),
            Self::Cooling80 => [0, 109, 255].map(|v| v as f32 / 255.0),
            Self::CoolingLbb => [0, 93, 255].map(|v| v as f32 / 255.0),
            Self::Cooling82 => [0, 181, 255].map(|v| v as f32 / 255.0),
            Self::Red => [234, 26, 26].map(|v| v as f32 / 255.0),
            Self::Orange => [243, 132, 23].map(|v| v as f32 / 255.0),
            Self::Yellow => [249, 227, 28].map(|v| v as f32 / 255.0),
            Self::Green => [25, 201, 25].map(|v| v as f32 / 255.0),
            Self::Cyan => [29, 201, 201].map(|v| v as f32 / 255.0),
            Self::Blue => [29, 53, 234].map(|v| v as f32 / 255.0),
            Self::Violet => [111, 29, 234].map(|v| v as f32 / 255.0),
            Self::Magenta => [201, 29, 201].map(|v| v as f32 / 255.0),
            Self::Sepia => [172, 122, 51].map(|v| v as f32 / 255.0),
            Self::DeepRed => [158, 0, 0].map(|v| v as f32 / 255.0),
            Self::DeepBlue => [0, 0, 158].map(|v| v as f32 / 255.0),
            Self::DeepEmerald => [0, 102, 51].map(|v| v as f32 / 255.0),
            Self::DeepYellow => [255, 204, 0].map(|v| v as f32 / 255.0),
            Self::Underwater => [0, 194, 177].map(|v| v as f32 / 255.0),
            Self::Custom(color) => color,
        };
        if !density.is_finite()
            || !(0.0..=100.0).contains(&density)
            || color
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err(EngineError::invalid(
                "photo_filter",
                "color [0,1] and density [0,100] must be finite",
            ));
        }
        Ok(Adjustment::PhotoFilter {
            color,
            density,
            preserve_luminosity,
        })
    }
}
