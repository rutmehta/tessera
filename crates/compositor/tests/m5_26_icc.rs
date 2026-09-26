//! Real LCMS-generated ICC looks, never a synthetic ICC byte header.
use compositor::{
    Adjustment, Compositor, Depth, DocOp, DocState, Document, Layer, LayerKind, Rect,
};
use engine_api::tile::{Extent, TileCoord};
use lcms2::{Flags, GlobalContext, Intent, PixelFormat, Profile, Transform};

fn abstract_fixture() -> Vec<u8> {
    Profile::new_bchsw_abstract_context(GlobalContext::new(), 17, 10.0, 1.0, 0.0, 0.0, None)
        .unwrap()
        .icc()
        .unwrap()
}

#[test]
fn abstract_icc_loads_as_red_fastest_color_lookup() {
    let bytes = abstract_fixture();
    let adjustment = Adjustment::color_lookup_from_icc(&bytes, 5).unwrap();
    let Adjustment::ColorLookup { size, data } = adjustment else {
        panic!("expected LUT")
    };
    assert_eq!(size, 5);
    assert_eq!(data.len(), 125);
    let reference = color_mgmt::sample_color_lookup_icc(&bytes, 5).unwrap();
    assert_eq!(data, reference.values);
    assert!(data[2 + 5 * (2 + 5 * 2)][0] > 0.55);
}

#[test]
fn rgb_device_link_loads_without_implicit_srgb_conversion() {
    let srgb = Profile::new_srgb();
    let mut registry = color_mgmt::Registry::new();
    let dst = registry.builtin(color_mgmt::Builtin::AdobeRgb).unwrap();
    let dst = Profile::new_icc(dst.icc_bytes()).unwrap();
    let transform: Transform<[f32; 3], [f32; 3]> = Transform::new(
        &srgb,
        PixelFormat::RGB_FLT,
        &dst,
        PixelFormat::RGB_FLT,
        Intent::RelativeColorimetric,
    )
    .unwrap();
    let link = Profile::new_device_link(&transform, 4.3, Flags::default()).unwrap();
    let adjustment = Adjustment::color_lookup_from_icc(&link.icc().unwrap(), 5).unwrap();
    let Adjustment::ColorLookup { size, data } = adjustment else {
        panic!("expected LUT")
    };
    assert_eq!(size, 5);
    assert_eq!(data.len(), 125);
    assert!((data[1 + 5 * (2 + 5 * 3)][0] - 0.25).abs() > 0.01);
}

#[test]
fn invalid_icc_inputs_are_engine_errors() {
    for bytes in [
        Vec::new(),
        b"invalid ICC".to_vec(),
        Profile::new_srgb().icc().unwrap(),
    ] {
        assert!(Adjustment::color_lookup_from_icc(&bytes, 5).is_err());
    }
    let bytes = abstract_fixture();
    for size in [0, 1, 257, u32::MAX] {
        assert!(Adjustment::color_lookup_from_icc(&bytes, size).is_err());
    }
    let cmyk = Profile::ink_limiting_context(
        GlobalContext::new(),
        lcms2::ColorSpaceSignature::CmykData,
        200.0,
    )
    .unwrap();
    assert!(Adjustment::color_lookup_from_icc(&cmyk.icc().unwrap(), 5).is_err());
}

#[test]
fn loaded_icc_renders_via_existing_lut_and_preserves_alpha() {
    let bytes = abstract_fixture();
    let adjustment = Adjustment::color_lookup_from_icc(&bytes, 5).unwrap();
    let want = color_mgmt::sample_color_lookup_icc(&bytes, 5)
        .unwrap()
        .sample([0.5; 3]);
    let extent = Extent::new(1, 1);
    let mut doc = Document::new(DocState::new(extent, Depth::F32));
    let mut raster = Layer::pixel("source", extent, Depth::F32);
    raster
        .raster_mut()
        .unwrap()
        .edit_region(Rect::of_extent(extent), 1, |_, _, p| {
            *p = [0.5, 0.5, 0.5, 0.75]
        })
        .unwrap();
    for layer in [
        raster,
        Layer::new("ICC look", LayerKind::Adjustment(adjustment)),
    ] {
        doc.apply(DocOp::AddLayer {
            parent: None,
            index: usize::MAX,
            layer,
        })
        .unwrap();
    }
    let tile = Compositor::new(1 << 20)
        .render_tile(&doc, TileCoord::new(0, 0, 0))
        .unwrap();
    let samples = tile.samples::<f32>().unwrap();
    for c in 0..3 {
        assert!((samples[c] - want[c]).abs() < 1e-5);
    }
    assert_eq!(samples[3], 0.75);
}
