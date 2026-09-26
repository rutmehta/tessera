//! ICC Color Lookup import delegates profile interpretation to the color engine.
use super::Adjustment;
use engine_api::{EngineError, EngineResult};

impl Adjustment {
    /// Load an ICC abstract look or RGB-to-RGB device link as a sampled 3D LUT.
    ///
    /// Abstract looks use encoded sRGB input/output with relative colorimetry.
    /// Device links use their own RGB encodings directly, without additional
    /// color conversions. `size` must be 2..=256 (33 is a useful default).
    /// Sampling is red-fastest and uses the existing trilinear ColorLookup path.
    /// Unsupported profile classes/spaces and malformed profiles return errors.
    /// See [`color_mgmt::sample_color_lookup_icc`] for the sampling contract.
    pub fn color_lookup_from_icc(bytes: &[u8], size: u32) -> EngineResult<Self> {
        let lut = color_mgmt::sample_color_lookup_icc(bytes, size)
            .map_err(|error| EngineError::invalid("color_lookup", error.to_string()))?;
        Ok(Self::ColorLookup {
            size,
            data: lut.values,
        })
    }
}
