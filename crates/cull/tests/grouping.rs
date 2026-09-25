use cull::{CullSession, Decision, GroupingOptions, Scorer, dhash};
use engine_api::EngineResult;
use image::{Rgb, RgbImage};
use index::{ImageInfo, Index, Metadata, MetadataProvider, NoopSidecarReader, Query};
use previews::{Codec, Jpeg};
use std::path::Path;

struct Times;
impl MetadataProvider for Times {
    fn read(&self, path: &Path) -> EngineResult<Metadata> {
        let n: usize = path.file_stem().unwrap().to_str().unwrap().parse().unwrap();
        let times = [
            "2026-09-01T23:59:59.000",
            "2026-09-02T00:00:01.000",
            "2026-09-02T00:00:03.001",
            "2026-09-02T00:01:00.000",
        ];
        Ok(Metadata {
            capture_time: Some(times[n].into()),
            ..Default::default()
        })
    }
}
#[test]
fn bursts_navigation_best_and_one_step_undo() {
    let dir = tempfile::tempdir().unwrap();
    for n in 0..4 {
        std::fs::write(dir.path().join(format!("{n}.jpg")), vec![0; n + 10]).unwrap();
    }
    let mut index = Index::open(":memory:").unwrap();
    index.scan(dir.path(), &NoopSidecarReader, &Times).unwrap();
    let mut session = CullSession::open(&index, Query::default()).unwrap();
    assert_eq!(
        session
            .groups()
            .iter()
            .map(|g| g.images.len())
            .collect::<Vec<_>>(),
        [2, 1, 1]
    );
    session.next_in_group();
    assert_eq!(session.position(), Some(1));
    session.next_in_group();
    assert_eq!(session.position(), Some(1));
    session.next_group();
    assert_eq!(session.position(), Some(2));
    session.prev_group();
    assert_eq!(session.position(), Some(0));
    session.keep_best_reject_rest(0).unwrap();
    assert_eq!(
        index
            .selection(session.images()[0])
            .unwrap()
            .unwrap()
            .decision,
        Decision::Reject
    );
    assert_eq!(
        index
            .selection(session.images()[1])
            .unwrap()
            .unwrap()
            .decision,
        Decision::Keep
    );
    assert!(session.undo().unwrap());
    for id in &session.images()[..2] {
        assert_eq!(
            index.selection(*id).unwrap().unwrap().decision,
            Decision::Undecided
        );
    }
    assert!(session.redo().unwrap());
    struct Smallest;
    impl Scorer for Smallest {
        fn score(&self, image: &ImageInfo) -> f64 {
            -(image.size as f64)
        }
    }
    session.set_scorer(Box::new(Smallest));
    session.keep_best_reject_rest(0).unwrap();
    assert_eq!(
        index
            .selection(session.images()[0])
            .unwrap()
            .unwrap()
            .decision,
        Decision::Keep
    );
    session
        .regroup(GroupingOptions {
            burst_gap_seconds: 3.,
            near_duplicates: false,
        })
        .unwrap();
    assert_eq!(session.groups()[0].images.len(), 3);
}

#[test]
fn dhash_groups_jpeg_brightness_and_blur_variants_not_opposite_gradient() {
    let base = RgbImage::from_fn(90, 80, |x, y| Rgb([(20 + x + y / 4) as u8; 3]));
    let bright = image::imageops::brighten(&base, 30);
    let blur = image::imageops::blur(&base, 1.5);
    let distinct = image::imageops::flip_horizontal(&base);
    assert!((dhash(&base) ^ dhash(&bright)).count_ones() <= 6);
    assert!((dhash(&base) ^ dhash(&blur)).count_ones() <= 6);
    assert!((dhash(&base) ^ dhash(&distinct)).count_ones() > 6);
    let dir = tempfile::tempdir().unwrap();
    for (n, image) in [base, bright, blur, distinct].iter().enumerate() {
        std::fs::write(
            dir.path().join(format!("{n}.jpg")),
            Jpeg.encode(image).unwrap(),
        )
        .unwrap();
    }
    let mut index = Index::open(":memory:").unwrap();
    index
        .scan(dir.path(), &NoopSidecarReader, &index::NoopMetadataProvider)
        .unwrap();
    let session = CullSession::open(&index, dir.path()).unwrap();
    let mut sizes: Vec<_> = session.groups().iter().map(|g| g.images.len()).collect();
    sizes.sort();
    assert_eq!(sizes, [1, 3]);
    assert!(session.preview_errors().is_empty());
}
