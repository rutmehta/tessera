mod common;

use std::sync::Arc;

use common::layer_fn;
use compositor::{gpu::GpuCompositor, render::styles::*, resident::ResidentRenderer, *};
use engine_api::{EngineError, tile::Extent};

#[test]
fn unvalidated_visible_styles_return_errors_before_halo_arithmetic() {
    let gpu = GpuCompositor::new().expect("style validation regression requires a GPU");
    let extent = Extent::new(16, 16);
    for value in [f32::INFINITY, f32::NAN, f32::MAX, -1.0] {
        for field in ["size", "distance", "scale", "light elevation"] {
            let mut layer = layer_fn("visible styled layer", extent, Depth::F32, |_, _| {
                [0.7, 0.2, 0.5, 1.0]
            });
            let mut shadow = Shadow::default();
            let mut state = DocState::new(extent, Depth::F32);
            match field {
                "size" => shadow.size = value,
                "distance" => shadow.distance = value,
                "scale" => layer.props.styles.scale = value,
                "light elevation" => state.global_light.elevation = value,
                _ => unreachable!(),
            }
            layer.props.visible = true;
            layer.props.styles.effects = vec![StyleEffect::DropShadow(shadow)];
            state.root = vec![Arc::new(layer)];
            // Deliberately bypass DocOp validation to exercise the render boundary.
            let doc = Document::new(state);
            for level in [0, 2] {
                for viewport in [false, true] {
                    let mut resident = ResidentRenderer::new(&gpu).unwrap();
                    // Any panic fails the test; invalid inputs must return an error.
                    let result = if viewport {
                        resident.render_viewport(&doc, level, Rect::new(1, 1, 3, 3), 0)
                    } else {
                        resident.render(&doc, level)
                    };
                    assert!(
                        matches!(result, Err(EngineError::InvalidArgument { .. })),
                        "{field}={value:?}, L{level}, viewport={viewport}: expected invalid-input error, got {result:?}"
                    );
                }
            }
        }
    }
}
