use std::sync::Arc;
use tessera_ffi::EnhanceOptions;
use tessera_ffi::{Engine, ImageQuery, PhotoJobListener, PhotoJobState, PhotoProgress};

struct Listener;
impl PhotoJobListener for Listener {
    fn on_progress(&self, _: PhotoProgress) {}
}
fn fixture(value: f32) -> (tempfile::TempDir, Arc<Engine>, String) {
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("photos");
    std::fs::create_dir(&photos).unwrap();
    let image = merge::LinearImage {
        width: 8,
        height: 8,
        pixels: vec![[value; 3]; 64],
        color_matrix: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        as_shot_neutral: [1.; 3],
    };
    merge::write_dng(
        &mut std::fs::File::create(photos.join("source.dng")).unwrap(),
        &image,
        &Default::default(),
    )
    .unwrap();
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into()).unwrap();
    engine
        .index_folder(photos.to_string_lossy().into())
        .unwrap();
    let id = engine.list_images(ImageQuery::default()).unwrap()[0]
        .id
        .clone();
    (dir, engine, id)
}

#[test]
fn zero_nr_publishes_bit_exact_hdr_dng_and_persistent_stack() {
    let (dir, engine, id) = fixture(2.5);
    let options = EnhanceOptions {
        denoise_amount: Some(0),
        ..Default::default()
    };
    let status = engine
        .clone()
        .enhance(vec![id.clone()], options.clone(), Arc::new(Listener))
        .unwrap()
        .wait();
    assert_eq!(status.state, PhotoJobState::Completed, "{:?}", status.error);
    let output = &status.outputs[0];
    assert!(output.path.ends_with("source-Enhanced-NR.dng"));
    let dng =
        raw_decode::linear_dng::read(&mut std::fs::File::open(&output.path).unwrap()).unwrap();
    assert!(
        dng.pixels
            .iter()
            .flatten()
            .all(|v| v.to_bits() == 2.5f32.to_bits())
    );
    let second = engine
        .clone()
        .enhance(vec![id.clone()], options, Arc::new(Listener))
        .unwrap()
        .wait();
    assert_eq!(second.state, PhotoJobState::Completed);
    assert!(second.outputs[0].path.ends_with("source-Enhanced-NR-2.dng"));
    drop(engine);
    let reopened = Engine::open(dir.path().join("support").to_string_lossy().into()).unwrap();
    let stack = reopened.photo_stack(id).unwrap();
    assert_eq!(stack.len(), 3);
    assert_eq!(stack[0], second.outputs[0].image_id);
    assert!(stack.contains(&output.image_id));
    assert_eq!(
        reopened.list_images(ImageQuery::default()).unwrap().len(),
        3
    );
}

#[test]
fn missing_weights_are_an_offline_job_error_without_output() {
    if std::env::var_os("TESSERA_ENHANCE_MODEL_CACHE").is_some() {
        eprintln!("SKIP: external model cache configured");
        return;
    }
    let (_dir, engine, id) = fixture(0.2);
    for options in [
        EnhanceOptions {
            denoise_amount: Some(50),
            ..Default::default()
        },
        EnhanceOptions {
            super_resolution: true,
            ..Default::default()
        },
    ] {
        let status = engine
            .clone()
            .enhance(vec![id.clone()], options, Arc::new(Listener))
            .unwrap()
            .wait();
        assert_eq!(status.state, PhotoJobState::Failed);
        assert!(
            status
                .error
                .unwrap()
                .message
                .contains("missing enhancement weights")
        );
        assert!(status.outputs.is_empty());
    }
    assert_eq!(engine.list_images(ImageQuery::default()).unwrap().len(), 1);
}

#[test]
fn rotated_source_is_not_silently_written_with_identity_orientation() {
    let (dir, engine, id) = fixture(0.2);
    let conn = rusqlite::Connection::open(dir.path().join("support/index.sqlite")).unwrap();
    conn.execute(
        "UPDATE metadata SET value='6' WHERE image_id=? AND key='orientation'",
        [&id],
    )
    .unwrap();
    let status = engine
        .enhance(
            vec![id],
            EnhanceOptions {
                denoise_amount: Some(0),
                ..Default::default()
            },
            Arc::new(Listener),
        )
        .unwrap()
        .wait();
    assert_eq!(status.state, PhotoJobState::Failed);
    assert!(status.error.unwrap().message.contains("orientation"));
}

#[test]
fn options_require_an_operation_and_reject_unavailable_raw_details() {
    let defaults = EnhanceOptions::default();
    assert!(!defaults.allow_model_download);
    assert!(defaults.validate().is_err());
    assert!(
        EnhanceOptions {
            denoise_amount: Some(0),
            ..defaults.clone()
        }
        .validate()
        .is_ok()
    );
    assert!(
        EnhanceOptions {
            denoise_amount: Some(101),
            ..defaults.clone()
        }
        .validate()
        .is_err()
    );
    let error = EnhanceOptions {
        raw_details: true,
        ..defaults
    }
    .validate()
    .unwrap_err();
    assert!(error.to_string().contains("Raw Details"));
}
