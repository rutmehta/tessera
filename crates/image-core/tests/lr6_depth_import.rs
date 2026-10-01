//! Synthetic resources only; model inference is never installed by these tests.
use engine_api::recipe::{
    Recipe,
    settings::{LensBlur, LensBlurDepth},
};
use image_core::{
    Renderer, RendererConfig,
    depth::DepthProvider,
    ml_depth::{DepthMap, DepthStore},
};
use pipeline_cpu::Image;
use std::{io::Cursor, sync::Arc};

fn input() -> Image {
    Image::new(
        16,
        16,
        vec![
            (0..256)
                .map(|i| if i % 2 == 0 { 0.8 } else { 0.1 })
                .collect();
            3
        ],
    )
    .unwrap()
}
fn recipe() -> Recipe {
    let mut recipe = Recipe::default();
    recipe
        .edit(Default::default(), |s| {
            s.effects.lens_blur = Some(LensBlur {
                depth: Some(LensBlurDepth {
                    base_raw_depth_table: Some("opaque-table".into()),
                    regenerate: true,
                    ..Default::default()
                }),
                ..Default::default()
            })
        })
        .unwrap();
    recipe
}
fn provider() -> DepthProvider {
    DepthProvider::from_map(
        DepthMap::from_normalized_inverse(
            16,
            16,
            (0..256).map(|i| if i < 128 { 1. } else { 0. }).collect(),
        )
        .unwrap(),
    )
}

#[test]
fn imports_decodable_resource_into_store_and_uses_reference_after_reload() {
    let temp = tempfile::tempdir().unwrap();
    let store = DepthStore::new(temp.path(), 100000).unwrap();
    let mut recipe = recipe();
    let gray =
        image::GrayImage::from_fn(16, 16, |_, y| image::Luma([if y < 8 { 191 } else { 64 }]));
    let mut bytes = Cursor::new(Vec::new());
    gray.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    let depth = provider()
        .prepare_lens_blur_depth(&mut recipe, &input(), &store, |id| {
            assert_eq!(id, "opaque-table");
            Some(bytes.get_ref().clone())
        })
        .unwrap();
    assert!((depth.inverse_depth()[0] - 191. / 255.).abs() < 1e-6);
    assert!((depth.inverse_depth()[255] - 64. / 255.).abs() < 1e-6);
    recipe.validate().unwrap();
    let json = recipe.to_json().unwrap();
    let mut restored = Recipe::from_json(&json).unwrap();
    let state = restored
        .settings
        .effects
        .lens_blur
        .as_ref()
        .unwrap()
        .depth
        .as_ref()
        .unwrap();
    assert!(!state.regenerate);
    assert_eq!(
        DepthMap::cached(&store, &state.mask_key.unwrap()).unwrap(),
        depth
    );
    let cached = provider()
        .prepare_lens_blur_depth(&mut restored, &input(), &store, |_| {
            panic!("cache must precede resolver")
        })
        .unwrap();
    assert_eq!(cached, depth);
    assert!(!String::from_utf8(json).unwrap().contains("inverse_depth"));
    let renderer = Renderer::new(RendererConfig::default())
        .with_depth(Arc::new(DepthProvider::from_map(cached)));
    assert!(
        renderer
            .apply_depth_effects(&input(), &restored.settings)
            .is_ok()
    );
}

#[test]
fn corrupt_missing_and_wrong_extent_resources_regenerate_without_models() {
    for bytes in [
        None,
        Some(b"proprietary undecodable helper".to_vec()),
        Some({
            let mut out = Cursor::new(Vec::new());
            image::GrayImage::new(1, 1)
                .write_to(&mut out, image::ImageFormat::Png)
                .unwrap();
            out.into_inner()
        }),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let store = DepthStore::new(temp.path(), 100000).unwrap();
        let mut recipe = recipe();
        let depth = provider()
            .prepare_lens_blur_depth(&mut recipe, &input(), &store, |_| bytes.clone())
            .unwrap();
        assert_eq!(depth.inverse_depth()[0], 1.);
        assert_eq!(depth.inverse_depth()[255], 0.);
        assert!(
            !recipe
                .settings
                .effects
                .lens_blur
                .as_ref()
                .unwrap()
                .depth
                .as_ref()
                .unwrap()
                .regenerate
        );
        assert!(
            String::from_utf8(recipe.to_json().unwrap())
                .unwrap()
                .contains("regenerated depth: complete")
        );
        recipe.validate().unwrap();
    }
}

#[test]
fn failed_store_keeps_regeneration_pending() {
    let temp = tempfile::tempdir().unwrap();
    let store = DepthStore::new(temp.path(), 1).unwrap();
    let mut recipe = recipe();
    let before = recipe.clone();
    assert!(
        provider()
            .prepare_lens_blur_depth(&mut recipe, &input(), &store, |_| None)
            .is_err()
    );
    assert_eq!(recipe, before);
}

#[test]
#[ignore = "paired synthetic import; tools/orchestrate/wp/LR-6/gate.sh supplies LR6_RECIPE"]
fn synthetic_import_to_cpu_render() {
    let path = std::env::var_os("LR6_RECIPE").expect("run LR-6 gate script");
    let mut recipe = Recipe::from_json(&std::fs::read(path).unwrap()).unwrap();
    recipe.validate().unwrap();
    assert_eq!(
        recipe
            .settings
            .effects
            .lens_blur
            .as_ref()
            .unwrap()
            .focus_range,
        [0.2, 0.6]
    );
    let temp = tempfile::tempdir().unwrap();
    let store = DepthStore::new(temp.path(), 100000).unwrap();
    // First half lies inside imported focus range; second half is fully defocused.
    let depth = DepthMap::from_normalized_inverse(
        16,
        16,
        (0..256).map(|i| if i < 128 { 0.6 } else { 0. }).collect(),
    )
    .unwrap();
    let provider = DepthProvider::from_map(depth);
    let ready = provider
        .prepare_lens_blur_depth(&mut recipe, &input(), &store, |_| None)
        .unwrap();
    let renderer = Renderer::new(RendererConfig::default())
        .with_depth(Arc::new(DepthProvider::from_map(ready)));
    let out = renderer
        .apply_depth_effects(&input(), &recipe.settings)
        .unwrap();
    assert_eq!(&out.planes()[0][..128], &input().planes()[0][..128]);
    let contrast =
        |p: &[f32]| p.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>() / (p.len() - 1) as f32;
    assert!(
        contrast(&out.planes()[0][128..]) < 0.35,
        "at least 50% reduction of 0.7 input contrast"
    );
    let restored = Recipe::from_json(&recipe.to_json().unwrap()).unwrap();
    let again = renderer
        .apply_depth_effects(&input(), &restored.settings)
        .unwrap();
    for (a, b) in out
        .planes()
        .iter()
        .flatten()
        .zip(again.planes().iter().flatten())
    {
        assert!((a - b).abs() <= 1e-6);
    }
    recipe.validate().unwrap();
}
