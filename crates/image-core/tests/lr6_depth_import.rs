//! Synthetic resources only; model inference is never installed by these tests.
use engine_api::recipe::{
    Recipe,
    settings::{LensBlur, LensBlurDepth},
};
use image_core::{
    Renderer, RendererConfig,
    depth::{DepthProvider, import_lens_blur_depth, imported_depth_key},
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
    let mut recipe = Recipe::new(engine_api::id::ImageId(66));
    recipe
        .edit(
            engine_api::recipe::EditMeta {
                author: engine_api::recipe::Author::Import {
                    source: "synthetic".into(),
                },
                ..Default::default()
            },
            |s| {
                s.effects.lens_blur = Some(LensBlur {
                    depth: Some(LensBlurDepth {
                        base_raw_depth_table: Some("opaque-table".into()),
                        regenerate: true,
                        ..Default::default()
                    }),
                    ..Default::default()
                })
            },
        )
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

fn stored_provider(root: &std::path::Path) -> DepthProvider {
    // A fallback with the wrong extent makes bypassing the store fail. This
    // provider cannot load a model even if weights happen to be installed.
    DepthProvider::from_map(DepthMap::from_normalized_inverse(1, 1, vec![0.]).unwrap())
        .with_store(DepthStore::new(root.join("previews/depth-cache"), 100000).unwrap())
}

#[test]
fn imports_decodable_resource_into_store_and_uses_reference_after_reload() {
    let temp = tempfile::tempdir().unwrap();
    let store = DepthStore::new(temp.path().join("previews/depth-cache"), 100000).unwrap();
    let mut recipe = recipe();
    let gray =
        image::GrayImage::from_fn(16, 16, |_, y| image::Luma([if y < 8 { 191 } else { 64 }]));
    let mut bytes = Cursor::new(Vec::new());
    gray.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    let history = recipe.history.clone();
    let depth = import_lens_blur_depth(&mut recipe, (16, 16), &store, |id| {
        assert_eq!(id, "opaque-table");
        Some(bytes.get_ref().clone())
    })
    .unwrap()
    .unwrap();
    assert_eq!(recipe.history.entries.len(), history.entries.len());
    assert_eq!(recipe.history.entries[0].meta, history.entries[0].meta);
    assert!((depth.inverse_depth()[0] - 191. / 255.).abs() < 1e-6);
    assert!((depth.inverse_depth()[255] - 64. / 255.).abs() < 1e-6);
    recipe.validate().unwrap();
    let json = recipe.to_json().unwrap();
    let restored = Recipe::from_json(&json).unwrap();
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
    assert!(!String::from_utf8(json).unwrap().contains("inverse_depth"));
    let renderer =
        Renderer::new(RendererConfig::default()).with_depth(Arc::new(stored_provider(temp.path())));
    assert!(
        renderer
            .apply_depth_effects(&input(), &restored.settings)
            .is_ok()
    );
}

#[test]
fn corrupt_missing_and_wrong_extent_resources_remain_pending_without_inference() {
    for bytes in [
        None,
        Some(b"proprietary helper".to_vec()),
        Some(png(1, 1, 128)),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let store = DepthStore::new(temp.path(), 100000).unwrap();
        let mut r = recipe();
        assert!(
            import_lens_blur_depth(&mut r, (16, 16), &store, |_| bytes.clone())
                .unwrap()
                .is_none()
        );
        let state = r
            .settings
            .effects
            .lens_blur
            .as_ref()
            .unwrap()
            .depth
            .as_ref()
            .unwrap();
        assert!(state.regenerate);
        assert!(state.mask_key.is_none());
        r.validate().unwrap();
    }
}

fn png(width: u32, height: u32, value: u8) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    image::GrayImage::from_pixel(width, height, image::Luma([value]))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

#[test]
fn failed_import_store_keeps_recipe_unchanged() {
    let temp = tempfile::tempdir().unwrap();
    let store = DepthStore::new(temp.path(), 1).unwrap();
    std::fs::write(temp.path().join("pinned"), b"blocked directory").unwrap();
    let mut r = recipe();
    let before = r.clone();
    assert!(import_lens_blur_depth(&mut r, (16, 16), &store, |_| Some(png(16, 16, 128))).is_err());
    assert_eq!(r, before);
}

#[test]
#[ignore = "paired synthetic import; tools/orchestrate/wp/LR-6/gate.sh supplies LR6_RECIPE"]
fn synthetic_import_to_cpu_render() {
    let path = std::env::var_os("LR6_RECIPE").expect("run LR-6 gate script");
    let recipe = Recipe::from_json(&std::fs::read(path).unwrap()).unwrap();
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
    let depth = DepthMap::from_normalized_inverse(
        16,
        16,
        (0..256).map(|i| if i < 128 { 0.6 } else { 0. }).collect(),
    )
    .unwrap();
    let before = recipe.to_json().unwrap();
    let renderer = Renderer::new(RendererConfig::default())
        .with_depth(Arc::new(DepthProvider::from_map(depth.clone())));
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
    assert_eq!(recipe.to_json().unwrap(), before);
    let restored = Recipe::from_json(&before).unwrap();
    let renderer = Renderer::new(RendererConfig::default())
        .with_depth(Arc::new(DepthProvider::from_map(depth)));
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
    let mut edited = restored.settings.clone();
    edited.effects.lens_blur.as_mut().unwrap().focus_range = [0.9, 1.];
    let changed = renderer.apply_depth_effects(&input(), &edited).unwrap();
    assert_ne!(
        changed.planes(),
        again.planes(),
        "post-import focus must change pixels"
    );
    recipe.validate().unwrap();
}

#[test]
fn imported_depth_survives_cache_eviction_and_user_edits() {
    let tmp = tempfile::tempdir().unwrap();
    let store = DepthStore::new(tmp.path().join("previews/depth-cache"), 1100).unwrap();
    let mut r = recipe();
    let depth = import_lens_blur_depth(&mut r, (16, 16), &store, |_| Some(png(16, 16, 128)))
        .unwrap()
        .unwrap();
    let key = imported_depth_key(r.image_id.unwrap());
    for i in 0..3 {
        DepthMap::from_normalized_inverse(16, 16, vec![i as f32 / 3.; 256])
            .unwrap()
            .store(&store, &[i; 32])
            .unwrap();
    }
    r.edit(engine_api::recipe::EditMeta::user("exposure", 1), |s| {
        s.tone.exposure = 1.
    })
    .unwrap();
    let mut restored = Recipe::from_json(&r.to_json().unwrap()).unwrap();
    assert_eq!(DepthMap::cached(&store, &key), Some(depth));
    let renderer =
        Renderer::new(RendererConfig::default()).with_depth(Arc::new(stored_provider(tmp.path())));
    assert!(
        renderer
            .apply_depth_effects(&input(), &restored.settings)
            .is_ok()
    );
    let before = restored.clone();
    assert!(
        import_lens_blur_depth(&mut restored, (16, 16), &store, |_| panic!(
            "must reject user head before resolution"
        ))
        .is_err()
    );
    assert_eq!(restored, before);
    store.remove_pinned(&key).unwrap();
    assert!(DepthMap::cached(&store, &key).is_none());
}

#[test]
fn lr6e_render_does_not_write_resources_or_recipe() {
    let tmp = tempfile::tempdir().unwrap();
    let store = DepthStore::new(tmp.path(), 100000).unwrap();
    let r = recipe();
    let before = r.to_json().unwrap();
    let renderer =
        Renderer::new(RendererConfig::default()).with_depth(Arc::new(provider().with_store(store)));
    renderer.apply_depth_effects(&input(), &r.settings).unwrap();
    assert_eq!(r.to_json().unwrap(), before);
    assert_eq!(
        std::fs::read_dir(tmp.path()).unwrap().count(),
        0,
        "render must not persist estimated or imported depth"
    );
}

#[test]
fn lr6e_reimport_replaces_one_pin_per_image_and_user_edits_keep_it() {
    let tmp = tempfile::tempdir().unwrap();
    let store = DepthStore::new(tmp.path(), 1100).unwrap();
    let mut keys = Vec::new();
    for value in [64, 191] {
        let mut r = recipe();
        r.image_id = Some(engine_api::id::ImageId(66));
        let mut bytes = Cursor::new(Vec::new());
        image::GrayImage::from_pixel(16, 16, image::Luma([value]))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        import_lens_blur_depth(&mut r, (16, 16), &store, |_| Some(bytes.get_ref().clone()))
            .unwrap()
            .unwrap();
        let key = r
            .settings
            .effects
            .lens_blur
            .as_ref()
            .unwrap()
            .depth
            .as_ref()
            .unwrap()
            .mask_key
            .unwrap();
        keys.push(key);
        r.edit(engine_api::recipe::EditMeta::user("exposure", 1), |s| {
            s.tone.exposure = 1.
        })
        .unwrap();
        let saved = Recipe::from_json(&r.to_json().unwrap()).unwrap();
        assert_eq!(
            saved
                .settings
                .effects
                .lens_blur
                .as_ref()
                .unwrap()
                .depth
                .as_ref()
                .unwrap()
                .mask_key,
            Some(key)
        );
        assert!(
            (DepthMap::cached(&store, &key).unwrap().inverse_depth()[0] - value as f32 / 255.)
                .abs()
                < 1e-6
        );
        assert_eq!(
            std::fs::read_dir(tmp.path().join("pinned"))
                .unwrap()
                .count(),
            1
        );
    }
    assert_eq!(keys[0], keys[1], "ownership key is stable across reimport");
}

#[test]
fn lr6e_unresolved_reimport_removes_only_the_images_previous_pin() {
    let tmp = tempfile::tempdir().unwrap();
    let store = DepthStore::new(tmp.path(), 1).unwrap();
    for id in [66, 67] {
        let mut r = recipe();
        r.image_id = Some(engine_api::id::ImageId(id));
        import_lens_blur_depth(&mut r, (16, 16), &store, |_| Some(png(16, 16, 128))).unwrap();
    }
    let mut r = recipe();
    assert!(
        import_lens_blur_depth(&mut r, (16, 16), &store, |_| None)
            .unwrap()
            .is_none()
    );
    assert!(
        r.settings
            .effects
            .lens_blur
            .as_ref()
            .unwrap()
            .depth
            .as_ref()
            .unwrap()
            .regenerate
    );
    assert!(DepthMap::cached(&store, &imported_depth_key(engine_api::id::ImageId(66))).is_none());
    assert!(DepthMap::cached(&store, &imported_depth_key(engine_api::id::ImageId(67))).is_some());
    assert_eq!(
        std::fs::read_dir(tmp.path().join("pinned"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn lr6e_preview_reads_full_resolution_imported_depth_without_inference_or_writes() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("previews/depth-cache");
    let store = DepthStore::new(&root, 1).unwrap();
    let mut r = recipe();
    let depth = import_lens_blur_depth(&mut r, (16, 16), &store, |_| Some(png(16, 16, 128)))
        .unwrap()
        .unwrap();
    let key = imported_depth_key(r.image_id.unwrap());
    let preview = Image::new(8, 8, vec![vec![0.4; 64]; 3]).unwrap();
    // The 1x1 fallback deliberately cannot estimate this 8x8 frame.
    let renderer =
        Renderer::new(RendererConfig::default()).with_depth(Arc::new(stored_provider(tmp.path())));
    let before = r.to_json().unwrap();
    let actual = renderer.apply_depth_effects(&preview, &r.settings).unwrap();
    let expected_depth = DepthMap::from_normalized_inverse(8, 8, vec![128. / 255.; 64]).unwrap();
    let expected = Renderer::new(RendererConfig::default())
        .with_depth(Arc::new(DepthProvider::from_map(expected_depth)))
        .apply_depth_effects(&preview, &r.settings)
        .unwrap();
    assert_eq!(actual.planes(), expected.planes());
    assert_eq!(r.to_json().unwrap(), before);
    assert_eq!(DepthMap::cached(&store, &key), Some(depth));
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
    assert_eq!(std::fs::read_dir(root.join("pinned")).unwrap().count(), 1);
}
