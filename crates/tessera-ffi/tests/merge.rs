use std::sync::{Arc, Mutex};
use tessera_ffi::*;

#[derive(Default)]
struct Listener(Mutex<Vec<PhotoProgress>>);
impl PhotoJobListener for Listener {
    fn on_progress(&self, progress: PhotoProgress) {
        self.0.lock().unwrap().push(progress);
    }
}

#[test]
fn cancel_at_publication_checkpoint_leaves_no_dng_or_catalog_row() {
    struct Paused(std::sync::Barrier);
    impl PhotoJobListener for Paused {
        fn on_progress(&self, p: PhotoProgress) {
            if p.stage == "write" {
                self.0.wait();
                self.0.wait();
            }
        }
    }
    let (dir, engine, ids) = fixture();
    let listener = Arc::new(Paused(std::sync::Barrier::new(2)));
    let job = engine
        .clone()
        .photo_merge(
            ids,
            MergeOptions {
                exposure_values: vec![1., 4.],
                ..Default::default()
            },
            listener.clone(),
        )
        .unwrap();
    listener.0.wait();
    job.cancel();
    listener.0.wait();
    let status = job.wait();
    assert_eq!(status.state, PhotoJobState::Cancelled);
    assert!(status.outputs.is_empty());
    assert_eq!(engine.list_images(ImageQuery::default()).unwrap().len(), 2);
    assert_eq!(
        std::fs::read_dir(dir.path().join("photos"))
            .unwrap()
            .count(),
        2
    );
}

#[test]
fn invalid_options_ids_missing_exposure_and_overlap_are_visible() {
    let (_dir, engine, ids) = fixture();
    for options in [
        MergeOptions {
            boundary_warp: 101,
            ..Default::default()
        },
        MergeOptions {
            exposure_values: vec![f64::NAN, 1.],
            ..Default::default()
        },
        MergeOptions {
            kind: MergeKind::HdrPanorama,
            ..Default::default()
        },
        MergeOptions {
            kind: MergeKind::Panorama,
            projection: MergeProjection::Cylindrical,
            ..Default::default()
        },
    ] {
        assert!(engine.merge_preview(ids.clone(), options).is_err());
    }
    assert!(
        engine
            .merge_preview(vec![ids[0].clone(); 2], Default::default())
            .is_err()
    );
    let missing = engine
        .merge_preview(ids.clone(), Default::default())
        .unwrap();
    assert!(missing.bytes.is_none());
    assert!(missing.warnings.iter().any(|w| w.contains("exposure")));
    let overlap = engine
        .merge_preview(
            ids,
            MergeOptions {
                kind: MergeKind::Panorama,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(overlap.bytes.is_none());
    assert!(!overlap.warnings.is_empty());
}

#[test]
fn panorama_and_explicit_hdr_panorama_publish_with_each_projection() {
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("photos");
    std::fs::create_dir(&photos).unwrap();
    for (i, (offset, gain)) in [(0., 0.25), (0., 1.), (85., 0.25), (85., 1.)]
        .into_iter()
        .enumerate()
    {
        let image = merge::LinearImage {
            width: 240,
            height: 160,
            pixels: (0..240 * 160)
                .map(|i| {
                    let x = (i % 240) as f64 + offset;
                    let y = (i / 240) as f64;
                    let v = (0.7
                        + 0.3 * (x * 0.17 + y * 0.13).sin()
                        + 0.2 * (x * 0.31 - y * 0.23).cos()
                        + 0.2 * ((x * 0.11).sin() * 7. + y * 0.19).cos())
                        as f32;
                    [v, v * 0.8, v * 0.7].map(|v| (v * gain).min(1.))
                })
                .collect(),
            color_matrix: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            as_shot_neutral: [1.; 3],
        };
        merge::write_dng(
            &mut std::fs::File::create(photos.join(format!("view{i}.dng"))).unwrap(),
            &image,
            &Default::default(),
        )
        .unwrap();
    }
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into()).unwrap();
    engine
        .index_folder(photos.to_string_lossy().into())
        .unwrap();
    let mut rows = engine.list_images(ImageQuery::default()).unwrap();
    rows.sort_by(|a, b| a.path.cmp(&b.path));
    let ids: Vec<_> = rows.iter().map(|r| r.id.clone()).collect();
    for projection in [
        MergeProjection::Perspective,
        MergeProjection::Cylindrical,
        MergeProjection::Spherical,
    ] {
        let options = MergeOptions {
            kind: MergeKind::Panorama,
            projection,
            focal_pixels: Some(300.),
            create_stack: false,
            fill_edges: true,
            ..Default::default()
        };
        let selected = vec![ids[0].clone(), ids[2].clone()];
        let preview = engine
            .merge_preview(selected.clone(), options.clone())
            .unwrap();
        assert!(preview.bytes.is_some(), "{:?}", preview.warnings);
        let status = engine
            .clone()
            .photo_merge(selected, options, Arc::new(Listener::default()))
            .unwrap()
            .wait();
        assert_eq!(status.state, PhotoJobState::Completed, "{:?}", status.error);
        assert!(
            engine
                .photo_stack(status.outputs[0].image_id.clone())
                .unwrap()
                .is_empty()
        );
    }
    let status = engine
        .clone()
        .photo_merge(
            ids,
            MergeOptions {
                kind: MergeKind::HdrPanorama,
                bracket_sizes: vec![2, 2],
                exposure_values: vec![0.25, 1., 0.25, 1.],
                auto_align: false,
                deghost: MergeDeghost::None,
                ..Default::default()
            },
            Arc::new(Listener::default()),
        )
        .unwrap()
        .wait();
    assert_eq!(status.state, PhotoJobState::Completed, "{:?}", status.error);
    assert!(status.outputs[0].path.ends_with("view0-HDR-Pano.dng"));
}

fn fixture() -> (tempfile::TempDir, Arc<Engine>, Vec<String>) {
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("photos");
    std::fs::create_dir(&photos).unwrap();
    for (i, value) in [0.15, 0.6].into_iter().enumerate() {
        let image = merge::LinearImage {
            width: 16,
            height: 12,
            pixels: vec![[value; 3]; 192],
            color_matrix: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            as_shot_neutral: [1.; 3],
        };
        merge::write_dng(
            &mut std::fs::File::create(photos.join(format!("frame{i}.dng"))).unwrap(),
            &image,
            &Default::default(),
        )
        .unwrap();
    }
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into()).unwrap();
    engine
        .index_folder(photos.to_string_lossy().into())
        .unwrap();
    let mut images = engine.list_images(ImageQuery::default()).unwrap();
    images.sort_by(|a, b| a.path.cmp(&b.path));
    (dir, engine, images.into_iter().map(|i| i.id).collect())
}

#[test]
fn unstacked_merge_preserves_all_source_ids() {
    let (_dir, engine, ids) = fixture();
    let status = engine
        .clone()
        .photo_merge(
            ids.clone(),
            MergeOptions {
                auto_align: false,
                exposure_values: vec![1., 4.],
                create_stack: false,
                ..Default::default()
            },
            Arc::new(Listener::default()),
        )
        .unwrap()
        .wait();
    assert_eq!(status.state, PhotoJobState::Completed, "{:?}", status.error);
    assert_eq!(status.outputs[0].source_ids, ids);
    assert!(
        engine
            .photo_stack(status.outputs[0].image_id.clone())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn hdr_preview_and_job_publish_float_dng_index_stack_and_changes() {
    let (_dir, engine, ids) = fixture();
    let options = MergeOptions {
        auto_align: false,
        auto_tone: false,
        exposure_values: vec![1., 4.],
        create_stack: true,
        ..Default::default()
    };
    let preview = engine.merge_preview(ids.clone(), options.clone()).unwrap();
    assert!(preview.bytes.is_some());
    assert_eq!(engine.list_images(ImageQuery::default()).unwrap().len(), 2);
    let before = engine.changes_since(0).unwrap().sequence;
    let listener = Arc::new(Listener::default());
    let job = engine
        .clone()
        .photo_merge(ids.clone(), options, listener.clone())
        .unwrap();
    let status = job.wait();
    assert_eq!(status.state, PhotoJobState::Completed, "{:?}", status.error);
    assert_eq!(status.outputs.len(), 1);
    let output = &status.outputs[0];
    assert!(output.path.ends_with("frame0-HDR.dng"));
    let dng =
        raw_decode::linear_dng::read(&mut std::fs::File::open(&output.path).unwrap()).unwrap();
    assert_eq!((dng.width, dng.height), (16, 12));
    assert!((dng.pixels[0][0] - 0.15).abs() < 1e-5);
    assert_eq!(engine.list_images(ImageQuery::default()).unwrap().len(), 3);
    let stack = engine.photo_stack(output.image_id.clone()).unwrap();
    assert_eq!(stack, [vec![output.image_id.clone()], ids].concat());
    assert!(
        engine
            .changes_since(before)
            .unwrap()
            .changes
            .iter()
            .any(|c| c.image_id == output.image_id && c.kind == LibraryChangeKind::Added)
    );
    let events = listener.0.lock().unwrap();
    assert_eq!(events.iter().filter(|p| p.stage == "completed").count(), 1);
}
