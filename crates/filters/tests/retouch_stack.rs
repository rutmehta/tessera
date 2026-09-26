use compositor::{
    Affine, Compositor, DocState, Document, Layer, LayerKind, Raster, Rect, SmartObject,
    document::SmartFilter, raster::Depth, render::smart_filters::SmartFilterEvaluator,
};
use engine_api::tile::Extent;
use filters::{
    CompositorFilters,
    liquify::{Interpolation, Mesh},
};
use std::sync::{Arc, atomic::AtomicBool};

#[test]
fn retouch_variants_replay_through_native_serialization() {
    use filters::{
        caf,
        remove::{CpuPatchMatch, Remove, RemoveParams},
    };
    let e = Extent::new(24, 24);
    let mut input = Raster::new(e, 4, Depth::F32, 0.0);
    input
        .edit_region(Rect::of_extent(e), 1, |_, _, p| *p = [0.4, 0.2, 0.1, 1.])
        .unwrap();
    let mut mask = vec![0.; 576];
    for y in 10..14 {
        for x in 10..14 {
            mask[y * 24 + x] = 1.;
        }
    }
    let cancel = AtomicBool::new(false);
    let params = caf::FillParams::default();
    let fill = caf::fill(&input, &mask, &params, &cancel)
        .unwrap()
        .composite;
    let cpu: &mut dyn Remove = &mut CpuPatchMatch;
    let removed = cpu
        .apply(&input, &mask, &RemoveParams::default(), &cancel)
        .unwrap();
    assert_eq!(removed.backend, filters::remove::BackendUsed::CpuPatchMatch);
    for name in [
        "content_aware_fill",
        "content_aware_move",
        "content_aware_extend",
        "remove",
    ] {
        let params = match name {
            "content_aware_fill" => serde_json::json!({"mask":mask,"fill":params}),
            "remove" => serde_json::json!({"mask":mask,"remove":{"dilation":0,"backend":"cpu"}}),
            _ => serde_json::json!({"mask":mask,"offset":[5,0],"fill":params,"seam":"default"}),
        };
        let node = SmartFilter {
            name: name.into(),
            params,
            enabled: true,
            ..Default::default()
        };
        let serialized = serde_json::to_vec(&node).unwrap();
        let roundtrip: SmartFilter = serde_json::from_slice(&serialized).unwrap();
        let result = CompositorFilters.evaluate(&input, &roundtrip).unwrap();
        for c in 0..4 {
            assert!((result.pixel(12, 12)[c] - fill.pixel(12, 12)[c]).abs() < 1e-5);
        }
    }
}

#[test]
fn liquify_native_stack_roundtrip_and_psd_proxy() {
    let e = Extent::new(16, 12);
    let mut input = Raster::new(e, 4, Depth::F32, 0.0);
    input
        .edit_region(Rect::of_extent(e), 1, |x, y, p| {
            *p = [x as f32 / 16., y as f32 / 12., 0.2, 1.]
        })
        .unwrap();
    let mut mesh = Mesh::new(16, 12, 4).unwrap();
    mesh.displacement.fill([1.25, -0.5]);
    let filter = SmartFilter {
        name: "liquify".into(),
        enabled: true,
        params: serde_json::json!({"mesh":mesh,"interpolation":"bilinear"}),
        ..Default::default()
    };
    let expected = mesh
        .render(&input, Interpolation::Bilinear, &AtomicBool::new(false))
        .unwrap();
    let actual = CompositorFilters.evaluate(&input, &filter).unwrap();
    assert_eq!(actual.pixel(5, 5), expected.pixel(5, 5));
    let mut child = DocState::new(e, Depth::F32);
    child
        .root
        .push(Arc::new(Layer::new("source", LayerKind::Pixel(input))));
    let mut so = SmartObject::new(child, Affine::IDENTITY);
    so.filters.push(filter.clone());
    let mut state = DocState::new(e, Depth::F32);
    state.root.push(Arc::new(Layer::new(
        "retouched",
        LayerKind::SmartObject(so),
    )));
    let bytes = compositor::format::to_bytes(&state).unwrap();
    let loaded = compositor::format::from_bytes(&bytes).unwrap();
    let LayerKind::SmartObject(so) = &loaded.root[0].kind else {
        panic!("lost smart object")
    };
    assert_eq!(so.filters, vec![filter]);
    let mut renderer = Compositor::new(8 << 20);
    renderer.set_filter_evaluator(Arc::new(CompositorFilters));
    let doc = Document::new(loaded);
    let (proxy, notes) = doc.rasterized_for_export(&mut renderer).unwrap();
    assert!(notes.iter().any(|n| n.contains("rasterized")));
    let psd = compositor::psd::to_psd(&proxy).unwrap();
    let imported = Document::from_psd(psd).unwrap();
    let px = renderer.render_level_rgba(&imported, 0).unwrap().1;
    let want = expected.pixel(5, 5);
    for c in 0..4 {
        assert!((px[(5 * 16 + 5) * 4 + c] - want[c]).abs() < 0.01);
    }
}
