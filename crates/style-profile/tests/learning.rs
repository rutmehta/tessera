use engine_api::recipe::{history::EditMeta, DevelopSettings};
use style_profile::{Features, Profile, Questionnaire};
fn sample(i: usize) -> (Features, DevelopSettings) {
    let a = (i as f64 * 2.399).sin();
    let b = (i as f64 * 1.719).cos();
    let mut embedding = vec![0.; 128];
    embedding[0] = (a * 0.5) as f32;
    embedding[70] = (b * 0.5) as f32;
    embedding[100] =
        (1. - f64::from(embedding[0]).powi(2) - f64::from(embedding[70]).powi(2)).sqrt() as f32;
    let f = Features {
        embedding,
        mean_luminance: 0.5 + 0.4 * (i as f64 * 0.613).sin(),
        ..Features::default()
    };
    let mut s = DevelopSettings::default();
    s.tone.exposure = (3. * f.mean_luminance + 2. * f64::from(f.embedding[0]) - 1.5) as f32;
    s.tone.shadows = (70. * f.mean_luminance - 60. * f64::from(f.embedding[70]) - 35.) as f32;
    s.white_balance.temperature = (5500. + 900. * f64::from(f.embedding[0])) as f32;
    (f, s)
}
#[test]
fn forty_examples_generalize_to_fifty_held_out() {
    let mut p = Profile::new("synthetic", Questionnaire::default()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    for i in 0..40 {
        std::fs::write(
            dir.path().join(format!("{i}.jpg")),
            b"synthetic indexed source",
        )
        .unwrap();
    }
    let mut catalog = index::Index::open(":memory:").unwrap();
    catalog
        .scan(
            dir.path(),
            &index::NoopSidecarReader,
            &index::NoopMetadataProvider,
        )
        .unwrap();
    let ids = catalog.search(&index::Query::default()).unwrap();
    let mut feedback_id = ids[0];
    for id in ids {
        let path = catalog.image_info(id).unwrap().path;
        let i = path
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap()
            .parse::<usize>()
            .unwrap();
        if i == 1 {
            feedback_id = id;
        }
        let (_, s) = sample(i);
        let mut doc = sidecar::RecipeDocument::default();
        doc.recipe.image_id = Some(id);
        doc.recipe
            .edit(EditMeta::user("Final edit", 1), |settings| *settings = s)
            .unwrap();
        sidecar::Sidecar::write_recipe(sidecar::Sidecar::paths(path).recipe, &doc).unwrap();
    }
    assert_eq!(
        p.collect_library(&catalog, |_, path| Ok(sample(
            path.file_stem().unwrap().to_str().unwrap().parse().unwrap()
        )
        .0))
            .unwrap(),
        40
    );
    assert_eq!(p.sample_count(), 40);
    let mut worst = 0_f64;
    for i in 40..90 {
        let (f, s) = sample(i);
        let predicted = p.predict(&f).unwrap();
        worst =
            worst.max(f64::from((s.tone.exposure - predicted.settings.tone.exposure).abs()) / 20.);
        worst =
            worst.max(f64::from((s.tone.shadows - predicted.settings.tone.shadows).abs()) / 200.);
        assert_eq!(predicted.sliders.len(), 51);
        let expected = serde_json::to_value(&s).unwrap();
        let actual = serde_json::to_value(&predicted.settings).unwrap();
        for slider in &predicted.sliders {
            let path = slider.path.as_str();
            let range = match path {
                "/white_balance/temperature" => 48000.,
                "/white_balance/tint" => 300.,
                "/tone/exposure" => 20.,
                "/color/grading/blending" => 100.,
                p if p.starts_with("/color/grading/") && p.ends_with("/hue") => 360.,
                p if p.starts_with("/color/grading/") && p.ends_with("/saturation") => 100.,
                _ => 200.,
            };
            let error = (expected.pointer(path).unwrap().as_f64().unwrap()
                - actual.pointer(path).unwrap().as_f64().unwrap())
            .abs()
                / range;
            worst = worst.max(error);
            assert!(error < 0.1, "{path}: normalized error {error}");
        }
    }
    eprintln!("worst held-out normalized slider error: {worst:.6}");
    assert!(worst < 0.1);
    let (f, mut s) = sample(1);
    s.tone.exposure = 4.;
    p.record_feedback(feedback_id, &f, &s).unwrap();
    assert_eq!(p.sample_count(), 40);
}
