//! Private restricted RAW admission predicates. No pixel-producing implementation.
//!
//! Initially included only in unit-test builds; no production resolver or caller.
//! These private facts never certify captured provenance, a native memory bound,
//! an ICC profile, or a trusted execution environment.
use engine_api::color::ColorMatrix3;
use engine_api::pinned_raw::PinnedRawDescriptor;
use engine_api::recipe::Recipe;
use engine_api::recipe::settings::{
    CameraProfileSettings, DemosaicMethod, DenoiseMethod, GeometrySettings, LensProfileSource,
    LensSettings, WhiteBalanceMode, WhiteBalanceSettings,
};
use raw_decode::CfaLayout;
use raw_decode::capture::DecodedCapturedCfa;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refusal {
    Demosaic,
    Denoise,
    Lens,
    Geometry,
    CameraProfile,
    LocalEdits,
    Depth,
    ColorDependency,
    Output,
    UnsupportedSettings,
    Dimensions,
    CfaLayout,
    Crop,
    Orientation,
    CorrectionMetadata,
    Calibration,
    Budget,
    Overflow,
}

/// Descriptive metadata facts only, never an authenticated capture capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MetadataFacts {
    sensor_pixels: u64,
    active_pixels: u64,
}

/// Explicit arithmetic inputs, not an audited renderer's allocation inventory.
/// The arithmetic includes U16+F32 sensor coexistence and opaque U16 RGBA output.
/// Caller-supplied scratch/metadata counts do not prove completeness or reserve memory.
#[derive(Debug, Clone, Copy)]
struct AllocationInventory {
    sensor_pixels: u64,
    active_pixels: u64,
    metadata_bytes: u64,
    scratch_bytes: u64,
}

/// Test-injected finite limits; no production default or numerical policy selected.
#[derive(Debug, Clone, Copy)]
struct CheckedLimits {
    sensor_pixels: u64,
    active_pixels: u64,
    output_bytes: u64,
    accounted_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AccountedBytes {
    output: u64,
    total: u64,
}

fn admit_recipe(descriptor: &PinnedRawDescriptor) -> Result<Recipe, Refusal> {
    // Descriptor construction already validates explicit headers and current-settings
    // shape. Parse its retained bytes through the existing compatibility authority;
    // never demand equality with a reserialized/default-expanded projection.
    let recipe =
        Recipe::from_json(descriptor.recipe_json()).map_err(|_| Refusal::UnsupportedSettings)?;
    let settings = &recipe.settings;
    // Redundant with the current descriptor constructor, intentionally explicit
    // so the profile remains neutral if that upstream contract is later widened.
    if settings.geometry != GeometrySettings::default() {
        return Err(Refusal::Geometry);
    }
    if settings.demosaic.method != DemosaicMethod::Bilinear || settings.demosaic.model.is_some() {
        return Err(Refusal::Demosaic);
    }
    if !matches!(settings.denoise.method, DenoiseMethod::Off) {
        return Err(Refusal::Denoise);
    }
    let neutral_lens = LensSettings {
        profile: LensProfileSource::None,
        remove_chromatic_aberration: false,
        ..LensSettings::default()
    };
    if settings.lens != neutral_lens {
        return Err(Refusal::Lens);
    }
    if settings.camera_profile != CameraProfileSettings::default() {
        return Err(Refusal::CameraProfile);
    }
    if !settings.locals.adjustments.is_empty() || !settings.locals.retouch.is_empty() {
        return Err(Refusal::LocalEdits);
    }
    if settings.effects.lens_blur.is_some() {
        return Err(Refusal::Depth);
    }
    if !settings.color.point_colors.is_empty() || settings.color.lut.is_some() {
        return Err(Refusal::ColorDependency);
    }
    if settings.output.hdr
        || settings.output.hdr_headroom_stops != 0.0
        || settings.output.proof_profile.is_some()
    {
        return Err(Refusal::Output);
    }
    // validate_settings copies WB as supported; Native2 cannot resolve Auto WB.
    if settings.white_balance.mode == WhiteBalanceMode::Auto {
        return Err(Refusal::UnsupportedSettings);
    }
    pipeline_cpu::validate_settings(settings).map_err(|_| Refusal::UnsupportedSettings)?;
    Ok(recipe)
}

fn admit_metadata(decoded: &DecodedCapturedCfa) -> Result<MetadataFacts, Refusal> {
    let image = &decoded.image;
    let metadata = &decoded.metadata;
    // A Bayer source must contain a full period. No sample allocation or decode occurs.
    if image.width < 2
        || image.height < 2
        || image.width != metadata.width
        || image.height != metadata.height
    {
        return Err(Refusal::Dimensions);
    }
    let sensor_pixels = u64::from(image.width)
        .checked_mul(u64::from(image.height))
        .ok_or(Refusal::Overflow)?;
    if u64::try_from(image.data.len()).map_err(|_| Refusal::Overflow)? != sensor_pixels {
        return Err(Refusal::Dimensions);
    }
    if image.cfa_layout != metadata.cfa_layout {
        return Err(Refusal::CfaLayout);
    }
    let CfaLayout::Bayer(pattern) = image.cfa_layout else {
        return Err(Refusal::CfaLayout);
    };
    let channels = [pattern[0][0], pattern[0][1], pattern[1][0], pattern[1][1]];
    let red = channels.iter().position(|&channel| channel == 0);
    let blue = channels.iter().position(|&channel| channel == 2);
    // R and B occupy opposite corners; both remaining corners are green (1 or 3).
    if channels.iter().filter(|&&channel| channel == 0).count() != 1
        || channels.iter().filter(|&&channel| channel == 2).count() != 1
        || channels
            .iter()
            .filter(|&&channel| channel == 1 || channel == 3)
            .count()
            != 2
        || !matches!((red, blue), (Some(r), Some(b)) if r ^ b == 3)
    {
        return Err(Refusal::CfaLayout);
    }
    let [left, top, width, height] = metadata.default_crop;
    if width == 0
        || height == 0
        || u64::from(left) + u64::from(width) > u64::from(image.width)
        || u64::from(top) + u64::from(height) > u64::from(image.height)
    {
        return Err(Refusal::Crop);
    }
    let active_pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or(Refusal::Overflow)?;
    if metadata.orientation != 1 {
        return Err(Refusal::Orientation);
    }
    if metadata.has_gain_map
        || metadata.has_opcode_list
        || metadata.opcode_lists.iter().any(Option::is_some)
        || metadata.maker_lens.is_some()
    {
        return Err(Refusal::CorrectionMetadata);
    }
    if metadata.white_level == 0
        || metadata
            .black_levels
            .iter()
            .any(|&black| !black.is_finite() || black >= metadata.white_level as f32)
        || metadata.as_shot_wb[..3]
            .iter()
            .any(|&wb| !wb.is_finite() || wb <= 0.0)
    {
        return Err(Refusal::Calibration);
    }
    // Keep the reviewed conservative stored-matrix checks, separately from the
    // actual Native2 calibration, which inverts only the first three cam_xyz rows.
    if metadata
        .camera_to_xyz
        .0
        .iter()
        .flatten()
        .any(|value| !value.is_finite())
    {
        return Err(Refusal::Calibration);
    }
    metadata
        .camera_to_xyz
        .inverse()
        .map_err(|_| Refusal::Calibration)?;
    let cam_xyz = ColorMatrix3(std::array::from_fn(|row| {
        metadata.cam_xyz[row].map(f64::from)
    }));
    if cam_xyz.0.iter().flatten().any(|value| !value.is_finite()) {
        return Err(Refusal::Calibration);
    }
    let camera_xyz = pipeline_cpu::camera_to_xyz(cam_xyz).map_err(|_| Refusal::Calibration)?;
    // Invertibility alone does not establish a usable as-shot scene white.
    pipeline_cpu::white_balance_matrix(
        &WhiteBalanceSettings::default(),
        camera_xyz,
        metadata.as_shot_wb,
    )
    .map_err(|_| Refusal::Calibration)?;
    // Deliberately no identity/route authentication: these are public mutable
    // synthetic values, and a declared route is not a content classification.
    Ok(MetadataFacts {
        sensor_pixels,
        active_pixels,
    })
}

fn check_inventory(
    inventory: AllocationInventory,
    limits: CheckedLimits,
) -> Result<AccountedBytes, Refusal> {
    if inventory.sensor_pixels == 0
        || inventory.active_pixels == 0
        || inventory.active_pixels > inventory.sensor_pixels
    {
        return Err(Refusal::Dimensions);
    }
    let sensor_bytes = inventory
        .sensor_pixels
        .checked_mul(6)
        .ok_or(Refusal::Overflow)?;
    let output = inventory
        .active_pixels
        .checked_mul(8)
        .ok_or(Refusal::Overflow)?;
    let total = sensor_bytes
        .checked_add(output)
        .and_then(|bytes| bytes.checked_add(inventory.metadata_bytes))
        .and_then(|bytes| bytes.checked_add(inventory.scratch_bytes))
        .ok_or(Refusal::Overflow)?;
    if inventory.sensor_pixels > limits.sensor_pixels
        || inventory.active_pixels > limits.active_pixels
        || output > limits.output_bytes
        || total > limits.accounted_bytes
    {
        return Err(Refusal::Budget);
    }
    Ok(AccountedBytes { output, total })
}

#[cfg(test)]
mod tests;
