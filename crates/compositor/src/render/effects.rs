//! CPU layer styles evaluated on a source region at the requested pyramid
//! level. Halo source tiles and cropped style planes share the render LRU.
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};

use engine_api::tile::{Extent, TILE_SIZE, Tile, TileCoord, TileLayout};
use engine_api::{EngineError, EngineResult};
use serde::{Deserialize, Serialize};

use super::exec::{FrameKind, Op, Src, TileJob};
use super::pixel::Params;
use super::{Compositor, DocRef, NodeKey, Part, styles};
use crate::blend::BlendMode;

use crate::document::{DocState, GroupMode, Knockout, Layer, LayerKind, LayerProps};
use crate::geom::{Rect, next_doc_key};
use crate::raster::{Depth, Raster};

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

// No pixel buffers live here. This small, bounded namespace registry gives
// derived source documents and plane sets collision-free process identities.
// Evicting an identity only sacrifices reuse: IDs are never recycled, and all
// old tiles remain subject to the compositor's existing byte-budgeted LRU.
#[derive(Clone, Copy)]
struct StyleNamespaces {
    source: u64,
    planes: u64,
}
#[derive(PartialEq, Eq)]
struct StyleIdentity {
    doc: u64,
    layer: u64,
    revision: u64,
    settings: [u8; 32],
}
fn style_namespaces(doc: DocRef<'_>, layer: &Layer) -> EngineResult<StyleNamespaces> {
    type Registry = VecDeque<(StyleIdentity, StyleNamespaces)>;
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    let bytes = serde_json::to_vec(&(
        &layer.props.styles,
        doc.state.global_light,
        doc.state.canvas,
        doc.state.depth,
    ))
    .map_err(|e| EngineError::internal(format!("style identity: {e}")))?;
    let (cols, rows) = doc.state.canvas.tile_grid(TILE_SIZE);
    let revision = (0..rows)
        .flat_map(|y| (0..cols).map(move |x| layer.stamp(0, x, y)))
        .max()
        .unwrap_or(0);
    let identity = StyleIdentity {
        doc: doc.key,
        layer: layer.id.0,
        revision,
        settings: *blake3::hash(&bytes).as_bytes(),
    };
    let mut registry = REGISTRY
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(i) = registry.iter().position(|(key, _)| key == &identity) {
        let entry = registry.remove(i).expect("located namespace");
        let result = entry.1;
        registry.push_back(entry);
        return Ok(result);
    }
    let result = StyleNamespaces {
        source: next_doc_key(),
        planes: next_doc_key(),
    };
    if registry.len() == 1024 {
        registry.pop_front();
    }
    registry.push_back((identity, result));
    Ok(result)
}

// Store plane descriptors in a small U8 tile so metadata, too, is budgeted.
// The private namespace uses Smart (straight) for every entry: node 0 is this
// descriptor tile, node 1 the source, and subsequent nodes the effect planes.
#[derive(Serialize, Deserialize)]
struct PlaneMetadata {
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
        let e = doc.state.canvas;
        let mut raster = Raster::new(e, 4, Depth::F32, 0.0);
        let (nx, ny) = e.tile_grid(TILE_SIZE);
        for y in 0..ny {
            for x in 0..nx {
                let coord = TileCoord::new(0, x, y);
                let tile = super::unpremultiply(&self.composite_premult(doc, coord)?)?;
                raster.set_slot(x, y, Some(tile), doc.state.rev)?;
            }
        }
        Ok(raster)
    }

    /// Fetch only intersecting source tiles, preserving the document coordinate
    /// system while making a compact, region-local straight RGBA raster.
    fn style_source_region(
        &self,
        doc: DocRef<'_>,
        level: u8,
        region: Rect,
    ) -> EngineResult<Raster> {
        let extent = Extent::new(region.width() as u32, region.height() as u32);
        let mut raster = Raster::new(extent, 4, Depth::F32, 0.0);
        let ts = i64::from(TILE_SIZE);
        for ty in region.y0 / ts..=(region.y1 - 1) / ts {
            for tx in region.x0 / ts..=(region.x1 - 1) / ts {
                let coord = TileCoord::new(level, tx as u32, ty as u32);
                let tile = super::unpremultiply(&self.composite_premult(doc, coord)?)?;
                let layout = tile.layout();
                let samples = tile.samples::<f32>()?;
                let rect =
                    Rect::of_tile(coord, doc.state.canvas.at_level(level)).intersect(&region);
                let local = Rect::new(
                    rect.x0 - region.x0,
                    rect.y0 - region.y0,
                    rect.x1 - region.x0,
                    rect.y1 - region.y0,
                );
                raster.edit_region(local, doc.state.rev, |x, y, p| {
                    let i = (y as i64 + region.y0 - ty * ts) as usize * layout.stride()
                        + (x as i64 + region.x0 - tx * ts) as usize;
                    for c in 0..4 {
                        p[c] = samples[c * layout.plane_len() + i];
                    }
                })?;
            }
        }
        Ok(raster)
    }
}

impl<'a> TileJob<'a> {
    fn cached_styles(&self, key: NodeKey) -> EngineResult<Option<StyledTile>> {
        let Some(metadata) = self.comp.cache_get(&key) else {
            return Ok(None);
        };
        let descriptors: Vec<PlaneMetadata> = serde_json::from_slice(metadata.samples::<u8>()?)
            .map_err(|e| EngineError::internal(format!("cached style metadata: {e}")))?;
        let Some(source) = self.comp.cache_get(&NodeKey { node: 1, ..key }) else {
            return Ok(None);
        };
        let mut planes = Vec::with_capacity(descriptors.len());
        for (i, plane) in descriptors.into_iter().enumerate() {
            let Some(tile) = self.comp.cache_get(&NodeKey {
                node: i as u64 + 2,
                ..key
            }) else {
                return Ok(None);
            };
            planes.push(EffectTile {
                samples: tile.samples::<f32>()?.to_vec(),
                mode: plane.mode,
                opacity: plane.opacity,
                outside: plane.outside,
                stroke: plane.stroke,
            });
        }
        Ok(Some(StyledTile {
            source: source.samples::<f32>()?.to_vec(),
            planes,
        }))
    }

    fn cache_styles(&self, key: NodeKey, styled: &StyledTile) -> EngineResult<()> {
        let metadata: Vec<_> = styled
            .planes
            .iter()
            .map(|plane| PlaneMetadata {
                mode: plane.mode,
                opacity: plane.opacity,
                outside: plane.outside,
                stroke: plane.stroke,
            })
            .collect();
        let mut bytes = serde_json::to_vec(&metadata)
            .map_err(|e| EngineError::internal(format!("style metadata: {e}")))?;
        let width = bytes.len().min(TILE_SIZE as usize);
        let height = bytes.len().div_ceil(width);
        bytes.resize(width * height, b' ');
        let descriptor = Tile::from_samples(
            self.coord,
            TileLayout {
                extent: Extent::new(width as u32, height as u32),
                halo: 0,
                channels: 1,
            },
            bytes,
        )?;
        self.comp.cache_put(
            NodeKey { node: 1, ..key },
            Tile::from_samples(self.coord, self.layout(), styled.source.clone())?,
        );
        for (i, plane) in styled.planes.iter().enumerate() {
            self.comp.cache_put(
                NodeKey {
                    node: i as u64 + 2,
                    ..key
                },
                Tile::from_samples(self.coord, self.layout(), plane.samples.clone())?,
            );
        }
        // Publishing last is helpful but not an atomic set insertion. A reader
        // checks every tile because any subset may be evicted by the shared LRU.
        self.comp.cache_put(key, descriptor);
        Ok(())
    }

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
        layer.props.styles.validate()?;
        self.doc.state.global_light.validate()?;
        let namespaces = style_namespaces(self.doc, layer)?;
        let key = NodeKey {
            doc: namespaces.planes,
            node: 0,
            part: Part::Smart,
            stamp: 0, // Revision is already encoded in the plane namespace.
            coord: self.coord,
        };
        let styled = match self.cached_styles(key)? {
            Some(styled) => styled,
            None => {
                let settings = styles::at_level(&layer.props.styles, self.coord.level);
                let extent = self.doc.state.canvas.at_level(self.coord.level);
                let target = Rect::of_tile(self.coord, extent);
                let region = target
                    .inflate(styles::halo(&settings))
                    .intersect(&Rect::of_extent(extent));
                let mut source = layer.clone();
                source.props = LayerProps::default();
                let mut state = self.doc.state.clone();
                state.depth = Depth::F32;
                state.root = vec![Arc::new(source)];
                let raster = self.comp.style_source_region(
                    DocRef {
                        state: &state,
                        key: namespaces.source,
                    },
                    self.coord.level,
                    region,
                )?;
                let planes = styles::render_at(
                    &raster,
                    &settings,
                    self.doc.state.global_light,
                    [region.x0 as u32, region.y0 as u32],
                    self.coord.level,
                )?;
                let crop = |raster: &Raster| {
                    let mut samples = vec![0.0; self.n * 4];
                    for y in 0..self.n / self.w {
                        for x in 0..self.w {
                            let p = raster.pixel(
                                (target.x0 - region.x0) as u32 + x as u32,
                                (target.y0 - region.y0) as u32 + y as u32,
                            );
                            for c in 0..4 {
                                samples[c * self.n + y * self.w + x] = p[c];
                            }
                        }
                    }
                    samples
                };
                let styled = StyledTile {
                    source: crop(&raster),
                    planes: planes
                        .iter()
                        .map(|plane| EffectTile {
                            samples: crop(&plane.raster),
                            mode: plane.mode,
                            opacity: plane.opacity,
                            outside: plane.outside,
                            stroke: plane.stroke,
                        })
                        .collect(),
                };
                self.cache_styles(key, &styled)?;
                styled
            }
        };
        ops.push(Op::Blend {
            layer,
            src: Src::Styled(styled),
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
mod tests {
    use super::*;
    use crate::document::Fill;
    use engine_api::tile::Extent;
    use styles::{LayerStyles, Overlay, StyleEffect};

    fn styled(comp: &Compositor, doc: DocRef<'_>, coord: TileCoord) -> StyledTile {
        let job = comp.job(doc, coord).unwrap();
        let layer = &doc.state.root[0];
        let mut ops = Vec::new();
        job.emit_styles(layer, Params::of(layer, false), &mut ops)
            .unwrap();
        match ops.remove(0) {
            Op::Blend {
                src: Src::Styled(styled),
                ..
            } => styled,
            _ => panic!("missing styled tile"),
        }
    }

    #[test]
    fn cpu_style_regions_match_whole_level_across_seams_and_canvas_edges() {
        use styles::{Bevel, BevelKind, Glow, Satin, Shadow, Stroke, StrokePosition};
        let extent = Extent::new(1063, 41);
        let mut layer = Layer::pixel("source", extent, Depth::F32);
        layer
            .raster_mut()
            .unwrap()
            .edit_region(Rect::of_extent(extent), 1, |x, y, p| {
                if (x % 127 < 61 && y > 3 && y < 34) || !(4..=1059).contains(&x) {
                    *p = [0.2, 0.4, 0.6, if x % 7 == 0 { 0.35 } else { 1.0 }];
                }
            })
            .unwrap();
        let shadow = Shadow {
            size: 1.7,
            spread: 0.2,
            distance: 37.5,
            ..Shadow::default()
        };
        layer.props.styles.effects = vec![
            StyleEffect::DropShadow(shadow.clone()),
            StyleEffect::InnerShadow(shadow),
            StyleEffect::OuterGlow(Glow {
                size: 2.3,
                spread: 0.2,
                ..Glow::default()
            }),
            StyleEffect::InnerGlow(Glow {
                size: 1.4,
                spread: 0.3,
                ..Glow::default()
            }),
            StyleEffect::Satin(Satin {
                size: 1.8,
                distance: 21.3,
                ..Satin::default()
            }),
            StyleEffect::Bevel(Bevel {
                size: 1.3,
                soften: 0.2,
                kind: BevelKind::Pillow,
                ..Bevel::default()
            }),
            StyleEffect::Stroke(Stroke {
                size: 2.5,
                position: StrokePosition::Center,
                fill: Fill::Pattern {
                    width: 3,
                    height: 1,
                    rgba: vec![1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0],
                    origin: [1.0, 0.0],
                },
                ..Stroke::default()
            }),
        ];
        let mut state = DocState::new(extent, Depth::F32);
        state.root = vec![Arc::new(layer.clone())];
        let doc = DocRef {
            state: &state,
            key: next_doc_key(),
        };
        let comp = Compositor::new(32 * 1024 * 1024);
        let mut source_state = state.clone();
        layer.props = LayerProps::default();
        source_state.root = vec![Arc::new(layer)];
        let source_doc = DocRef {
            state: &source_state,
            key: next_doc_key(),
        };
        for level in [0, 1, 2] {
            let le = extent.at_level(level);
            let source = comp
                .style_source_region(source_doc, level, Rect::of_extent(le))
                .unwrap();
            let settings = styles::at_level(&state.root[0].props.styles, level);
            let expected =
                styles::render_at(&source, &settings, state.global_light, [0, 0], level).unwrap();
            let (cols, rows) = le.tile_grid(TILE_SIZE);
            for ty in 0..rows {
                for tx in 0..cols {
                    let coord = TileCoord::new(level, tx, ty);
                    let actual = styled(&comp, doc, coord);
                    let rect = Rect::of_tile(coord, le);
                    let n = rect.area() as usize;
                    let w = rect.width() as usize;
                    assert_eq!(actual.planes.len(), expected.len());
                    for (plane_index, (a, e)) in actual.planes.iter().zip(&expected).enumerate() {
                        assert_eq!(
                            (a.mode, a.opacity, a.outside, a.stroke),
                            (e.mode, e.opacity, e.outside, e.stroke)
                        );
                        for y in 0..rect.height() as usize {
                            for x in 0..w {
                                let p = e
                                    .raster
                                    .pixel(rect.x0 as u32 + x as u32, rect.y0 as u32 + y as u32);
                                for (c, expected) in p.iter().enumerate() {
                                    assert!(
                                        (a.samples[c * n + y * w + x] - expected).abs() <= 1e-4,
                                        "seam mismatch plane {plane_index} at level {level}, tile {tx},{ty}, pixel {x},{y}, channel {c}: actual {} expected {}",
                                        a.samples[c * n + y * w + x],
                                        expected
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn cpu_style_large_documents_and_partial_cache_eviction_are_safe() {
        for extent in [Extent::new(5000, 4000), Extent::new(10000, 5000)] {
            let mut layer = Layer::new("large", LayerKind::Fill(Fill::Solid { color: [0.25; 3] }));
            layer.props.styles.effects = vec![StyleEffect::Overlay(Overlay::default())];
            let mut state = DocState::new(extent, Depth::F32);
            state.root = vec![Arc::new(layer)];
            let doc = DocRef {
                state: &state,
                key: next_doc_key(),
            };
            // Fits a single RGBA plane plus descriptors, not a complete set.
            let budget = TILE_SIZE as usize * TILE_SIZE as usize * 16 + 4096;
            let comp = Compositor::new(budget);
            for level in [0, 2] {
                let coord = TileCoord::new(level, 1, 1);
                let a = styled(&comp, doc, coord);
                let b = styled(&comp, doc, coord);
                assert_eq!(a.source, b.source);
                assert_eq!(a.planes[0].samples, b.planes[0].samples);
                assert!(comp.stats().cache_bytes <= budget);
            }
            assert!(comp.stats().root_full <= 4);
            assert!(comp.stats().evictions > 0);
        }
    }

    #[test]
    fn cpu_style_namespace_separates_sources_settings_light_and_revisions() {
        let mut state = DocState::new(Extent::new(7, 5), Depth::F32);
        let mut layer = Layer::new("style", LayerKind::Fill(Fill::Solid { color: [0.2; 3] }));
        layer.props.styles.effects = vec![StyleEffect::Overlay(Overlay::default())];
        let doc_key = next_doc_key();
        let identity =
            |s: &DocState, l: &Layer, key| style_namespaces(DocRef { state: s, key }, l).unwrap();
        let original = identity(&state, &layer, doc_key);
        assert_eq!(original.planes, identity(&state, &layer, doc_key).planes);
        assert_ne!(original.source, original.planes);
        assert_ne!(original.source, doc_key);
        assert_ne!(
            original.planes,
            identity(&state, &layer, next_doc_key()).planes
        );
        layer.id.0 += 1;
        assert_ne!(original.source, identity(&state, &layer, doc_key).source);
        layer.id.0 -= 1;
        layer.props.styles.scale = 2.0;
        assert_ne!(original.planes, identity(&state, &layer, doc_key).planes);
        layer.props.styles.scale = 1.0;
        state.global_light.angle += 1.0;
        assert_ne!(original.planes, identity(&state, &layer, doc_key).planes);
        state.global_light = styles::GlobalLight::default();
        state.rev += 1;
        assert_eq!(original.source, identity(&state, &layer, doc_key).source);
        layer.content_rev += 1;
        assert_ne!(original.source, identity(&state, &layer, doc_key).source);
    }

    #[test]
    fn cpu_style_tiles_use_requested_level_region_and_reuse_budgeted_planes() {
        let mut layer = Layer::new("styled", LayerKind::Fill(Fill::Solid { color: [0.2; 3] }));
        let fill = Fill::Pattern {
            width: 3,
            height: 1,
            rgba: vec![1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0],
            origin: [0.0; 2],
        };
        layer.props.styles = LayerStyles {
            effects: vec![StyleEffect::Overlay(Overlay {
                fill: fill.clone(),
                ..Overlay::default()
            })],
            ..LayerStyles::default()
        };
        let mut state = DocState::new(Extent::new(2051, 35), Depth::F32);
        state.root = vec![Arc::new(layer)];
        let doc = DocRef {
            state: &state,
            key: next_doc_key(),
        };
        let budget = 4 * 1024 * 1024;
        let comp = Compositor::new(budget);
        let job = comp.job(doc, TileCoord::new(2, 1, 0)).unwrap();
        let mut ops = Vec::new();
        job.emit_styles(&state.root[0], Params::of(&state.root[0], false), &mut ops)
            .unwrap();
        let Op::Blend {
            src: Src::Styled(styled),
            ..
        } = &ops[0]
        else {
            panic!("missing styled tile")
        };
        let expected = fill.sample((256.0 + 0.5) * 4.0, 2.0);
        for (c, expected) in expected.iter().enumerate() {
            assert!((styled.planes[0].samples[c * job.n] - expected).abs() < 1e-5);
        }
        assert!(
            comp.stats().root_full <= 2,
            "must not composite the whole L0 canvas"
        );
        let before = comp.stats();
        let mut repeat = Vec::new();
        job.emit_styles(
            &state.root[0],
            Params::of(&state.root[0], false),
            &mut repeat,
        )
        .unwrap();
        assert_eq!(comp.stats().root_full, before.root_full);
        assert!(comp.stats().cache_hits > before.cache_hits);
        assert!(comp.stats().cache_bytes <= budget);
    }
}
