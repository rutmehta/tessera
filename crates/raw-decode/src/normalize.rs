//! Private owned normalization shared with legacy linearize.
//! No public conversion API, allocation cap or provenance claim.
#[cfg(test)]
use engine_api::EngineError;
use engine_api::{EngineResult, jobs::CancellationToken};

use super::{CfaImage, CfaLayout};

/// Work quantum, not a memory budget: checks also occur within a very wide row.
const CHECKPOINT_SAMPLES: usize = 1024;

pub(super) struct PackedPlane {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) layout: CfaLayout,
    pub(super) samples: Vec<u16>,
    pub(super) black: [f32; 4],
    pub(super) white: u32,
}

#[derive(Debug, PartialEq, Eq)]
struct Sizes {
    samples: usize,
    output_bytes: usize,
}

fn checked_sizes(width: u32, height: u32, len: usize) -> EngineResult<Sizes> {
    let invalid = || super::decode_error("invalid packed CFA dimensions or allocation size");
    if width == 0 || height == 0 {
        return Err(super::decode_error(
            "LibRaw did not return a packed CFA plane",
        ));
    }
    let samples = usize::try_from(u64::from(width) * u64::from(height)).map_err(|_| invalid())?;
    let output_bytes = samples
        .checked_mul(std::mem::size_of::<f32>())
        .ok_or_else(invalid)?;
    if samples != len {
        return Err(super::decode_error(
            "LibRaw did not return a packed CFA plane",
        ));
    }
    if output_bytes > isize::MAX as usize {
        return Err(invalid());
    }
    Ok(Sizes {
        samples,
        output_bytes,
    })
}

fn reserve_pixels(pixels: &mut Vec<f32>, count: usize) -> EngineResult<()> {
    pixels
        .try_reserve_exact(count)
        .map_err(|e| super::decode_error(format!("normalization allocation: {e}")))
}

pub(super) fn normalize(plane: PackedPlane, cancel: &CancellationToken) -> EngineResult<CfaImage> {
    #[cfg(test)]
    {
        legacy_tests::record_entry(&plane, cancel);
        normalize_observed(
            plane,
            cancel,
            &mut Hooks {
                reserve: &mut reserve_pixels,
                observe: &mut |_| {},
            },
        )
    }
    #[cfg(not(test))]
    normalize_inner(plane, cancel)
}

// Test-only seam. Observations carry values only. No production generic callback.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    Validated,
    Reserved {
        input_ptr: usize,
        input_capacity: usize,
        output_ptr: usize,
        output_capacity: usize,
    },
    Chunk {
        completed: usize,
    },
    BeforePublish,
}

#[cfg(test)]
struct Hooks<'a> {
    reserve: &'a mut dyn FnMut(&mut Vec<f32>, usize) -> EngineResult<()>,
    observe: &'a mut dyn FnMut(Event),
}

#[cfg(test)]
fn normalize_observed(
    plane: PackedPlane,
    cancel: &CancellationToken,
    hooks: &mut Hooks<'_>,
) -> EngineResult<CfaImage> {
    normalize_inner(plane, cancel, hooks)
}

fn normalize_inner(
    plane: PackedPlane,
    cancel: &CancellationToken,
    #[cfg(test)] hooks: &mut Hooks<'_>,
) -> EngineResult<CfaImage> {
    cancel.check()?;
    let sizes = checked_sizes(plane.width, plane.height, plane.samples.len())?;
    let valid_layout = match plane.layout {
        CfaLayout::Bayer(p) => p.iter().flatten().all(|&c| c < 4),
        CfaLayout::XTrans(p) => p.iter().flatten().all(|&c| c < 3),
        CfaLayout::Unsupported => false,
    };
    if !valid_layout {
        return Err(super::decode_error("unsupported CFA layout"));
    }
    if plane
        .black
        .iter()
        .any(|&b| !b.is_finite() || b >= plane.white as f32)
    {
        return Err(super::decode_error("invalid sensor black/white levels"));
    }
    #[cfg(test)]
    (hooks.observe)(Event::Validated);
    cancel.check()?;
    let mut pixels = Vec::new();
    // A reservation error takes precedence over cancellation during that operation.
    #[cfg(test)]
    (hooks.reserve)(&mut pixels, sizes.samples)?;
    #[cfg(not(test))]
    reserve_pixels(&mut pixels, sizes.samples)?;
    // A test hook cannot accidentally fall back to infallible push allocation.
    if !pixels.is_empty() || pixels.capacity() < sizes.samples {
        return Err(super::decode_error(
            "normalization reservation did not provide capacity",
        ));
    }
    // Actual live capacities, not logical lengths; this is arithmetic, not a quota.
    let input_bytes = plane
        .samples
        .capacity()
        .checked_mul(std::mem::size_of::<u16>())
        .ok_or_else(|| super::decode_error("normalization input capacity overflow"))?;
    let output_bytes = pixels
        .capacity()
        .checked_mul(std::mem::size_of::<f32>())
        .ok_or_else(|| super::decode_error("normalization output capacity overflow"))?;
    debug_assert!(output_bytes >= sizes.output_bytes);
    input_bytes
        .checked_add(output_bytes)
        .ok_or_else(|| super::decode_error("normalization live capacity overflow"))?;
    #[cfg(test)]
    (hooks.observe)(Event::Reserved {
        input_ptr: plane.samples.as_ptr() as usize,
        input_capacity: plane.samples.capacity(),
        output_ptr: pixels.as_ptr() as usize,
        output_capacity: pixels.capacity(),
    });
    cancel.check()?;
    let width = plane.width as usize;
    for chunk in plane.samples.chunks(CHECKPOINT_SAMPLES) {
        cancel.check()?;
        for &value in chunk {
            let i = pixels.len();
            let channel = plane
                .layout
                .channel_at((i % width) as u32, (i / width) as u32);
            let black = plane.black[channel];
            pixels.push(((value as f32 - black) / (plane.white as f32 - black)).clamp(0.0, 1.2));
        }
        #[cfg(test)]
        (hooks.observe)(Event::Chunk {
            completed: pixels.len(),
        });
        cancel.check()?;
    }
    #[cfg(test)]
    (hooks.observe)(Event::BeforePublish);
    cancel.check()?;
    // Move the normalized allocation directly, avoiding from_linear's full-plane scan.
    // The owned input drops on return (and both local buffers drop on any error).
    Ok(CfaImage {
        pyramid: super::CfaPyramid {
            extent: super::Extent::new(plane.width, plane.height),
            pixels,
        },
    })
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod legacy_tests;
