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
        })?;
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
