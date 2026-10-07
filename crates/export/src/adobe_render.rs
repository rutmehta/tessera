//! ENG-10: Adobe-process (imported Lightroom) outputs are drawn by Develop's
//! own renderer, at the pyramid level that matches the render scale.
//!
//! Develop draws an Adobe recipe with `image_core::Renderer` (the
//! compatibility stages of `AdobeStageOp` over the native CPU operators).
//! A level-`L` frame box-averages the white-balanced frame first and runs
//! Detail, Tone, Colour, locals, Effects and Geometry on level pixels (the
//! documented preview order, ENG-6). An export or print at render scale
//! `2^L` is that frame, so it shows exactly what Develop shows at that
//! level instead of developing at full resolution and reducing afterwards.
//! Level 0 is Develop's full-resolution reference path, which equals
//! `pipeline_adobe::render_linear_scaled(.., 1)` within the image-core
//! contract (`compat_matches_standalone_with_and_without_dcp`, 1e-4 linear).
//!
//! The renderer is private to the export: no shared tile cache (budget 0,
//! so nothing is kept at f16 precision), the export's own mask rasters,
//! retouch kernels, depth provider and denoiser (the resources Develop
//! installs), and the export's cancellation token. Tiles are copied into
//! the output frame as they are delivered; no tile vector of the frame is
//! retained.
use engine_api::{
    EngineError, EngineResult,
    id::ImageId,
    jobs::CancellationToken,
    recipe::{DevelopSettings, ProcessVersion},
    tile::{Pyramid as _, TILE_SIZE},
};
use image_core::{
    PixelRect, RawImage, RenderOutput, Renderer, RendererConfig, depth::DepthProvider,
    mask_cache::MaskHooks,
};
use pipeline_cpu::RenderSource;
use std::sync::Arc;

/// Caller-owned resources, as the Develop session installs them.
#[derive(Default)]
pub(crate) struct Resources {
    pub masks: Option<Arc<dyn MaskHooks>>,
    pub retouch: Option<Arc<dyn pipeline_cpu::RetouchRenderer>>,
    pub depth: Option<Arc<DepthProvider>>,
    pub denoiser: Option<image_core::MlCfaDenoise>,
}

/// The pyramid level a render scale selects (1, 2, 4 or 8).
pub(crate) fn level(scale: u32) -> EngineResult<u8> {
    match scale {
        1 | 2 | 4 | 8 => Ok(scale.trailing_zeros() as u8),
        _ => Err(EngineError::invalid("render_scale", "must be 1, 2, 4 or 8")),
    }
}

/// Develop's linear rendering (display-referred linear Rec.2020, before the
/// Output stage) of `source` at the level for `scale`.
pub(crate) fn render(
    source: &RenderSource<'_>,
    process_version: ProcessVersion,
    settings: &DevelopSettings,
    scale: u32,
    resources: Resources,
    cancel: &CancellationToken,
) -> EngineResult<image::Rgb32FImage> {
    let level = level(scale)?;
    cancel.check()?;
    let started = std::time::Instant::now();
    let image = raw_image(source)?;
    let mut renderer = Renderer::new(RendererConfig {
        process_version,
        cache_budget_bytes: 0,
        ..Default::default()
    });
    if let Some(retouch) = resources.retouch {
        renderer = renderer.with_retouch_renderer(retouch);
    }
    if let Some(depth) = resources.depth {
        renderer = renderer.with_depth(depth);
    }
    if let Some(denoiser) = resources.denoiser {
        // Develop installs the packed CFA capability for CFA-selected neural
        // denoise and the post-demosaic adapter otherwise.
        renderer = if pipeline_cpu::cfa_denoise_selected(&settings.denoise) {
            renderer.with_cfa_denoise(Arc::new(denoiser))
        } else {
            renderer.with_post_demosaic_denoise(Arc::new(denoiser))
        };
    }
    if let Some(masks) = resources.masks {
        renderer.mask_cache().set_hooks(Some(masks));
    }
    let extent = Renderer::output_extent(&image, settings, level)?;
    let (width, height) = (extent.width, extent.height);
    let mut out = image::Rgb32FImage::new(width, height);
    let mut failure = None;
    renderer.render_region_into(
        &image,
        settings,
        level,
        PixelRect::full(extent),
        RenderOutput::SceneLinear,
        cancel,
        &mut |tile| {
            if failure.is_some() {
                return;
            }
            let layout = tile.layout();
            let n = layout.plane_len();
            let samples = match tile.samples::<f32>() {
                Ok(samples) => samples,
                Err(e) => {
                    failure = Some(e);
                    return;
                }
            };
            let (ox, oy) = tile.coord().pixel_origin(TILE_SIZE);
            for y in 0..layout.extent.height {
                for x in 0..layout.extent.width {
                    let i = (y * layout.extent.width + x) as usize;
                    let (px, py) = (ox + x, oy + y);
                    if px < width && py < height {
                        out.put_pixel(
                            px,
                            py,
                            image::Rgb(std::array::from_fn(|c| samples[c * n + i])),
                        );
                    }
                }
            }
        },
    )?;
    if let Some(e) = failure {
        return Err(e);
    }
    crate::gpu::trace("Adobe Develop render", started);
    cancel.check()?;
    Ok(out)
}

/// The export's borrowed source as the renderer's owned image. RAW sensor
/// samples and RGB planes are copied once (the renderer owns its source);
/// no developed full-resolution intermediate is made here.
fn raw_image(source: &RenderSource<'_>) -> EngineResult<RawImage> {
    let id = ImageId(1);
    match source {
        RenderSource::Cfa { image, metadata } => {
            let pyramid = image.pyramid();
            let extent = pyramid.extent();
            RawImage::new(
                id,
                Arc::new(raw_decode::CfaImage::from_linear(
                    extent.width,
                    extent.height,
                    pyramid.pixels().to_vec(),
                )?),
                Arc::new((*metadata).clone()),
            )
        }
        RenderSource::Rgb(rgb) => RawImage::from_rgb(
            id,
            image_core::RgbSource::from_linear_rec2020((*rgb).clone())?,
        ),
        RenderSource::CameraLinear(proxy) => {
            RawImage::from_camera_linear_proxy(id, ImageId(2), Arc::new((*proxy).clone()))
        }
    }
}
