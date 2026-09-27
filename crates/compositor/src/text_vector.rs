//! Validation shared by editable content operations and native-file loading.

use crate::{Affine, Layer, VectorMask};
use engine_api::{EngineError, EngineResult};

/// Reject singular or non-finite local-to-document transforms.
pub fn validate_transform(transform: Affine) -> EngineResult<()> {
    if transform.inverse().is_none() {
        return Err(EngineError::invalid(
            "transform",
            "must be finite and invertible",
        ));
    }
    Ok(())
}

/// Validate text without resolving fonts or rasterizing.
pub fn validate_text(model: &typography::TextModel, transform: Affine) -> EngineResult<()> {
    validate_transform(transform)?;
    model
        .validate()
        .map_err(|e| EngineError::invalid("text", e.to_string()))
}

/// Validate vector geometry and paint without rasterizing.
pub fn validate_shape(model: &vector::ShapeModel, transform: Affine) -> EngineResult<()> {
    validate_transform(transform)?;
    model
        .validate()
        .map_err(|e| EngineError::invalid("shape", e.to_string()))
}

/// Validate document-space mask geometry, feather and density.
pub fn validate_mask(mask: &VectorMask) -> EngineResult<()> {
    mask.path
        .validate()
        .map_err(|e| EngineError::invalid("vector_mask", e.to_string()))?;
    if !mask.feather.is_finite()
        || mask.feather < 0.0
        || mask.feather > 1_000_000.0
        || !mask.density.is_finite()
        || !(0.0..=1.0).contains(&mask.density)
    {
        return Err(EngineError::invalid(
            "vector_mask",
            "invalid feather or density",
        ));
    }
    Ok(())
}

pub(crate) fn check_editable(
    layer: &Layer,
    transforms: Option<(Affine, Affine)>,
) -> EngineResult<()> {
    let locks = layer.props.locks;
    if locks.all || locks.pixels {
        return Err(EngineError::invalid("layer", "content is locked"));
    }
    if locks.position && transforms.is_some_and(|(old, new)| old != new) {
        return Err(EngineError::invalid("layer", "position is locked"));
    }
    Ok(())
}
