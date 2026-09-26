use compositor::{
    Affine, Compositor, Depth, DocState, Document, Layer, LayerKind, Raster, Rect, SmartFilter,
    SmartObject,
};
use engine_api::tile::Extent;
use std::sync::Arc;
use transform::{Kernel, Operation, TransformOp, seam::ContentAwareScale};

#[test]
fn content_aware_stage_resizes_inside_unchanged_smart_canvas() {
    let e = Extent::new(3, 2);
    let mut raster = Raster::new(e, 4, Depth::F32, 0.);
    raster
        .edit_region(Rect::of_extent(e), 1, |x, _, p| {
            *p = [[0.1, 0., 0., 1.], [0.4, 0., 0., 1.], [0.9, 0., 0., 1.]][x as usize]
        })
        .unwrap();
    let mut child = DocState::new(e, Depth::F32);
    child
        .root
        .push(Arc::new(Layer::new("source", LayerKind::Pixel(raster))));
    let mut so = SmartObject::new(child, Affine::IDENTITY);
    so.filters.push(
        SmartFilter::transform(TransformOp {
            version: 1,
            kernel: Kernel::Bilinear,
            operation: Operation::ContentAwareScale(ContentAwareScale {
                target_width: 2,
                target_height: 2,
                amount: 1.,
                protect: Some([1., 0., 1.].repeat(2)),
            }),
        })
        .unwrap(),
    );
    let mut state = DocState::new(e, Depth::F32);
    state
        .root
        .push(Arc::new(Layer::new("smart", LayerKind::SmartObject(so))));
    let output = Compositor::new(1 << 20)
        .render_level_rgba(&Document::new(state), 0)
        .unwrap()
        .1;
    for row in output.as_chunks::<12>().0 {
        assert_eq!(row, &[0.1, 0., 0., 1., 0.9, 0., 0., 1., 0., 0., 0., 0.]);
    }
}
