//! Console-owned decoded sources and persistent stage caches. RAW uses the
//! resident GPU renderer when available. RGB uses the CPU preview fallback:
//! image-core currently accepts CFA sources only.
use crate::pixels::{self, Source};
use engine_api::{EngineError, EngineResult, id::ImageId, recipe::Recipe, tile::TILE_SIZE};
use image_core::{PixelRect, RawImage, RenderOutput, Renderer, RendererConfig, TileCache};
use pipeline_cpu::{Image, RenderSource};
#[path = "critic.rs"]
mod critic;
#[cfg(test)]
#[path = "preview_tests.rs"]
mod tests;
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex, OnceLock},
};

enum Decoded {
    Raw(RawImage),
    Rgb { full: Image, preview: Image },
}
#[derive(Default)]
pub(crate) struct PreviewCache {
    sources: Mutex<HashMap<ImageId, Arc<Decoded>>>,
    renderer: OnceLock<Renderer>,
}
fn level(width: u32, height: u32, max: u32) -> u8 {
    let mut level = 0;
    while width.div_ceil(1 << level).max(height.div_ceil(1 << level)) > max && level < 12 {
        level += 1;
    }
    level
}
impl PreviewCache {
    fn renderer(&self) -> &Renderer {
        self.renderer.get_or_init(|| {
            // Share device initialization, not image identities or memo caches.
            static GPU: OnceLock<Option<Arc<pipeline_gpu::GpuContext>>> = OnceLock::new();
            let config = RendererConfig::default();
            match GPU.get_or_init(|| pipeline_gpu::GpuContext::new().ok().map(Arc::new)) {
                Some(gpu) => Renderer::with_ops(
                    // Retain WB, Detail and encoded frames together (36MP NEF),
                    // plus preview/crop entries. Still a bounded 1.5 GiB LRU.
                    Arc::new(pipeline_gpu::GpuStageOp::with_cache_budget(
                        gpu.clone(),
                        1536 << 20,
                    )),
                    Arc::new(TileCache::new(config.cache_budget_bytes)),
                    config,
                ),
                None => Renderer::new(config),
            }
        })
    }
    fn source(&self, id: ImageId, path: &Path) -> EngineResult<Arc<Decoded>> {
        let mut sources = self
            .sources
            .lock()
            .map_err(|_| EngineError::internal("preview cache poisoned"))?;
        if let Some(source) = sources.get(&id) {
            return Ok(source.clone());
        }
        let source = Arc::new(if pixels::is_rgb(path) {
            let Source::Rgb(full) = Source::open(path)? else {
                unreachable!()
            };
            let scale = 1 << level(full.width(), full.height(), 1024);
            let preview = full.downsample_crop([0, 0, full.width(), full.height()], scale)?;
            Decoded::Rgb { full, preview }
        } else {
            Decoded::Raw(RawImage::open(id, path)?)
        });
        sources.insert(id, source.clone());
        Ok(source)
    }
    pub(crate) fn display(
        &self,
        id: ImageId,
        path: &Path,
        recipe: &Recipe,
        max: Option<u32>,
    ) -> EngineResult<image::RgbImage> {
        let source = self.source(id, path)?;
        let rgb = match &*source {
            Decoded::Rgb { full, preview } => {
                let input = if max.is_some() { preview } else { full };
                pipeline_cpu::render(&recipe.settings, &RenderSource::Rgb(input))?
            }
            Decoded::Raw(raw) => {
                let e = raw.active_extent();
                let l = if max.is_some() {
                    level(e.width, e.height, 1024)
                } else {
                    0
                };
                let e = Renderer::output_extent(raw, &recipe.settings, l)?;
                let tiles = self.renderer().for_recipe(recipe).render_region(
                    raw,
                    &recipe.settings,
                    l,
                    PixelRect::full(e),
                )?;
                let mut rgb = image::RgbImage::new(e.width, e.height);
                for tile in tiles {
                    let layout = tile.layout();
                    let data = tile.samples::<u8>()?;
                    let (ox, oy) = tile.coord().pixel_origin(TILE_SIZE);
                    for y in 0..layout.extent.height {
                        for x in 0..layout.extent.width {
                            rgb.put_pixel(
                                ox + x,
                                oy + y,
                                image::Rgb(std::array::from_fn(|c| {
                                    data[layout.index(c as u8, x as i32, y as i32).unwrap()]
                                })),
                            );
                        }
                    }
                }
                pixels::orient(rgb, raw.metadata().orientation)
            }
        };
        if let Some(max) = max
            && rgb.width().max(rgb.height()) > max
        {
            return Ok(image::DynamicImage::ImageRgb8(rgb)
                .resize(max, max, image::imageops::FilterType::Triangle)
                .to_rgb8());
        }
        Ok(rgb)
    }
    pub(crate) fn linear(&self, id: ImageId, path: &Path, recipe: &Recipe) -> EngineResult<Image> {
        let source = self.source(id, path)?;
        match &*source {
            Decoded::Rgb { preview, .. } => {
                pipeline_cpu::render_linear_scaled(&recipe.settings, &RenderSource::Rgb(preview), 1)
            }
            Decoded::Raw(raw) => {
                let e = raw.active_extent();
                let l = level(e.width, e.height, 1024);
                let e = Renderer::output_extent(raw, &recipe.settings, l)?;
                let tiles = self.renderer().for_recipe(recipe).render_region_as(
                    raw,
                    &recipe.settings,
                    l,
                    PixelRect::full(e),
                    RenderOutput::SceneLinear,
                )?;
                let mut planes = vec![vec![0.; e.width as usize * e.height as usize]; 3];
                for tile in tiles {
                    let layout = tile.layout();
                    let data = tile.samples::<f32>()?;
                    let (ox, oy) = tile.coord().pixel_origin(TILE_SIZE);
                    for (c, plane) in planes.iter_mut().enumerate() {
                        for y in 0..layout.extent.height {
                            for x in 0..layout.extent.width {
                                plane[((oy + y) * e.width + ox + x) as usize] =
                                    data[layout.index(c as u8, x as i32, y as i32).unwrap()];
                            }
                        }
                    }
                }
                Image::new(e.width, e.height, planes)
            }
        }
    }
}
