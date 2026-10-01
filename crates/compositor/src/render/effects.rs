//! CPU-only whole-source barrier for neighbourhood effects. No tile-edge halos
//! are invented: masks and nested composites are evaluated at level zero first.
use std::sync::Arc;

use engine_api::tile::{TILE_SIZE, TileCoord};
use engine_api::{EngineError, EngineResult};

use super::exec::{FrameKind, Op, Src, TileJob};
use super::pixel::Params;
use super::{Compositor, DocRef, Part, styles};
use crate::blend::BlendMode;

use crate::document::{DocState, GroupMode, Knockout, Layer, LayerKind, LayerProps};
use crate::geom::next_doc_key;
use crate::raster::{Depth, Raster, load_normalized};

pub(crate) struct StyledTile {
    source: Vec<f32>,
    planes: Vec<EffectTile>,
}
struct EffectTile {
    samples: Vec<f32>,
    mode: BlendMode,
    opacity: f32,
    outside: bool,
    stroke: bool,
}

pub(super) fn has_styles(state: &DocState) -> bool {
    state.has_layer_styles()
}

impl Compositor {
    pub(super) fn source_raster(&self, doc: DocRef<'_>) -> EngineResult<Raster> {
        #[cfg(test)]
        self.stats
            .source_raster_build_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let e = doc.state.canvas;
        let mut raster = Raster::new(e, 4, Depth::F32, 0.0);
        let (nx, ny) = e.tile_grid(TILE_SIZE);
        for y in 0..ny {
            for x in 0..nx {
                super::smart_filters::check_render_cancel(doc.cancel)?;
                let coord = TileCoord::new(0, x, y);
                let tile = super::unpremultiply(&self.composite_premult(doc, coord)?)?;
                raster.set_slot(x, y, Some(tile), doc.state.rev)?;
            }
        }
        super::smart_filters::check_render_cancel(doc.cancel)?;
        Ok(raster)
    }

    fn effect_samples(&self, raster: &Raster, coord: TileCoord) -> EngineResult<Vec<f32>> {
        let tile = self
            .raster_level(next_doc_key(), 0, Part::Content, raster, coord)?
            .ok_or_else(|| EngineError::internal("missing effect tile"))?;
        let mut samples = vec![0.0; tile.layout().plane_len() * 4];
        load_normalized(&tile, &mut samples)?;
        Ok(samples)
    }
}

impl<'a> TileJob<'a> {
    pub(super) fn emit_styles(
        &self,
        layer: &'a Layer,
        params: Params,
        ops: &mut Vec<Op<'a>>,
    ) -> EngineResult<()> {
        if matches!(
            layer.kind,
            LayerKind::Adjustment(_)
                | LayerKind::Group {
                    mode: GroupMode::PassThrough,
                    ..
                }
        ) {
            return Err(EngineError::Unsupported {
                what: "styles on adjustment/pass-through layers require isolation".into(),
            });
        }
        let mut source = layer.clone();
        source.props = LayerProps::default();
        let mut state = self.doc.state.clone();
        state.depth = Depth::F32;
        state.root = vec![Arc::new(source)];
        let raster = self.comp.source_raster(DocRef {
            state: &state,
            key: next_doc_key(),
            pass: self.doc.pass,
            cancel: self.doc.cancel,
        })?;
        #[cfg(test)]
        self.comp
            .stats
            .style_render_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let planes = styles::render(&raster, &layer.props.styles, self.doc.state.global_light)?;
        let source = self.comp.effect_samples(&raster, self.coord)?;
        let planes = planes
            .iter()
            .map(|plane| -> EngineResult<EffectTile> {
                Ok(EffectTile {
                    samples: self.comp.effect_samples(&plane.raster, self.coord)?,
                    mode: plane.mode,
                    opacity: plane.opacity,
                    outside: plane.outside,
                    stroke: plane.stroke,
                })
            })
            .collect::<EngineResult<Vec<_>>>()?;
        ops.push(Op::Blend {
            layer,
            src: Src::Styled(StyledTile { source, planes }),
            params,
            mask: false,
        });
        Ok(())
    }

    pub(super) fn run_styles(
        &self,
        frames: &mut [(FrameKind, Vec<f32>)],
        deep: Option<&[f32]>,
        styled: &StyledTile,
        params: &Params,
    ) -> EngineResult<()> {
        let top = frames
            .len()
            .checked_sub(1)
            .ok_or_else(|| EngineError::internal("empty style frame"))?;
        let before = frames[top].1.clone();
        let effect_params = |plane: &EffectTile| Params {
            mode: plane.mode,
            opacity: plane.opacity,
            fill: 1.0,
            knockout: Knockout::None,
            ..*params
        };
        for plane in styled.planes.iter().filter(|p| p.outside && !p.stroke) {
            self.blend_top(frames, deep, &plane.samples, &effect_params(plane));
        }
        let interior_before = frames[top].1.clone();
        let shape = &styled.source[3 * self.n..];
        let mut source = styled.source.clone();
        for (a, s) in source[3 * self.n..].iter_mut().zip(shape) {
            *a = if *s > 0.0 { 1.0 } else { 0.0 };
        }
        self.blend_top(
            frames,
            deep,
            &source,
            &Params {
                opacity: 1.0,
                ..*params
            },
        );
        // Evaluate the interior at unit shape coverage, then apply the shape
        // once. Repeated source-over of antialiased alpha would fatten edges.
        for plane in styled.planes.iter().filter(|p| !p.outside && !p.stroke) {
            source.copy_from_slice(&plane.samples);
            for (a, s) in source[3 * self.n..].iter_mut().zip(shape) {
                *a = if *s > 0.0 {
                    (*a / *s).clamp(0.0, 1.0)
                } else {
                    0.0
                };
            }
            self.blend_top(frames, deep, &source, &effect_params(plane));
        }
        let r = self.region;
        for y in r.y0..r.y1 {
            for (i, s) in shape
                .iter()
                .enumerate()
                .take(y * self.w + r.x1)
                .skip(y * self.w + r.x0)
            {
                for c in 0..4 {
                    let j = c * self.n + i;
                    frames[top].1[j] =
                        interior_before[j] + s * (frames[top].1[j] - interior_before[j]);
                }
            }
        }
        for plane in styled.planes.iter().filter(|p| p.stroke) {
            self.blend_top(frames, deep, &plane.samples, &effect_params(plane));
        }
        for y in r.y0..r.y1 {
            for i in y * self.w + r.x0..y * self.w + r.x1 {
                for c in 0..4 {
                    let j = c * self.n + i;
                    frames[top].1[j] = before[j] + params.opacity * (frames[top].1[j] - before[j]);
                }
            }
        }
        self.comp.stats.bump_blend();
        Ok(())
    }
}

#[cfg(test)]
mod perf1_tests {
    use super::*;
    use crate::Document;
    use engine_api::tile::Extent;
    use std::sync::atomic::Ordering;

    fn styled_document(effects: Vec<styles::StyleEffect>, hide_fill: bool) -> Document {
        // Six output tiles, including partial right/bottom tiles. No fonts,
        // files, providers, wall-clock assertions or global test counters.
        let extent = Extent::new(513, 259);
        let mut raster = Raster::new(extent, 4, Depth::F32, 0.0);
        raster
            .edit_region(crate::geom::Rect::of_extent(extent), 1, |x, y, p| {
                let inside = (17..497).contains(&x) && (11..248).contains(&y);
                *p = if inside {
                    [0.125, 0.25, 0.5, 1.0]
                } else {
                    [0.0; 4]
                };
            })
            .unwrap();
        let mut layer = Layer::new("synthetic styled raster", LayerKind::Pixel(raster));
        layer.props.styles.effects = effects;
        if hide_fill {
            layer.props.fill_opacity = 0.0;
        }
        let mut state = DocState::new(extent, Depth::F32);
        state.root.push(Arc::new(layer));
        Document::new(state)
    }

    #[test]
    fn full_level_evaluates_each_styled_source_once() {
        let doc = styled_document(
            vec![
                styles::StyleEffect::DropShadow(styles::Shadow {
                    size: 2.0,
                    distance: 3.0,
                    ..Default::default()
                }),
                styles::StyleEffect::OuterGlow(styles::Glow {
                    size: 1.0,
                    ..Default::default()
                }),
            ],
            false,
        );
        let compositor = Compositor::new(8 << 20);
        let (extent, pixels) = compositor.render_level_rgba(&doc, 0).unwrap();
        assert_eq!(extent, Extent::new(513, 259));
        assert_eq!(pixels.len(), 513 * 259 * 4);
        assert!(pixels.iter().all(|v| v.is_finite()));
        // Expected RED on the unoptimized renderer: emit_styles computes the
        // full source and effects for every output tile. This assertion is a
        // work invariant, not a claim that a failure has already been observed.
        assert_eq!(
            compositor
                .stats
                .source_raster_build_calls
                .load(Ordering::Relaxed),
            1,
            "one immutable styled source raster must be built once per full-level pass"
        );
        assert_eq!(
            compositor.stats.style_render_calls.load(Ordering::Relaxed),
            1,
            "one immutable style stack must be rendered once per full-level pass"
        );
    }

    #[test]
    fn multitile_overlay_matches_independent_exact_pixel_fixture() {
        let doc = styled_document(
            vec![styles::StyleEffect::ColorOverlay(styles::Overlay {
                fill: crate::document::Fill::Solid {
                    color: [0.25, 0.5, 0.75],
                },
                opacity: 1.0,
                ..Default::default()
            })],
            true,
        );
        let (_, pixels) = Compositor::new(8 << 20).render_level_rgba(&doc, 0).unwrap();
        assert_eq!(pixels.len(), 513 * 259 * 4);
        // Independent analytic oracle: zero source fill; opaque normal overlay
        // on a binary alpha rectangle. Dyadic colors are exactly representable.
        // This protects basic source/plane extraction at tile seams; it does
        // not substitute for the pending frozen blur/morphology oracle.
        for y in 0..259usize {
            for x in 0..513usize {
                let expected: [f32; 4] = if (17..497).contains(&x) && (11..248).contains(&y) {
                    [0.25, 0.5, 0.75, 1.0]
                } else {
                    [0.0; 4]
                };
                let start = (y * 513 + x) * 4;
                for channel in 0..4 {
                    assert_eq!(
                        pixels[start + channel].to_bits(),
                        expected[channel].to_bits(),
                        "pixel ({x},{y}) channel {channel}"
                    );
                }
            }
        }
    }
}
