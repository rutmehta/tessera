//! Tethered capture through the FFI with the folder-drop test camera
//! (`tether_use_fake`, the app's `--fake-tether` aid). No camera needed.
use std::{
    path::Path,
    time::{Duration, Instant},
};
use tessera_ffi::{Engine, tether::TetherFrame};

fn shoot(dir: &Path, names: &[&str]) {
    std::fs::create_dir_all(dir).unwrap();
    for (i, name) in names.iter().enumerate() {
        let img = image::RgbImage::from_fn(96, 64, |x, y| {
            let v = ((x * 7 + y * 3 + i as u32 * 40) % 255) as u8;
            image::Rgb([v, 255 - v, (x % 2 * 200) as u8])
        });
        img.save(dir.join(name)).unwrap();
    }
}

fn wait_for(engine: &Engine, count: usize) -> Vec<TetherFrame> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut frames = Vec::new();
    while frames.len() < count && Instant::now() < deadline {
        frames.extend(engine.tether_poll().unwrap());
        std::thread::sleep(Duration::from_millis(20));
    }
    frames
}

#[test]
fn fake_camera_capture_downloads_names_indexes_and_scores() {
    let dir = tempfile::tempdir().unwrap();
    let camera = dir.path().join("card");
    shoot(&camera, &["DSC_0002.jpg", "DSC_0001.jpg"]);
    let engine = Engine::open(dir.path().join("app").to_string_lossy().into_owned()).unwrap();
    engine
        .tether_use_fake(Some(camera.to_string_lossy().into_owned()), 0)
        .unwrap();

    let devices = engine.tether_devices().unwrap();
    assert_eq!(devices.len(), 1);
    assert!(devices[0].can_capture);
    assert_eq!(devices[0].shots_remaining, Some(2));
    assert!(!engine.tether_active());

    let session = dir.path().join("shoot").join("Session A");
    engine
        .tether_start(
            session.to_string_lossy().into_owned(),
            "studio_{sequence}_{original}.{ext}".into(),
        )
        .unwrap();
    assert!(engine.tether_active());
    assert!(
        engine.tether_poll().unwrap().is_empty(),
        "no timer: nothing without capture"
    );

    engine.tether_capture().unwrap();
    let first = wait_for(&engine, 1);
    assert_eq!(first.len(), 1);
    let f = &first[0];
    assert_eq!(f.sequence, 1);
    assert!(f.error.is_none(), "{:?}", f.error);
    assert!(f.path.ends_with("studio_0001_DSC_0001.jpg"), "{}", f.path);
    assert!(Path::new(&f.path).is_file());
    assert!(f.image_id.is_some());
    assert!(Path::new(f.preview.as_ref().unwrap()).is_file());
    let sharpness = f
        .sharpness
        .expect("quality scored before the frame is published");
    assert!((0.0..=1.0).contains(&sharpness));
    // No face weights in the app dir: a warning, and no fabricated zero-face result.
    assert!(f.face_warning.is_some());
    assert_eq!(f.faces, None);
    assert_eq!(f.eyes_open, None);
    assert_eq!(engine.tether_devices().unwrap()[0].shots_remaining, Some(1));

    engine.tether_capture().unwrap();
    assert!(
        engine.tether_capture().is_err(),
        "the card has one frame left"
    );
    let second = wait_for(&engine, 1);
    assert_eq!(second[0].sequence, 2);
    assert!(second[0].path.ends_with("studio_0002_DSC_0002.jpg"));

    // The catalog sees both frames as undecided images of the session folder.
    let handle = engine
        .index_folder(session.to_string_lossy().into_owned())
        .unwrap();
    let cull = engine.open_cull_session(handle.path).unwrap();
    assert_eq!(cull.images().unwrap().len(), 2);

    engine.tether_stop().unwrap();
    assert!(!engine.tether_active());
    assert!(engine.tether_capture().is_err());
    engine.tether_use_fake(None, 0).unwrap();
}

#[test]
fn fake_camera_timer_drops_frames_and_bad_naming_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let camera = dir.path().join("card");
    shoot(&camera, &["A.jpg", "B.jpg"]);
    let engine = Engine::open(dir.path().join("app").to_string_lossy().into_owned()).unwrap();
    assert!(
        engine
            .tether_use_fake(
                Some(dir.path().join("missing").to_string_lossy().into_owned()),
                0
            )
            .is_err()
    );
    engine
        .tether_use_fake(Some(camera.to_string_lossy().into_owned()), 30)
        .unwrap();
    let session = dir.path().join("timer");
    // Paths and a changed extension are refused before any camera work.
    for bad in [
        "../{sequence}.{ext}",
        "{sequence}.tif",
        "{sequence}_{camera}.{ext}",
    ] {
        assert!(
            engine
                .tether_start(session.to_string_lossy().into_owned(), bad.into())
                .is_err(),
            "{bad}"
        );
        assert!(!engine.tether_active());
    }
    engine
        .tether_start(
            session.to_string_lossy().into_owned(),
            "{sequence}.{ext}".into(),
        )
        .unwrap();
    let frames = wait_for(&engine, 2);
    let names: Vec<_> = frames
        .iter()
        .map(|f| {
            Path::new(&f.path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(
        names,
        ["0001.jpg", "0002.jpg"],
        "arrival order, file-name order on the card"
    );
    let tail = engine.tether_stop().unwrap();
    assert!(tail.is_empty());
    engine.tether_use_fake(None, 0).unwrap();
}
