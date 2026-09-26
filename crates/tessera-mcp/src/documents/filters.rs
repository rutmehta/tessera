//! Atomic document filter evaluation. The adapter never downloads model weights.
use super::{Documents, dense};
use compositor::render::smart_filters::{FilterContext, SmartFilterEvaluator};
use compositor::{Compositor, Depth, DocOp, PaintTarget, Raster, Rect, SmartFilter};
use engine_api::{
    EngineError, EngineResult,
    id::LayerId,
    tools::{DocumentToolCall as Call, DocumentToolOutput, DocumentToolRequest},
};
use serde_json::Value;
use std::sync::Arc;

pub(super) fn compositor(budget: usize) -> Compositor {
    let mut c = Compositor::new(budget);
    c.set_filter_evaluator(Arc::new(filters::CompositorFilters));
    c
}

fn arguments(call: &Call) -> EngineResult<(LayerId, &Value, bool, &str)> {
    let (layer, params, smart, name) = match call {
        Call::DocumentRemoveObject {
            layer,
            params,
            smart,
            ..
        } => (layer, params, smart, "remove"),
        Call::RemoveDistractions {
            layer,
            params,
            smart,
            ..
        } => (layer, params, smart, "remove_distractions"),
        Call::ContentAwareFill {
            layer,
            params,
            smart,
            ..
        } => (layer, params, smart, "content_aware_fill"),
        Call::ContentAwareMove {
            layer,
            params,
            smart,
            ..
        } => (layer, params, smart, "content_aware_move"),
        Call::Liquify {
            layer,
            params,
            smart,
            ..
        } => (layer, params, smart, "liquify"),
        Call::CameraRawFilter {
            layer,
            params,
            smart,
            ..
        } => (layer, params, smart, "camera_raw"),
        Call::NeuralSkinSmoothing {
            layer,
            params,
            smart,
            ..
        } => (layer, params, smart, "neural/skin_smoothing"),
        Call::NeuralColorize {
            layer,
            params,
            smart,
            ..
        } => (layer, params, smart, "neural/colorize"),
        Call::NeuralJpegArtifactRemoval {
            layer,
            params,
            smart,
            ..
        } => (layer, params, smart, "neural/jpeg_artifact_removal"),
        _ => return Err(EngineError::internal("not a filter call")),
    };
    Ok((*layer, params, *smart, name))
}

impl Documents {
    pub(super) fn run_filter(
        &mut self,
        request: &DocumentToolRequest,
    ) -> EngineResult<DocumentToolOutput> {
        let document = request.call.document().expect("filter document");
        let (id, params, smart, name) = arguments(&request.call)?;
        let session = self.session(document)?;
        let layer = session.layer(id)?;
        if layer.props.locks.all || layer.props.locks.pixels {
            return Err(EngineError::invalid("layer", "pixels are locked"));
        }
        let (input, context) = if smart {
            let compositor::LayerKind::SmartObject(so) = &layer.kind else {
                return Err(crate::unsupported(
                    "smart=true requires an existing smart object; pixel-layer filter stacks are not supported by the compositor",
                ));
            };
            if session.state().selection.is_some() || layer.props.locks.transparency {
                return Err(crate::unsupported(
                    "smart filters with an active parent-space selection or transparency lock are not supported",
                ));
            }
            // Evaluate the existing stack in child space, before the shared mask
            // and outer transform. These must not be baked into a new node.
            let mut unmasked = so.clone();
            unmasked.transform = compositor::Affine::IDENTITY;
            unmasked.filter_mask = None;
            let mut state = compositor::DocState::new(so.state.canvas, Depth::F32);
            state.profile = so.state.profile.clone();
            state.root.push(Arc::new(compositor::Layer::new(
                "input",
                compositor::LayerKind::SmartObject(unmasked),
            )));
            let (_, rgba) =
                compositor(64 << 20).render_level_rgba(&compositor::Document::new(state), 0)?;
            let mut input = Raster::new(so.state.canvas, 4, Depth::F32, 0.0);
            input.edit_region(Rect::of_extent(so.state.canvas), 0, |x, y, p| {
                let i = ((y * so.state.canvas.width + x) * 4) as usize;
                p.copy_from_slice(&rgba[i..i + 4]);
            })?;
            (
                input,
                FilterContext {
                    profile: so.state.profile.clone(),
                    canvas: so.state.canvas,
                    level: 0,
                },
            )
        } else {
            let compositor::LayerKind::Pixel(source) = &layer.kind else {
                return Err(crate::unsupported(
                    "destructive filters require a pixel layer; use smart=true for smart objects",
                ));
            };
            // Match the compositor's normalized F32 filter input convention.
            let mut input = Raster::new(source.extent(), 4, Depth::F32, 0.0);
            input.edit_region(Rect::of_extent(source.extent()), 0, |x, y, p| {
                *p = source.pixel(x, y)
            })?;
            (
                input,
                FilterContext {
                    profile: session.state().profile.clone(),
                    canvas: source.extent(),
                    level: 0,
                },
            )
        };
        let (mut node, report) = if name == "remove_distractions" {
            let (node, report) = filters::detect_distractions(&input, params)?;
            (node, Some(report))
        } else {
            (
                SmartFilter {
                    name: name.into(),
                    params: params.clone(),
                    enabled: true,
                    ..Default::default()
                },
                None,
            )
        };
        node.enabled = true;
        let result = filters::CompositorFilters.evaluate(&input, &node, &context)?;
        if result.extent() != input.extent()
            || result.channels() != 4
            || result.depth() != Depth::F32
        {
            return Err(EngineError::invalid(
                "filter",
                "evaluator changed raster layout",
            ));
        }
        let rect = Rect::of_extent(input.extent());
        let mut finite = true;
        for y in 0..input.extent().height {
            for x in 0..input.extent().width {
                finite &= result.pixel(x, y).iter().all(|v| v.is_finite());
            }
        }
        if !finite {
            return Err(EngineError::invalid("filter", "nonfinite result"));
        }
        let op = if smart {
            let compositor::LayerKind::SmartObject(so) = &layer.kind else {
                unreachable!()
            };
            let mut stack = so.filters.clone();
            stack.push(node);
            DocOp::SetSmartFilters {
                id,
                filters: stack,
                mask: so.filter_mask.clone(),
            }
        } else {
            let source = layer.raster().expect("validated pixel layer");
            let tiles = dense::deltas(source, rect, |x, y, p| {
                let out = result.pixel(x, y);
                finite &= out.iter().all(|v| v.is_finite());
                let weight = session
                    .state()
                    .selection
                    .as_ref()
                    .map_or(1.0, |s| s.pixel(x, y)[0]);
                let channels = if layer.props.locks.transparency { 3 } else { 4 };
                for c in 0..channels {
                    p[c] += weight * (out[c] - p[c]);
                }
            })?;
            if !finite {
                return Err(EngineError::invalid("filter", "nonfinite result"));
            }
            DocOp::PaintTiles {
                id,
                target: PaintTarget::Content,
                tiles,
                dirty: rect,
            }
        };
        let (entry, _) = self.session_mut(document)?.commit(op, request)?;
        if let Some(report) = report {
            Ok(DocumentToolOutput::DistractionsRemoved {
                document,
                entry,
                layer: id,
                report,
            })
        } else {
            Ok(DocumentToolOutput::DocumentEdited {
                document,
                entry: Some(entry),
                layer: Some(id),
                selection: None,
                channel: None,
            })
        }
    }
}
