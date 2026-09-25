use super::*;
use crate::resident::{DisplayHistogram, ResidentBatch, ResidentOutput, SurfaceTarget};

impl Renderer {
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
        if level > MAX_LEVEL || has_m2_settings(settings) || !matches!(r.cfa, CfaLayout::Bayer(_)) {
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
        if level > MAX_LEVEL || has_m2_settings(settings) || !matches!(r.cfa, CfaLayout::Bayer(_)) {
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
        let cache_dem = self.config.graph.node(StageId::Demosaic).cacheable;
        let cache_wb = self.config.graph.node(StageId::WhiteBalance).cacheable;
        let mut finished = Vec::with_capacity(coords.len());
        for &c in coords {
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
                                batch.cache(key(StageId::Demosaic, d), &t)?
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
                        batch.cache(level_key, &t)?
                    } else {
                        t
                    }
                };
                // Both matrices are linear: transform only the requested level.
                let t = batch.run(&Op::Matrix(r.profile), &t)?;
                let t = batch.run(&Op::Matrix(r.wb), &t)?;
                if cache_wb {
                    batch.cache(key(StageId::WhiteBalance, c), &t)?
                } else {
                    t
                }
            };
            let t = if output == RenderOutput::Display {
                batch.run_chain(
                    &[
                        Op::Tone(&r.settings.tone),
                        Op::Display {
                            gamut: r.settings.output.gamut_mapping,
                        },
                    ],
                    &t,
                )?
            } else {
                batch.run(&Op::Tone(&r.settings.tone), &t)?
            };
            finished.push(t);
        }
        batch.finish(finished, output == RenderOutput::Display, surface, cancel)
    }
}
