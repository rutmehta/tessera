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

    fn effect_samples(
        &self,
        key: u64,
        raster: &Raster,
        coord: TileCoord,
    ) -> EngineResult<Vec<f32>> {
        let tile = self
            .raster_level(key, 0, Part::Content, raster, coord)?
            .ok_or_else(|| EngineError::internal("missing effect tile"))?;
        let mut samples = vec![0.0; tile.layout().plane_len() * 4];
        load_normalized(&tile, &mut samples)?;
        Ok(samples)
    }
}

impl Compositor {
    fn style_entry(
        &self,
        doc: DocRef<'_>,
        layer: &Layer,
    ) -> EngineResult<Arc<super::style_pass::Entry>> {
        use super::smart_filters::check_render_cancel;
        use super::style_pass::{Entry, predicted_bytes};
        let build = |admitted| {
            // Declined parents use uncached subtrees: their isolated keys are
            // short-lived, so retaining children would consume budget without reuse.
            let child_pass = if admitted { doc.style_pass } else { None };
            let mut source = layer.clone();
            source.props = LayerProps::default();
            let mut state = doc.state.clone();
            state.depth = Depth::F32;
            state.root = vec![Arc::new(source)];
            // One unique source namespace per miss, stable across all source
            // tiles and nested styles. No cache lock is held during rendering.
            let raster = self.source_raster(DocRef {
                state: &state,
                key: next_doc_key(),
                pass: doc.pass,
                style_pass: child_pass,
                cancel: doc.cancel,
            })?;
            check_render_cancel(doc.cancel)?;
            #[cfg(test)]
            self.stats
                .style_render_calls
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let planes = styles::render(&raster, &layer.props.styles, doc.state.global_light)?;
            let plane_keys = planes.iter().map(|_| next_doc_key()).collect();
            Ok(Entry {
                source: raster,
                planes,
                source_key: next_doc_key(),
                plane_keys,
            })
        };
        match doc.style_pass {
            Some(pass) => pass.get_or_build(
                (doc.key, doc.state.rev, layer.id.0),
                predicted_bytes(doc.state.canvas, &layer.props.styles),
                doc.cancel,
                build,
            ),
            None => {
                check_render_cancel(doc.cancel)?;
                let entry = build(false)?;
                check_render_cancel(doc.cancel)?;
                Ok(Arc::new(entry))
            }
        }
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
        let entry = self.comp.style_entry(self.doc, layer)?;
        let source = self
            .comp
            .effect_samples(entry.source_key, &entry.source, self.coord)?;
        let planes = entry
            .planes
            .iter()
            .zip(&entry.plane_keys)
            .map(|(plane, &key)| -> EngineResult<EffectTile> {
                Ok(EffectTile {
                    samples: self.comp.effect_samples(key, &plane.raster, self.coord)?,
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
            (
                compositor
                    .stats
                    .source_raster_build_calls
                    .load(Ordering::Relaxed),
                compositor.stats.style_render_calls.load(Ordering::Relaxed)
            ),
            (1, 1),
            "source builds and style renders must each occur once per full-level pass"
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

    fn nested_overlay_document(color: [f32; 3], left: u32) -> Document {
        // Two output tiles and a styled child inside a styled isolated group.
        // Both documents intentionally share persisted IDs/revisions; only
        // Document::new's runtime namespace distinguishes their contexts.
        let extent = Extent::new(257, 3);
        let mut raster = Raster::new(extent, 4, Depth::F32, 0.0);
        raster
            .edit_region(crate::geom::Rect::of_extent(extent), 1, |x, y, p| {
                *p = if x >= left && y == 1 {
                    [1.0; 4]
                } else {
                    [0.0; 4]
                };
            })
            .unwrap();
        let mut child = Layer::new("nested styled child", LayerKind::Pixel(raster));
        child.id = engine_api::id::LayerId(2);
        child.props.fill_opacity = 0.0;
        child.props.styles.effects = vec![styles::StyleEffect::ColorOverlay(styles::Overlay {
            fill: crate::document::Fill::Solid { color },
            ..Default::default()
        })];
        let mut group = Layer::new(
            "styled isolated parent",
            LayerKind::Group {
                mode: GroupMode::Isolated,
                children: vec![Arc::new(child)],
            },
        );
        group.id = engine_api::id::LayerId(1);
        group.props.styles.effects = vec![styles::StyleEffect::ColorOverlay(styles::Overlay {
            fill: crate::document::Fill::Solid {
                color: [0.0, 0.0, 1.0],
            },
            opacity: 0.5,
            ..Default::default()
        })];
        let mut state = DocState::new(extent, Depth::F32);
        state.rev = 7;
        state.root.push(Arc::new(group));
        Document::new(state)
    }

    fn assert_nested_overlay_pixels(pixels: &[f32], left: usize, color: [f32; 3]) {
        assert_eq!(pixels.len(), 257 * 3 * 4);
        // Opaque child overlay followed by half-opacity blue parent overlay.
        // All expected values are dyadic, with transparent black elsewhere.
        for y in 0..3 {
            for x in 0..257 {
                let expected = if x >= left && y == 1 {
                    [color[0] * 0.5, color[1] * 0.5, color[2] * 0.5 + 0.5, 1.0]
                } else {
                    [0.0; 4]
                };
                for c in 0..4 {
                    assert_eq!(
                        pixels[(y * 257 + x) * 4 + c].to_bits(),
                        expected[c].to_bits(),
                        "nested pixel ({x},{y}) channel {c}, left={left}"
                    );
                }
            }
        }
    }

    #[test]
    fn nested_styled_group_matches_independent_overlay_oracle() {
        let doc = nested_overlay_document([1.0, 0.0, 0.0], 1);
        let (_, pixels) = Compositor::new(8 << 20).render_level_rgba(&doc, 0).unwrap();
        assert_nested_overlay_pixels(&pixels, 1, [1.0, 0.0, 0.0]);
    }

    #[test]
    fn nested_styles_do_not_reuse_another_documents_same_ids_and_revision() {
        let first = nested_overlay_document([1.0, 0.0, 0.0], 1);
        let second = nested_overlay_document([0.0, 1.0, 0.0], 128);
        assert_ne!(first.key(), second.key());
        assert_eq!(first.state().rev, second.state().rev);
        assert_eq!(first.state().root[0].id, second.state().root[0].id);
        assert_eq!(
            first.state().root[0].children().unwrap()[0].id,
            second.state().root[0].children().unwrap()[0].id
        );
        let compositor = Compositor::new(8 << 20);
        for (doc, left, color) in [
            (&first, 1, [1.0, 0.0, 0.0]),
            (&second, 128, [0.0, 1.0, 0.0]),
            (&first, 1, [1.0, 0.0, 0.0]),
        ] {
            // Keep the same compositor and its caches across A/B/A. Clearing
            // between documents would conceal an incorrectly shared entry.
            let (_, pixels) = compositor.render_level_rgba(doc, 0).unwrap();
            assert_nested_overlay_pixels(&pixels, left, color);
        }
    }

    #[test]
    fn same_pass_smart_children_keep_identical_ids_in_distinct_contexts() {
        use crate::document::SmartObject;
        use crate::geom::Affine;
        use engine_api::id::LayerId;

        let first = nested_overlay_document([1.0, 0.0, 0.0], 1);
        let second = nested_overlay_document([0.0, 1.0, 0.0], 128);
        let first_child = SmartObject::new(first.state().as_ref().clone(), Affine::IDENTITY);
        let second_child = SmartObject::new(second.state().as_ref().clone(), Affine::IDENTITY);
        // smart_tile constructs child DocRefs from these namespaces, not from
        // the top-level Document keys used by the earlier A/B/A regression.
        assert_ne!(first_child.key, second_child.key);
        assert_eq!(first_child.state.rev, second_child.state.rev);
        let first_group = &first_child.state.root[0];
        let second_group = &second_child.state.root[0];
        assert_eq!(first_group.id, second_group.id);
        assert_eq!(first_group.props_rev, second_group.props_rev);
        assert_eq!(first_group.content_rev, second_group.content_rev);
        let first_leaf = &first_group.children().unwrap()[0];
        let second_leaf = &second_group.children().unwrap()[0];
        assert_eq!(first_leaf.id, second_leaf.id);
        assert_eq!(first_leaf.props_rev, second_leaf.props_rev);
        assert_eq!(first_leaf.content_rev, second_leaf.content_rev);

        let mut bottom = Layer::new("first child context", LayerKind::SmartObject(first_child));
        bottom.id = LayerId(10);
        let mut top = Layer::new("second child context", LayerKind::SmartObject(second_child));
        top.id = LayerId(11);
        let extent = Extent::new(257, 3);
        let mut parent_state = DocState::new(extent, Depth::F32);
        parent_state.root = vec![Arc::new(bottom), Arc::new(top)];
        let parent = Document::new(parent_state);
        // One full-level render contains both smart-object namespaces and
        // their nested styles. Identity transforms sample exact pixel centres.
        let (actual_extent, pixels) = Compositor::new(8 << 20)
            .render_level_rgba(&parent, 0)
            .unwrap();
        assert_eq!(actual_extent, extent);
        assert_eq!(pixels.len(), 257 * 3 * 4);
        for y in 0..3 {
            for x in 0..257 {
                // Opaque red+half-blue bottom survives where the top is clear;
                // opaque green+half-blue top replaces it starting at x=128.
                // This independent oracle is not another compositor render.
                let expected: [f32; 4] = if y != 1 || x == 0 {
                    [0.0; 4]
                } else if x < 128 {
                    [0.5, 0.0, 0.5, 1.0]
                } else {
                    [0.0, 0.5, 0.5, 1.0]
                };
                for c in 0..4 {
                    assert_eq!(
                        pixels[(y * 257 + x) * 4 + c].to_bits(),
                        expected[c].to_bits(),
                        "same-pass child contexts: pixel ({x},{y}) channel {c}"
                    );
                }
            }
        }
    }

    #[test]
    fn full_level_live_shape_styles_use_one_source_and_exact_overlay_pixels() {
        use crate::geom::Affine;
        use engine_api::id::LayerId;

        let extent = Extent::new(257, 3);
        // Extend the opaque axis-aligned shape beyond every canvas edge, so
        // all output samples are strictly interior. The oracle does not rely
        // on edge-antialias coverage or any platform font/raster font data.
        let model = vector::ShapeModel {
            path: vector::Path::polyline(
                &[
                    vector::Point::new(-4.0, -4.0),
                    vector::Point::new(261.0, -4.0),
                    vector::Point::new(261.0, 7.0),
                    vector::Point::new(-4.0, 7.0),
                ],
                true,
            ),
            fill: Some(vector::Fill::Solid([0.125, 0.25, 0.5, 1.0])),
            ..Default::default()
        };
        let mut layer = Layer::new(
            "opaque live styled shape",
            LayerKind::Shape {
                model,
                transform: Affine::IDENTITY,
            },
        );
        layer.id = LayerId(1);
        layer.props.fill_opacity = 0.0;
        layer.props.styles.effects = vec![styles::StyleEffect::ColorOverlay(styles::Overlay {
            fill: crate::document::Fill::Solid {
                color: [0.25, 0.5, 0.75],
            },
            ..Default::default()
        })];
        let mut state = DocState::new(extent, Depth::F32);
        state.root.push(Arc::new(layer));
        let doc = Document::new(state);
        assert!(super::super::live_damage::has_live(&doc.state().root));
        assert!(doc.state().has_layer_styles());
        let compositor = Compositor::new(8 << 20);
        let (actual_extent, pixels) = compositor.render_level_rgba(&doc, 0).unwrap();
        assert_eq!(actual_extent, extent);
        assert_eq!(pixels.len(), 257 * 3 * 4);
        // Source fill is hidden, but its opaque shape clips a unit-opacity
        // normal overlay. Every canvas pixel must be this exact dyadic RGBA.
        let expected: [f32; 4] = [0.25, 0.5, 0.75, 1.0];
        for (i, pixel) in pixels.chunks_exact(4).enumerate() {
            for c in 0..4 {
                assert_eq!(
                    pixel[c].to_bits(),
                    expected[c].to_bits(),
                    "live overlay pixel {i}, channel {c}"
                );
            }
        }
        assert!(
            compositor.stats().live_tiles > 0,
            "must rasterize a live source"
        );
        // Expected RED until the future full-level StylePass is forwarded
        // through render_live_scene as well as the plain composite route.
        // Baseline source count was observed RED; both candidate counts remain UNRUN.
        assert_eq!(
            (
                compositor
                    .stats
                    .source_raster_build_calls
                    .load(Ordering::Relaxed),
                compositor.stats.style_render_calls.load(Ordering::Relaxed)
            ),
            (1, 1),
            "source builds and style renders must each occur once per full-level pass"
        );
    }
    #[test]
    fn blurred_morphology_planes_match_uncached_with_budget_fallback() {
        use super::super::style_pass::StylePass;
        use engine_api::jobs::CancellationToken;
        let extent = Extent::new(257, 13);
        let mut raster = Raster::new(extent, 4, Depth::F32, 0.0);
        raster
            .edit_region(crate::geom::Rect::of_extent(extent), 1, |x, y, p| {
                let alpha = if y > 1 && y < 11 && !(x > 125 && x < 130 && y > 4 && y < 8) {
                    [0.25, 0.5, 0.75, 1.0][(x as usize + y as usize) % 4]
                } else {
                    0.0
                };
                *p = [0.25, 0.5, 0.75, alpha];
            })
            .unwrap();
        let mut layer = Layer::new("fractional alpha", LayerKind::Pixel(raster));
        layer.props.styles.effects = vec![
            styles::StyleEffect::DropShadow(styles::Shadow {
                size: 2.5,
                spread: 0.375,
                distance: 1.5,
                ..Default::default()
            }),
            styles::StyleEffect::OuterGlow(styles::Glow {
                size: 1.25,
                ..Default::default()
            }),
        ];
        let mut state = DocState::new(extent, Depth::F32);
        state.root.push(Arc::new(layer));
        let doc = Document::new(state);
        let render = |pass: Option<&StylePass>| {
            Compositor::new(8 << 20)
                .render_level_premultiplied_with_styles(&doc, 0, &CancellationToken::new(), pass)
                .unwrap()
                .iter()
                .flat_map(|t| t.samples::<f32>().unwrap().iter().map(|v| v.to_bits()))
                .collect::<Vec<_>>()
        };
        let uncached = render(None);
        let normal = StylePass::default();
        assert_eq!(render(Some(&normal)), uncached);
        assert_eq!(normal.usage().2, 1);
        for bytes in [0, 1] {
            let limited = StylePass::new(bytes, 256);
            assert_eq!(render(Some(&limited)), uncached);
            assert_eq!(limited.usage(), (0, 0, 0));
        }
        // A frozen preimplementation numerical oracle is a separate pending
        // runtime gate. This checks reuse/fallback parity, not kernel semantics.
    }
    // Authored benchmark, UNRUN; timing scope and historical baseline gate in BENCHMARK-PLAN.md.
    // No product changes. Requires the b64e5e00 private full-level style seam.
    #[test]
    #[ignore = "expensive release-only synthetic benchmark; requires exclusive runtime lane"]
    fn perf1_serial_style_reuse_benchmark() {
        use engine_api::{jobs::CancellationToken, tile::Tile};
        use std::hint::black_box;
        use std::time::{Duration, Instant};

        if cfg!(debug_assertions) {
            panic!("run with --release");
        }
        // Keep an accidental ignored-test sweep bounded to a small smoke fixture.
        // The 14MP preset is explicit and uses the reported canvas/effect radii.
        let preset = std::env::var("TESSERA_PERF1_PRESET").unwrap_or_else(|_| "smoke".into());
        let extent = match preset.as_str() {
            "smoke" => Extent::new(513, 259),
            "14mp" => Extent::new(4608, 3072),
            _ => panic!("TESSERA_PERF1_PRESET must be smoke or 14mp"),
        };
        let host = std::env::var("TESSERA_PERF1_HOST")
            .expect("set TESSERA_PERF1_HOST to hardware/OS/RAM/power/load provenance");
        let revision = std::env::var("TESSERA_PERF1_REVISION")
            .expect("set TESSERA_PERF1_REVISION to exact git HEAD plus dirty-state description");
        const CACHE_BYTES: usize = 64 << 20;
        const WARMUPS: usize = 1;
        const REPEATS: usize = 3;
        let mut raster = Raster::new(extent, 4, Depth::F32, 0.0);
        raster
            .edit_region(crate::geom::Rect::of_extent(extent), 1, |x, y, p| {
                // Resolution-independent dyadic alpha bands, interior hole, canvas edges;
                // synthetic raster deliberately avoids fonts, catalogs and file decoding.
                let ux = (u64::from(x) * 64 / u64::from(extent.width)) as u32;
                let uy = (u64::from(y) * 64 / u64::from(extent.height)) as u32;
                let inside = (2..62).contains(&ux)
                    && (2..62).contains(&uy)
                    && !((27..37).contains(&ux) && (23..41).contains(&uy));
                let alpha = if inside {
                    [0.25, 0.5, 0.75, 1.0][((ux + uy) % 4) as usize]
                } else {
                    0.0
                };
                *p = [0.125, 0.25, 0.5, alpha];
            })
            .unwrap();
        let mut layer = Layer::new("PERF1 synthetic raster", LayerKind::Pixel(raster));
        layer.props.styles.effects = vec![
            styles::StyleEffect::DropShadow(styles::Shadow {
                distance: 30.0,
                size: 40.0,
                spread: 0.0,
                ..Default::default()
            }),
            styles::StyleEffect::OuterGlow(styles::Glow {
                size: 30.0,
                ..Default::default()
            }),
        ];
        // Debug includes every default field and document lighting used by this build.
        let settings = format!("{:?}", layer.props.styles);
        let mut state = DocState::new(extent, Depth::F32);
        let lighting = format!("{:?}", state.global_light);
        state.root.push(Arc::new(layer));
        let doc = Document::new(state);
        let grid = extent.tile_grid(TILE_SIZE);
        let expected_tiles = u64::from(grid.0) * u64::from(grid.1);
        let expected_payload = u64::from(extent.width) * u64::from(extent.height) * 16 * 3;
        assert!(
            expected_payload <= 1 << 30,
            "fixture must fit normal retained cap"
        );
        eprintln!(
            "PERF1 host={host}; revision={revision}; arch={}; os={}; release=true; available_parallelism={:?}; RAYON_NUM_THREADS={:?}",
            std::env::consts::ARCH,
            std::env::consts::OS,
            std::thread::available_parallelism(),
            std::env::var("RAYON_NUM_THREADS")
        );
        eprintln!(
            "PERF1 preset={preset}; extent={extent:?}; tiles={grid:?}; level=0; depth=F32; planar premultiplied output; fixture=dyadic-alpha-bands-hole-v1; cache_bytes={CACHE_BYTES}; style_cap_bytes={}; style_cap_entries=256; predicted_retained_bytes={expected_payload}; warmups_per_path={WARMUPS}; repeats_per_path={REPEATS}; settings={settings}; global_light={lighting}",
            1usize << 30
        );
        eprintln!(
            "PERF1 boundary=full-level premultiplied tile render, including style-pass drop; excludes fixture/compositor construction, output digest/drop, interleaving/encoding/app spans; new compositor per render; serial both paths; warmups warm code/allocator, not frame result cache; cap excludes scratch/metadata/output/compositor cache/RSS; fallback forfeits once-per-pass benefit"
        );

        // Source kernel and pixel execution remain identical; only frame reuse differs.
        // The None path is the candidate's serial uncached control, NOT a frozen
        // historical binary. Cross-revision baseline procedure is documented separately.
        let render = |cached: bool| -> (Vec<Tile>, Duration, (u64, u64)) {
            let compositor = Compositor::new(CACHE_BYTES);
            let cancel = CancellationToken::new();
            let start = Instant::now();
            let tiles = if cached {
                compositor.render_level_premultiplied(&doc, 0, &cancel)
            } else {
                compositor.render_level_premultiplied_with_styles(&doc, 0, &cancel, None)
            }
            .unwrap();
            let elapsed = start.elapsed();
            let counts = (
                compositor
                    .stats
                    .source_raster_build_calls
                    .load(Ordering::Relaxed),
                compositor.stats.style_render_calls.load(Ordering::Relaxed),
            );
            // Compositor teardown is outside timing on both paths. No previous
            // output/compositor is retained when the next timed render begins.
            (tiles, elapsed, counts)
        };
        let digest = |tiles: &[Tile]| -> u64 {
            tiles
                .iter()
                .flat_map(|t| t.samples::<f32>().unwrap())
                .fold(0xcbf29ce484222325u64, |h, value| {
                    (h ^ u64::from(value.to_bits())).wrapping_mul(0x100000001b3)
                })
        };
        let expected_counts = |cached| {
            if cached {
                (1, 1)
            } else {
                (expected_tiles, expected_tiles)
            }
        };

        // Full exact comparison is untimed. Two full outputs coexist only here;
        // this raises validation peak memory, not timed-run resident peer output.
        let (baseline, _, baseline_counts) = render(false);
        let (candidate, _, candidate_counts) = render(true);
        assert_eq!(baseline_counts, expected_counts(false));
        assert_eq!(candidate_counts, expected_counts(true));
        assert_eq!(baseline.len(), candidate.len());
        for (a, b) in baseline.iter().zip(&candidate) {
            assert_eq!(a.coord(), b.coord());
            assert_eq!(a.layout(), b.layout());
            let (aa, bb) = (a.samples::<f32>().unwrap(), b.samples::<f32>().unwrap());
            assert_eq!(aa.len(), bb.len());
            for (i, (&a, &b)) in aa.iter().zip(bb).enumerate() {
                assert!(a.is_finite() && b.is_finite());
                assert_eq!(a.to_bits(), b.to_bits(), "tile sample {i}");
            }
        }
        let oracle_digest = digest(&baseline);
        drop(baseline);
        drop(candidate);
        eprintln!(
            "PERF1 untimed_all_bits_parity=pass; digest={oracle_digest:016x}; uncached_counts={baseline_counts:?}; cached_counts={candidate_counts:?}"
        );

        for _ in 0..WARMUPS {
            for cached in [false, true] {
                let (tiles, _, counts) = render(cached);
                assert_eq!(counts, expected_counts(cached));
                assert_eq!(black_box(digest(&tiles)), oracle_digest);
                drop(tiles);
            }
        }
        let mut durations = [Vec::new(), Vec::new()];
        for repeat in 0..REPEATS {
            // Alternate order to expose drift; report each observation as well as median.
            for cached in if repeat % 2 == 0 {
                [false, true]
            } else {
                [true, false]
            } {
                let (tiles, elapsed, counts) = render(cached);
                let observed_digest = black_box(digest(&tiles));
                drop(tiles);
                assert_eq!(observed_digest, oracle_digest);
                assert_eq!(counts, expected_counts(cached));
                durations[usize::from(cached)].push(elapsed.as_secs_f64());
                eprintln!(
                    "PERF1 repeat={repeat}; cached={cached}; seconds={:.6}; counts={counts:?}; digest={observed_digest:016x}",
                    elapsed.as_secs_f64()
                );
            }
        }
        for samples in &mut durations {
            samples.sort_by(f64::total_cmp);
        }
        let uncached = durations[0][REPEATS / 2];
        let cached = durations[1][REPEATS / 2];
        eprintln!(
            "PERF1 median_uncached_seconds={uncached:.6}; median_cached_seconds={cached:.6}; observed_ratio={:.4}; once_per_pass_invariant=pass; no_preselected_speed_threshold=true",
            uncached / cached
        );
    }
}
