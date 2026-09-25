use super::*;
use crate::resident::{
    DisplayHistogram, LocalToneOptions, ResidentBatch, ResidentOutput, SurfaceTarget,
};
use engine_api::recipe::settings::ToneSettings;

/// Whole-level WB padding: the largest `pipeline_cpu::detail_halo`.
const LEVEL_PAD: u16 = pipeline_cpu::DETAIL_HALO;

pub(super) fn has_presence(s: &ToneSettings) -> bool {
    s.texture != 0.0 || s.clarity != 0.0 || s.dehaze != 0.0
}

/// Dehaze statistics depend on everything upstream of Dehaze, never on the
/// Dehaze amount or the curves applied after it.
fn dehaze_statistics_key(r: &Resolved<'_>, level: u8) -> engine_api::stage::MemoKey {
    let upstream = ToneSettings {
        dehaze: 0.0,
        curves: Default::default(),
        ..r.settings.tone.clone()
    };
    let frame = r.image.level_extent(level);
    let mut key = PipelineGraph::memo_key(
        r.image.id(),
        &r.chain,
        StageId::Detail,
        TileCoord::new(level, 0, 0),
    );
    key.stage = StageId::Tone;
    key.params_hash = ParamHash::chain(
        key.params_hash,
        ParamHash::of(
            StageId::Tone,
            &(&upstream, frame.width, frame.height, "dehaze-statistics-v1"),
        ),
    );
    key
}

impl Renderer {
    /// Whether this backend can develop this image/recipe without host pixel
    /// barriers. This is a capability query, not a frame-time guarantee.
    pub fn can_render_resident(
        &self,
        image: &RawImage,
        settings: &DevelopSettings,
    ) -> EngineResult<bool> {
        self.validate_settings(settings)?;
        Ok(self.supports_resident(&self.resolve(image, settings)?, None))
    }

    /// `level`: the level to render, when known. Texture/Clarity/Dehaze need a
    /// backend whole-level barrier that fits that level (any level when None).
    pub(super) fn supports_resident(&self, r: &Resolved<'_>, level: Option<u8>) -> bool {
        let s = r.settings;
        if pipeline_cpu::denoise_active(&s.denoise)
            || !matches!(r.cfa, CfaLayout::Bayer(_) | CfaLayout::XTrans(_))
            // Local adjustment operators/rasterization use the whole-image
            // nonresident path until all local kernels are resident-capable.
            || !s.locals.adjustments.is_empty()
            || s.geometry != Default::default()
        {
            return false;
        }
        let Some(batch) = self.ops.begin_resident() else {
            return false;
        };
        if !has_presence(&s.tone) {
            return true;
        }
        let frame = r.image.level_extent(level.unwrap_or(0).min(MAX_LEVEL));
        // Without a known level, probe the smallest frame: capability only.
        let frame = if level.is_some() {
            frame
        } else {
            Extent::new(1, 1)
        };
        batch.supports_local_tone(frame)
    }
    /// Direct display render to an RGBA8 IOSurface. Returns false if the
    /// backend/settings require the conventional CPU tile delivery path.
    pub fn render_to_surface(
        &self,
        image: &RawImage,
        settings: &DevelopSettings,
        level: u8,
        surface: u32,
        cancel: &CancellationToken,
    ) -> EngineResult<bool> {
        cancel.check()?;
        let r = self.resolve(image, settings)?;
        if level > MAX_LEVEL || !self.supports_resident(&r, Some(level)) {
            return Ok(false);
        }
        let Some(batch) = self.ops.begin_resident() else {
            return Ok(false);
        };
        let coords = Self::tiles_for(image, level, PixelRect::full(image.level_extent(level)));
        self.run_resident(
            &r,
            &coords,
            RenderOutput::Display,
            cancel,
            batch,
            Some(SurfaceTarget {
                id: surface,
                histogram: false,
            }),
        )?;
        Ok(true)
    }

    /// Direct surface presentation with only a 4 KiB histogram readback.
    pub fn render_surface(
        &self,
        image: &RawImage,
        settings: &DevelopSettings,
        level: u8,
        surface: u32,
        cancel: &CancellationToken,
    ) -> EngineResult<Option<DisplayHistogram>> {
        cancel.check()?;
        let r = self.resolve(image, settings)?;
        if level > MAX_LEVEL || !self.supports_resident(&r, Some(level)) {
            return Ok(None);
        }
        let Some(batch) = self.ops.begin_resident() else {
            return Ok(None);
        };
        let coords = Self::tiles_for(image, level, PixelRect::full(image.level_extent(level)));
        self.run_resident(
            &r,
            &coords,
            RenderOutput::Display,
            cancel,
            batch,
            Some(SurfaceTarget {
                id: surface,
                histogram: true,
            }),
        )
        .and_then(|out| {
            out.histogram
                .map(Some)
                .ok_or_else(|| engine_api::EngineError::internal("resident histogram missing"))
        })
    }

    /// Scene-linear WB tiles of the output level: memoized WB, the extra
    /// output-level demosaic checkpoint, or the streamed sensor chain.
    fn balanced_tiles(
        &self,
        r: &Resolved<'_>,
        needed: impl IntoIterator<Item = TileCoord>,
        batch: &mut dyn ResidentBatch,
        cancel: &CancellationToken,
    ) -> EngineResult<HashMap<TileCoord, crate::resident::ResidentTile>> {
        let key = |stage, c| PipelineGraph::memo_key(r.image.id(), &r.chain, stage, c);
        let cache = |batch: &mut dyn ResidentBatch, key, tile: &crate::resident::ResidentTile| {
            batch.cache_exact(key, tile)
        };
        let cache_dem = self.config.graph.node(StageId::Demosaic).cacheable;
        let cache_wb = self.config.graph.node(StageId::WhiteBalance).cacheable;
        let mut balanced = HashMap::new();
        for c in needed {
            cancel.check()?;
            let t = if cache_wb && let Some(t) = batch.cached(&key(StageId::WhiteBalance, c))? {
                t
            } else {
                // Domain-separate output-frame demosaic keys from sensor-frame
                // level-zero tiles, including a crop offset at level zero.
                let mut level_key = key(StageId::Demosaic, c);
                level_key.params_hash = ParamHash::chain(
                    level_key.params_hash,
                    ParamHash::of(StageId::Demosaic, &("output-demosaic-v1", r.crop)),
                );
                let t = if cache_dem && let Some(t) = batch.cached(&level_key)? {
                    t
                } else {
                    let mut sampled = None;
                    // Stream sensor dependencies; no full-frame f32 map. Each
                    // chunk can reuse earlier scratch buffers in this encoder.
                    for sources in resample_sources(r.crop, c).chunks(16) {
                        cancel.check()?;
                        let mut dem = HashMap::new();
                        let mut missing = Vec::new();
                        for &d in sources {
                            if cache_dem
                                && let Some(t) = batch.cached(&key(StageId::Demosaic, d))?
                            {
                                dem.insert(d, t);
                            } else {
                                missing.push(d);
                            }
                        }
                        let need_lin: BTreeSet<_> = missing
                            .iter()
                            .flat_map(|&d| gather_sources(r.sensor, d, r.dem_halo, r.period))
                            .collect();
                        let need_raw: BTreeSet<_> = need_lin
                            .iter()
                            .flat_map(|&l| gather_sources(r.sensor, l, r.lin_halo, r.period))
                            .collect();
                        let mut raw = HashMap::new();
                        for d in need_raw {
                            cancel.check()?;
                            let k = key(StageId::Decode, d);
                            let t = if let Some(t) = batch.cached(&k)? {
                                t
                            } else {
                                let t =
                                    engine_api::tile::Pyramid::tile(r.image.cfa().pyramid(), d)?;
                                batch.upload_cached(k, &t)?
                            };
                            raw.insert(d, t);
                        }
                        let mut linear = HashMap::new();
                        for d in need_lin {
                            cancel.check()?;
                            let t = batch.gather(r.sensor, d, r.lin_halo, r.period, &raw)?;
                            let t = batch.run(
                                &Op::Highlights {
                                    cfa: r.cfa,
                                    mode: r.highlights,
                                },
                                &t,
                            )?;
                            linear.insert(d, t);
                        }
                        drop(raw);
                        for d in missing {
                            cancel.check()?;
                            let t = batch.gather(r.sensor, d, r.dem_halo, r.period, &linear)?;
                            let t = batch.run(
                                &Op::Demosaic {
                                    cfa: r.cfa,
                                    algorithm: r.algorithm,
                                },
                                &t,
                            )?;
                            let t = if cache_dem {
                                cache(batch, key(StageId::Demosaic, d), &t)?
                            } else {
                                t
                            };
                            dem.insert(d, t);
                        }
                        drop(linear);
                        sampled = Some(batch.resample(r.crop, c, &dem, sampled)?);
                    }
                    let t = sampled
                        .ok_or_else(|| engine_api::EngineError::internal("no resample sources"))?;
                    // At level 0 this checkpoint is a crop of the memoized
                    // sensor demosaic: retaining it duplicates a whole frame and
                    // evicted WB/Detail tiles on large images (per-edit re-decode).
                    if cache_dem && c.level > 0 {
                        // This is an extra checkpoint not present in the scalar
                        // graph. Do not add a second f16 rounding before WB.
                        batch.cache_exact(level_key, &t)?
                    } else {
                        t
                    }
                };
                // Both matrices are linear: transform only the requested level.
                let t = batch.run(&Op::Matrix(r.profile), &t)?;
                let t = batch.run(&Op::Matrix(r.wb), &t)?;
                if cache_wb {
                    cache(batch, key(StageId::WhiteBalance, c), &t)?
                } else {
                    t
                }
            };
            balanced.insert(c, t);
        }
        Ok(balanced)
    }

    /// Whole-level resident develop: one level-sized tile per stage instead
    /// of one dispatch per pyramid tile. The WB level (padded by the largest
    /// Detail halo, edges replicated like `gather`) and the developed level
    /// are memoized, so point-stage edits cost one fused pass plus output.
    #[allow(clippy::too_many_arguments)]
    fn run_resident_level(
        &self,
        r: &Resolved<'_>,
        all: &[TileCoord],
        coords: &[TileCoord],
        output: RenderOutput,
        cancel: &CancellationToken,
        mut batch: Box<dyn ResidentBatch + '_>,
        surface: Option<SurfaceTarget>,
    ) -> EngineResult<ResidentOutput> {
        let level = all[0].level;
        let frame = r.image.level_extent(level);
        let lc = TileCoord::new(level, 0, 0);
        let level_key = |stage, pad: u16| {
            let mut k = PipelineGraph::memo_key(r.image.id(), &r.chain, stage, lc);
            k.params_hash = ParamHash::chain(
                k.params_hash,
                ParamHash::of(
                    stage,
                    &("resident-level-v1", pad, frame.width, frame.height),
                ),
            );
            k
        };
        let cache_wb = self.config.graph.node(StageId::WhiteBalance).cacheable;
        let cache_detail = self.config.graph.node(StageId::Detail).cacheable;
        let detail_key = level_key(StageId::Detail, 0);
        let developed = if cache_detail && let Some(t) = batch.cached(&detail_key)? {
            t
        } else {
            let wb_key = level_key(StageId::WhiteBalance, LEVEL_PAD);
            let padded = if cache_wb && let Some(t) = batch.cached(&wb_key)? {
                t
            } else {
                let tiles = self.balanced_tiles(r, all.iter().copied(), &mut *batch, cancel)?;
                cancel.check()?;
                let t = batch.gather_level(frame, lc, LEVEL_PAD, &tiles)?;
                if cache_wb {
                    batch.cache_exact(wb_key, &t)?
                } else {
                    t
                }
            };
            // Detail consumes the padded neighbours and returns the interior.
            let t = batch.run(&Op::Detail(&r.settings.detail), &padded)?;
            if cache_detail {
                batch.cache_exact(detail_key, &t)?
            } else {
                t
            }
        };
        cancel.check()?;
        let curves_only = ToneSettings {
            texture: 0.0,
            clarity: 0.0,
            dehaze: 0.0,
            ..r.settings.tone.clone()
        };
        let mut t = developed;
        let mut chain = if has_presence(&r.settings.tone) {
            let toned = batch.run(&Op::Tone(&r.settings.tone), &t)?;
            let options = LocalToneOptions {
                preview: level > 0 && self.config.preview_approximations,
                statistics_key: dehaze_statistics_key(r, level),
            };
            let input = HashMap::from([(lc, toned)]);
            t = batch
                .local_tone(&r.settings.tone, frame, &input, &[lc], &options)?
                .remove(&lc)
                .ok_or_else(|| engine_api::EngineError::internal("local tone output missing"))?;
            vec![Op::ToneExtra(&curves_only)]
        } else {
            vec![Op::Tone(&r.settings.tone), Op::ToneExtra(&r.settings.tone)]
        };
        chain.extend([
            Op::Color(&r.settings.color),
            Op::EffectsInCrop(&r.settings.effects, frame, &r.settings.geometry.crop),
        ]);
        if output == RenderOutput::Display {
            chain.push(Op::Display {
                gamut: r.settings.output.gamut_mapping,
            });
        }
        // Previews develop in their own pixel domain (grain scale), as below.
        t.coord.level = 0;
        let mut t = batch.run_chain(&chain, &t)?;
        t.coord = lc;
        cancel.check()?;
        let finished = if surface.is_some() {
            vec![t]
        } else {
            coords
                .iter()
                .map(|&c| {
                    let (x, y) = c.pixel_origin(TILE_SIZE);
                    let extent = Extent::new(
                        (frame.width - x).min(TILE_SIZE),
                        (frame.height - y).min(TILE_SIZE),
                    );
                    batch.crop(&t, c, (x, y), extent)
                })
                .collect::<EngineResult<Vec<_>>>()?
        };
        batch.finish(finished, output == RenderOutput::Display, surface, cancel)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn run_resident(
        &self,
        r: &Resolved<'_>,
        coords: &[TileCoord],
        output: RenderOutput,
        cancel: &CancellationToken,
        mut batch: Box<dyn ResidentBatch + '_>,
        surface: Option<SurfaceTarget>,
    ) -> EngineResult<ResidentOutput> {
        let key = |stage, c| PipelineGraph::memo_key(r.image.id(), &r.chain, stage, c);
        // Creative curves/color can amplify f16 checkpoint error beyond the
        // full-chain tolerance. Retain f32 checkpoints for every recipe so a
        // later creative edit cannot reuse lower-precision neutral entries.
        // Their full payload still counts against the configured cache budget.
        let cache = |batch: &mut dyn ResidentBatch, key, tile: &crate::resident::ResidentTile| {
            batch.cache_exact(key, tile)
        };
        if let Some(first) = coords.first() {
            let frame = r.image.level_extent(first.level);
            let all = Self::tiles_for(r.image, first.level, PixelRect::full(frame));
            // `coords` are unique tiles of one level: whole-level requests
            // (every surface frame) run as one level-sized tile.
            if coords.len() == all.len() && batch.supports_level(frame, LEVEL_PAD) {
                return self.run_resident_level(r, &all, coords, output, cancel, batch, surface);
            }
        }
        let cache_detail = self.config.graph.node(StageId::Detail).cacheable;
        let halo = pipeline_cpu::detail_halo(&r.settings.detail);
        let presence = has_presence(&r.settings.tone);
        // Texture/Clarity/Dehaze read neighbourhoods and global statistics of
        // the whole developed level, like the scalar image-level barrier.
        let work: Vec<TileCoord> = match coords.first() {
            Some(first) if presence => Self::tiles_for(
                r.image,
                first.level,
                PixelRect::full(r.image.level_extent(first.level)),
            ),
            _ => coords.to_vec(),
        };
        let mut detailed = HashMap::new();
        let mut needed = BTreeSet::new();
        for &c in &work {
            if halo > 0
                && cache_detail
                && let Some(t) = batch.cached(&key(StageId::Detail, c))?
            {
                detailed.insert(c, t);
            } else if halo > 0 {
                needed.extend(gather_sources(r.image.level_extent(c.level), c, halo, 1));
            } else {
                needed.insert(c);
            }
        }
        let balanced = self.balanced_tiles(r, needed, &mut *batch, cancel)?;
        let mut finished = Vec::with_capacity(coords.len());
        let mut developed = HashMap::new();
        for &c in &work {
            cancel.check()?;
            let t = if let Some(t) = detailed.remove(&c) {
                t
            } else if halo > 0 {
                let t = batch.gather(r.image.level_extent(c.level), c, halo, 1, &balanced)?;
                let t = batch.run(&Op::Detail(&r.settings.detail), &t)?;
                if cache_detail {
                    cache(&mut *batch, key(StageId::Detail, c), &t)?
                } else {
                    t
                }
            } else {
                // Disabled Detail still validates all controls, just as the
                // scalar reference does (zero halo is not a validation bypass).
                batch.run(&Op::Detail(&r.settings.detail), &balanced[&c])?
            };
            developed.insert(c, t);
        }
        drop(balanced);
        let curves_only = ToneSettings {
            texture: 0.0,
            clarity: 0.0,
            dehaze: 0.0,
            ..r.settings.tone.clone()
        };
        if presence && let Some(first) = coords.first() {
            let level = first.level;
            let mut toned = HashMap::with_capacity(developed.len());
            for (c, t) in developed.drain() {
                cancel.check()?;
                toned.insert(c, batch.run(&Op::Tone(&r.settings.tone), &t)?);
            }
            let options = LocalToneOptions {
                preview: level > 0 && self.config.preview_approximations,
                statistics_key: dehaze_statistics_key(r, level),
            };
            let frame = r.image.level_extent(level);
            developed = batch.local_tone(&r.settings.tone, frame, &toned, coords, &options)?;
        }
        for &c in coords {
            cancel.check()?;
            let t = developed
                .remove(&c)
                .ok_or_else(|| engine_api::EngineError::internal("developed tile missing"))?;
            let mut chain = if presence {
                vec![Op::ToneExtra(&curves_only)]
            } else {
                vec![Op::Tone(&r.settings.tone), Op::ToneExtra(&r.settings.tone)]
            };
            chain.extend([
                Op::Color(&r.settings.color),
                Op::EffectsInCrop(
                    &r.settings.effects,
                    r.image.level_extent(c.level),
                    &r.settings.geometry.crop,
                ),
            ]);
            if output == RenderOutput::Display {
                chain.push(Op::Display {
                    gamut: r.settings.output.gamut_mapping,
                });
            }
            // The whole-image reference develops previews in their own pixel
            // domain (including grain scale), not the sensor-resolution domain.
            let mut t = t;
            t.coord.level = 0;
            let mut t = batch.run_chain(&chain, &t)?;
            t.coord = c;
            finished.push(t);
        }
        batch.finish(finished, output == RenderOutput::Display, surface, cancel)
    }
}
