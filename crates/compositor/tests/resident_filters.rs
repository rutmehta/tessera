use compositor::document::SmartFilter;
use compositor::geom::Rect;
use compositor::gpu::GpuCompositor;
use compositor::resident::ResidentRenderer;
use compositor::{
    Affine, Compositor, Depth, DocState, Document, Layer, LayerKind, Raster, SmartObject,
};
use engine_api::tile::Extent;
use std::sync::Arc;

fn document(filters: Vec<SmartFilter>) -> Document {
    let e = Extent::new(17, 13);
    let mut raster = Raster::new(e, 4, Depth::F32, 0.0);
    raster
        .edit_region(Rect::of_extent(e), 1, |x, y, p| {
            *p = [x as f32 / 19.0, y as f32 / 17.0, 0.25, 0.5];
        })
        .unwrap();
    let mut child = DocState::new(e, Depth::F32);
    child
        .root
        .push(Arc::new(Layer::new("pixels", LayerKind::Pixel(raster))));
    let mut so = SmartObject::new(child, Affine::IDENTITY);
    so.filters = filters;
    let mut state = DocState::new(e, Depth::F32);
    state
        .root
        .push(Arc::new(Layer::new("filtered", LayerKind::SmartObject(so))));
    Document::new(state)
}

#[test]
fn resident_invert_is_not_silently_omitted() {
    let gpu = GpuCompositor::new().unwrap();
    let doc = document(vec![SmartFilter {
        name: "invert".into(),
        enabled: true,
        ..Default::default()
    }]);
    let mut resident = ResidentRenderer::new(&gpu).unwrap();
    let cpu = Compositor::new(1 << 20);
    for level in [0, 2] {
        resident.render(&doc, level).unwrap();
        let actual = resident.read_level(level, false).unwrap().1;
        let expected = cpu.render_level_rgba(&doc, level).unwrap().1;
        assert_eq!(actual.len(), expected.len());
        let error = actual
            .iter()
            .zip(expected)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(error <= 1e-4, "L{level}: {error}");
    }
}
