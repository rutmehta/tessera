use super::*;
use crate::resident::{DisplayHistogram, ResidentBatch, ResidentOutput, SurfaceTarget};

impl Renderer {
    /// Whether this backend can develop this image/recipe without host pixel
    /// barriers. This is a capability query, not a frame-time guarantee.
    pub fn can_render_resident(
        &self,
        image: &RawImage,
        settings: &DevelopSettings,
    ) -> EngineResult<bool> {
        pipeline_cpu::validate_settings(settings)?;
        Ok(self.supports_resident(&self.resolve(image, settings)?))
    }

    pub(super) fn supports_resident(&self, r: &Resolved<'_>) -> bool {
        let s = r.settings;
        matches!(r.cfa, CfaLayout::Bayer(_) | CfaLayout::XTrans(_))
            && self.ops.begin_resident().is_some()
            // Local adjustment operators/rasterization use the whole-image
            // nonresident path until all local kernels are resident-capable.
            && s.locals.adjustments.is_empty()
            && s.geometry == Default::default()
            && s.tone.texture == 0.0
            && s.tone.clarity == 0.0
            && s.tone.dehaze == 0.0
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
        if level > MAX_LEVEL || !self.supports_resident(&r) {
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
        if level > MAX_LEVEL || !self.supports_resident(&r) {
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
        let cache_dem = self.config.graph.node(StageId::Demosaic).cacheable;
        let cache_wb = self.config.graph.node(StageId::WhiteBalance).cacheable;
        let cache_detail = self.config.graph.node(StageId::Detail).cacheable;
        let halo = pipeline_cpu::detail_halo(&r.settings.detail);
        let mut detailed = HashMap::new();
        let mut needed = BTreeSet::new();
        for &c in coords {
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
        let mut balanced = HashMap::new();
        let mut finished = Vec::with_capacity(coords.len());
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
                                cache(&mut *batch, key(StageId::Demosaic, d), &t)?
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
                    if cache_dem {
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
                    cache(&mut *batch, key(StageId::WhiteBalance, c), &t)?
                } else {
                    t
                }
            };
            balanced.insert(c, t);
        }
        for &c in coords {
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
            let mut chain = vec![
                Op::Tone(&r.settings.tone),
                Op::ToneExtra(&r.settings.tone),
                Op::Color(&r.settings.color),
                Op::EffectsInCrop(
                    &r.settings.effects,
                    r.image.level_extent(c.level),
                    &r.settings.geometry.crop,
                ),
            ];
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
