//! Device-independent execution of the resident graph contract against CPU ops.
#[path = "../tests/common/mod.rs"]
mod common;
use crate::{
    CpuStageOp, Op, PixelRect, RenderOutput, Renderer, RendererConfig, StageOp, TileCache,
    cache::{to_f16, to_f32},
    resident::{ResidentBatch, ResidentOutput, ResidentTile, SurfaceTarget},
};
use engine_api::{
    EngineError, EngineResult,
    jobs::CancellationToken,
    recipe::{DevelopSettings, settings::WhiteBalanceMode},
    stage::{MemoKey, StageId},
    tile::{Extent, Tile, TileCoord},
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
#[derive(Default)]
struct Model {
    cache: Mutex<HashMap<MemoKey, Tile>>,
    matrix_pixels: std::sync::atomic::AtomicU64,
    detail_pixels: std::sync::atomic::AtomicU64,
}
struct Batch<'a> {
    owner: &'a Model,
    pending: HashMap<MemoKey, Tile>,
}
fn resident(tile: Tile) -> ResidentTile {
    ResidentTile {
        coord: tile.coord(),
        layout: tile.layout(),
        storage: Arc::new(tile),
    }
}
fn cpu(tile: &ResidentTile) -> Tile {
    let stored = tile.storage.downcast_ref::<Tile>().unwrap();
    if stored.coord() == tile.coord {
        stored.clone()
    } else if let Ok(samples) = stored.samples::<f32>() {
        Tile::from_samples(tile.coord, tile.layout, samples.to_vec()).unwrap()
    } else {
        Tile::from_samples(
            tile.coord,
            tile.layout,
            stored.samples::<u8>().unwrap().to_vec(),
        )
        .unwrap()
    }
}
impl StageOp for Model {
    fn run(&self, _stage: StageId, _op: &Op<'_>, _input: Tile) -> EngineResult<Tile> {
        panic!("resident graph must not invoke host stage execution")
    }
    fn begin_resident(&self) -> Option<Box<dyn ResidentBatch + '_>> {
        Some(Box::new(Batch {
            owner: self,
            pending: HashMap::new(),
        }))
    }
}
impl ResidentBatch for Batch<'_> {
    fn cached(&mut self, key: &MemoKey) -> EngineResult<Option<ResidentTile>> {
        if let Some(t) = self.pending.get(key) {
            return to_f32(t).map(resident).map(Some);
        }
        self.owner
            .cache
            .lock()
            .unwrap()
            .get(key)
            .map(|t| to_f32(t).map(resident))
            .transpose()
    }
    fn cache(&mut self, key: MemoKey, tile: &ResidentTile) -> EngineResult<ResidentTile> {
        let t = to_f16(&cpu(tile))?;
        self.pending.insert(key, t.clone());
        Ok(resident(to_f32(&t)?))
    }
    fn cache_exact(&mut self, key: MemoKey, tile: &ResidentTile) -> EngineResult<ResidentTile> {
        self.pending.insert(key, cpu(tile));
        Ok(tile.clone())
    }
    fn upload(&mut self, tile: &Tile) -> EngineResult<ResidentTile> {
        Ok(resident(tile.clone()))
    }
    fn run(&mut self, op: &Op<'_>, tile: &ResidentTile) -> EngineResult<ResidentTile> {
        if matches!(op, Op::Matrix(_)) {
            self.owner.matrix_pixels.fetch_add(
                tile.layout.extent.area(),
                std::sync::atomic::Ordering::Relaxed,
            );
        }
        let t = CpuStageOp.run(StageId::Tone, op, cpu(tile))?;
        if matches!(op, Op::Detail(_)) {
            let l = t.layout();
            self.owner
                .detail_pixels
                .fetch_add(l.extent.area(), std::sync::atomic::Ordering::Relaxed);
            let mut data = Vec::new();
            for c in 0..l.channels as usize {
                for y in 0..l.extent.height as usize {
                    let start =
                        c * l.plane_len() + (y + l.halo as usize) * l.stride() + l.halo as usize;
                    data.extend_from_slice(
                        &t.samples::<f32>()?[start..start + l.extent.width as usize],
                    );
                }
            }
            return Tile::from_samples(
                t.coord(),
                engine_api::tile::TileLayout { halo: 0, ..l },
                data,
            )
            .map(resident);
        }
        Ok(resident(t))
    }
    fn gather(
        &mut self,
        frame: Extent,
        coord: TileCoord,
        halo: u16,
        period: u32,
        tiles: &HashMap<TileCoord, ResidentTile>,
    ) -> EngineResult<ResidentTile> {
        let tiles = tiles.iter().map(|(k, t)| (*k, cpu(t))).collect();
        crate::resample::gather(frame, coord, halo, period, &tiles).map(resident)
    }
    fn resample(
        &mut self,
        crop: [u32; 4],
        coord: TileCoord,
        tiles: &HashMap<TileCoord, ResidentTile>,
        accumulated: Option<ResidentTile>,
    ) -> EngineResult<ResidentTile> {
        let mut all: HashMap<_, _> = tiles.iter().map(|(k, t)| (*k, cpu(t))).collect();
        for c in crate::resample::resample_sources(crop, coord) {
            all.entry(c).or_insert_with(|| {
                let layout = engine_api::tile::TileLayout {
                    extent: crate::resample::interior(
                        Extent::new(crop[0] + crop[2], crop[1] + crop[3]),
                        c,
                    ),
                    halo: 0,
                    channels: 3,
                };
                Tile::from_samples(c, layout, vec![0_f32; layout.len()]).unwrap()
            });
        }
        let mut t = crate::resample::resample(crop, coord, &all)?;
        if let Some(previous) = accumulated {
            for (a, b) in t
                .samples_mut::<f32>()?
                .iter_mut()
                .zip(cpu(&previous).samples::<f32>()?)
            {
                *a += b;
            }
        }
        Ok(resident(t))
    }
    fn finish(
        self: Box<Self>,
        tiles: Vec<ResidentTile>,
        _display: bool,
        surface: Option<SurfaceTarget>,
        cancel: &CancellationToken,
    ) -> EngineResult<ResidentOutput> {
        cancel.check()?;
        if surface.is_some() {
            return Err(EngineError::internal("test model has no surface"));
        }
        self.owner.cache.lock().unwrap().extend(self.pending);
        Ok(ResidentOutput {
            tiles: tiles.iter().map(cpu).collect(),
            histogram: None,
            metrics: None,
        })
    }
}
#[test]
fn resident_graph_preserves_crop_phase_parity_and_edit_invalidation() {
    let model = Arc::new(Model::default());
    let cfg = RendererConfig::default();
    let r = Renderer::with_ops(model.clone(), Arc::new(TileCache::new(0)), cfg.clone());
    let cpu = Renderer::new(cfg);
    let image = common::synthetic(1008, 517, 269, common::RGGB, [3, 5, 511, 261]);
    for level in [0, 2, 5, 12] {
        let mut settings = DevelopSettings::default();
        for change in 0..9 {
            if change == 1 {
                settings.tone.exposure = 0.3;
            }
            if change == 2 {
                settings.white_balance.mode = WhiteBalanceMode::Daylight;
            }
            if change == 3 {
                settings.linearize.highlight_reconstruction =
                    engine_api::recipe::settings::HighlightReconstruction::Clip;
            }
            if change == 4 {
                settings.detail.sharpening.amount = 80.0;
            }
            if change == 5 {
                settings.detail.sharpening.amount = 0.0;
                settings.detail.noise_reduction.color = 0.0;
            }
            if change == 6 {
                settings.tone.curves.parametric.lights = 25.0;
            }
            if change == 7 {
                settings.color.vibrance = 30.0;
            }
            if change == 8 {
                settings.effects.vignette.amount = -30.0;
                settings.effects.grain.amount = 20.0;
            }
            let rect = PixelRect::full(image.level_extent(level));
            let a = r.render_region(&image, &settings, level, rect).unwrap();
            let b = cpu.render_region(&image, &settings, level, rect).unwrap();
            let again = r.render_region(&image, &settings, level, rect).unwrap();
            assert_eq!(a.len(), b.len());
            assert_eq!(a.len(), again.len());
            for ((a, b), again) in a.iter().zip(&b).zip(&again) {
                assert_eq!(a.coord(), b.coord());
                assert_eq!(a.layout(), b.layout());
                let max = a
                    .samples::<u8>()
                    .unwrap()
                    .iter()
                    .zip(b.samples::<u8>().unwrap())
                    .map(|(a, b)| a.abs_diff(*b))
                    .max()
                    .unwrap();
                assert!(
                    max <= 2,
                    "level {level}, change {change}, display error {max}"
                );
                assert_eq!(a.samples::<u8>().unwrap(), again.samples::<u8>().unwrap());
            }
            cpu.cache().clear();
            let a = r
                .render_region_as(&image, &settings, level, rect, RenderOutput::SceneLinear)
                .unwrap();
            let b = cpu
                .render_region_as(&image, &settings, level, rect, RenderOutput::SceneLinear)
                .unwrap();
            for (a, b) in a.iter().zip(&b) {
                let max = a
                    .samples::<f32>()
                    .unwrap()
                    .iter()
                    .zip(b.samples::<f32>().unwrap())
                    .map(|(a, b)| (a - b).abs())
                    .fold(0.0, f32::max);
                assert!(
                    max <= 0.005,
                    "level {level}, change {change}, linear error {max}"
                );
            }
        }
    }
}

#[test]
fn preview_matrices_run_only_at_output_resolution() {
    let model = Arc::new(Model::default());
    let r = Renderer::with_ops(
        model.clone(),
        Arc::new(TileCache::new(0)),
        RendererConfig::default(),
    );
    let image = common::synthetic(12345, 700, 533, common::RGGB, [3, 5, 690, 521]);
    let extent = image.level_extent(2);
    r.render_region(
        &image,
        &DevelopSettings::default(),
        2,
        PixelRect::full(extent),
    )
    .unwrap();
    assert_eq!(
        model
            .matrix_pixels
            .load(std::sync::atomic::Ordering::Relaxed),
        2 * extent.area()
    );
    let pixels = || {
        model
            .detail_pixels
            .load(std::sync::atomic::Ordering::Relaxed)
    };
    assert_eq!(
        pixels(),
        extent.area(),
        "default Detail runs at preview resolution"
    );
    let mut settings = DevelopSettings::default();
    settings.tone.exposure = 0.3;
    r.render_region(&image, &settings, 2, PixelRect::full(extent))
        .unwrap();
    assert_eq!(pixels(), extent.area(), "tone reuses Detail");
    settings.white_balance.mode = WhiteBalanceMode::Daylight;
    r.render_region(&image, &settings, 2, PixelRect::full(extent))
        .unwrap();
    assert_eq!(pixels(), 2 * extent.area(), "WB invalidates Detail");
    assert_eq!(
        model
            .matrix_pixels
            .load(std::sync::atomic::Ordering::Relaxed),
        4 * extent.area()
    );
    settings.detail.sharpening.amount = 80.0;
    r.render_region(&image, &settings, 2, PixelRect::full(extent))
        .unwrap();
    assert_eq!(
        pixels(),
        3 * extent.area(),
        "Detail edits invalidate only Detail"
    );
    assert_eq!(
        model
            .matrix_pixels
            .load(std::sync::atomic::Ordering::Relaxed),
        4 * extent.area()
    );
}

#[test]
fn disabled_detail_still_validates_controls() {
    let r = Renderer::with_ops(
        Arc::new(Model::default()),
        Arc::new(TileCache::new(0)),
        RendererConfig::default(),
    );
    let image = common::synthetic(12346, 41, 39, common::RGGB, [1, 1, 39, 37]);
    let mut settings = DevelopSettings::default();
    settings.detail.sharpening.amount = -1.0;
    settings.detail.noise_reduction.color = 0.0;
    assert!(
        r.render_region(&image, &settings, 2, PixelRect::full(image.level_extent(2)))
            .is_err()
    );
}
