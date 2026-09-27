#[path = "../../image-core/tests/support/linear_dng.rs"]
mod support;
use previews::{Codec, Jpeg, Level, PreviewSource, PreviewStore};
#[test]
fn linear_dng_grid_and_edited_previews_share_upright_rgb_path() {
    let dir = tempfile::tempdir().unwrap();
    for bits in [32, 16] {
        let path = dir.path().join(format!("merge-{bits}.dng"));
        let pixels: Vec<_> = (1..=6)
            .map(|i| [0.07 * i as f32, 0.1 * i as f32, 0.06 * i as f32])
            .collect();
        std::fs::write(&path, support::fixture(bits, 6, &pixels)).unwrap();
        let store = PreviewStore::new(dir.path().join(format!("cache-{bits}")), 1_000_000).unwrap();
        let (key, kind) = store.from_raw(&path, 32).unwrap();
        assert_eq!(kind, PreviewSource::Rendered);
        assert_eq!(key.orientation, 1);
        let full = Jpeg.decode(&store.get(&key, Level::Full).unwrap()).unwrap();
        assert_eq!(full.dimensions(), (3, 2));
        assert!(full.get_pixel(0, 0)[1] > full.get_pixel(2, 0)[1]);
        let mut recipe = engine_api::recipe::Recipe::default();
        let same = store
            .from_raw_settings(&path, 32, &recipe.settings, recipe.recipe_hash().0.0)
            .unwrap();
        assert_eq!(same, key);
        assert_eq!(store.render_count(), 1);
        recipe.settings.tone.exposure = -2.;
        let edited = store
            .from_raw_settings(&path, 32, &recipe.settings, recipe.recipe_hash().0.0)
            .unwrap();
        assert_ne!(edited, key);
        let full_edited = Jpeg
            .decode(&store.get(&edited, Level::Full).unwrap())
            .unwrap();
        assert_eq!(full_edited.dimensions(), (3, 2));
        assert!(full_edited.get_pixel(0, 0)[1] < full.get_pixel(0, 0)[1]);
        drop(store);
        let reopened =
            PreviewStore::new(dir.path().join(format!("cache-{bits}")), 1_000_000).unwrap();
        let warm = reopened
            .from_raw_settings(&path, 32, &recipe.settings, recipe.recipe_hash().0.0)
            .unwrap();
        assert_eq!(warm, edited);
        assert_eq!(reopened.source_work_count(), 0);
        assert_eq!(reopened.render_count(), 0);
        assert_eq!(
            Jpeg.decode(&reopened.get(&warm, Level::Full).unwrap())
                .unwrap(),
            full_edited
        );
    }
}
