//! Conservative fusion: only opaque sources and lossless canvas permutations.
//! General filtered reconstruction is not associative, even for affine maps.
use crate::document::{Fill, LayerKind, LayerProps, SmartFilter, SmartObject};
use crate::render::smart_filters::FilterBlend;
use engine_api::EngineResult;
use transform::{Kernel, Operation, TransformOp, free::FreeTransform};

pub(super) fn fuse(so: &SmartObject) -> EngineResult<Option<SmartFilter>> {
    let [source] = so.state.root.as_slice() else {
        return Ok(None);
    };
    let expected = LayerProps {
        name: source.props.name.clone(),
        ..Default::default()
    };
    if source.props != expected || source.mask.is_some() {
        return Ok(None);
    }
    let opaque = match &source.kind {
        LayerKind::Fill(Fill::Solid { .. }) => true,
        LayerKind::Fill(Fill::Gradient { stops, .. }) => {
            !stops.is_empty() && stops.iter().all(|s| s.color[3] == 1.0)
        }
        LayerKind::Fill(Fill::Pattern { rgba, .. }) => {
            !rgba.is_empty() && rgba.as_chunks::<4>().0.iter().all(|p| p[3] == 1.0)
        }
        _ => false,
    };
    if !opaque {
        return Ok(None);
    }
    let e = so.state.canvas;
    let mut matrix = FreeTransform::identity().matrix;
    let mut count = 0;
    for filter in so.filters.iter().filter(|f| f.enabled) {
        if filter.blend != FilterBlend::default() {
            return Ok(None);
        }
        let Some(op) = filter.transform_op()? else {
            return Ok(None);
        };
        if op.effective_kernel(0) != Kernel::Nearest {
            return Ok(None);
        }
        let Operation::Free(t) = op.operation else {
            return Ok(None);
        };
        let m = t.matrix;
        // Signed axis permutations preserve every canvas pixel and never clip.
        let axis = |a: f64, b: f64| (a.abs() == 1.0 && b == 0.0) || (a == 0.0 && b.abs() == 1.0);
        if m[2] != [0.0, 0.0, 1.0]
            || !axis(m[0][0], m[0][1])
            || !axis(m[1][0], m[1][1])
            || m[0][2].fract() != 0.0
            || m[1][2].fract() != 0.0
            || t.bounds(f64::from(e.width), f64::from(e.height)).ok()
                != Some([[0.0, 0.0], [f64::from(e.width), f64::from(e.height)]])
        {
            return Ok(None);
        }
        matrix = std::array::from_fn(|r| {
            std::array::from_fn(|c| (0..3).map(|k| m[r][k] * matrix[k][c]).sum())
        });
        count += 1;
    }
    if count < 2 {
        return Ok(None);
    }
    SmartFilter::transform(TransformOp {
        version: 1,
        kernel: Kernel::Nearest,
        operation: Operation::Free(FreeTransform { matrix }),
    })
    .map(Some)
}
