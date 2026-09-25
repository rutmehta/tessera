//! Known-vector policy tests, not model-inference tests.
use cull::{CullSession, GroupingOptions, GroupingStrategy};
use engine_api::{EngineResult, id::ImageId};
use image::{Rgb, RgbImage};
use index::{Index, Metadata, MetadataProvider, NoopSidecarReader, Query};
use ml_embed::{
    EmbeddingGrouping,
    vector::{SqliteVectorIndex, VectorIndex},
};
use previews::{Codec, Jpeg};
use std::path::Path;

// Raw test index allows non-normalized and corrupt vectors; no inference is simulated.
struct RawVectors(Vec<(ImageId, Vec<f32>)>);
impl VectorIndex for RawVectors {
    fn insert(&mut self, id: ImageId, vector: &[f32]) -> anyhow::Result<()> {
        self.0.push((id, vector.to_vec()));
        Ok(())
    }
    fn search(&self, _: &[f32], _: usize) -> anyhow::Result<Vec<(ImageId, f32)>> {
        unreachable!("grouping must only snapshot by ID")
    }
    fn get(&self, id: ImageId) -> anyhow::Result<Option<Vec<f32>>> {
        Ok(self
            .0
            .iter()
            .find(|(key, _)| *key == id)
            .map(|(_, v)| v.clone()))
    }
}

#[test]
fn disabling_near_duplicates_uses_only_capture_proximity() {
    let (_dir, index) = fixtures();
    let mut session = CullSession::open(&index, Query::default()).unwrap();
    let ids = session.images().to_vec();
    let store = RawVectors(vec![(ids[0], vec![1., 0.]), (ids[1], vec![0., 1.])]);
    session.set_grouping_strategy(Box::new(
        EmbeddingGrouping::from_index(&store, &ids).unwrap(),
    ));
    session
        .regroup(GroupingOptions {
            near_duplicates: false,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(session.groups().len(), 1);
}

#[test]
fn snapshot_rejects_corrupt_or_inconsistent_vectors() {
    for bad in [
        vec![],
        vec![0., 0.],
        vec![f32::NAN, 1.],
        vec![1., f32::INFINITY],
        vec![1.],
        vec![1., 0., 0.],
    ] {
        let store = RawVectors(vec![(ImageId(1), vec![1., 0.]), (ImageId(2), bad)]);
        assert!(EmbeddingGrouping::from_index(&store, &[ImageId(1), ImageId(2)]).is_err());
    }
}

#[test]
fn policy_matrix_normalizes_vectors_and_requires_supporting_evidence() {
    let (_dir, index) = fixtures();
    let ids = index.search(&Query::default()).unwrap();
    let mut a = index.image_info(ids[0]).unwrap();
    let mut b = index.image_info(ids[1]).unwrap();
    for (va, vb, semantic) in [
        (Some(vec![2., 0.]), Some(vec![3., 0.]), Some(true)),
        (Some(vec![2., 0.]), Some(vec![0., 3.]), Some(false)),
        (
            Some(vec![1., 0.]),
            Some(vec![0.921, (1.0_f32 - 0.921_f32.powi(2)).sqrt()]),
            Some(true),
        ),
        (
            Some(vec![1., 0.]),
            Some(vec![0.919, (1.0_f32 - 0.919_f32.powi(2)).sqrt()]),
            Some(false),
        ),
        (
            Some(vec![f32::MAX, 0.]),
            Some(vec![f32::MIN_POSITIVE, 0.]),
            Some(true),
        ),
        (Some(vec![1., 0.]), None, None),
        (None, Some(vec![1., 0.]), None),
        (None, None, None),
    ] {
        let store = RawVectors(
            [(a.id, va), (b.id, vb)]
                .into_iter()
                .filter_map(|(id, v)| v.map(|v| (id, v)))
                .collect(),
        );
        let strategy = EmbeddingGrouping::from_index(&store, &ids).unwrap();
        for ta in [None, Some(100.)] {
            for tb in [
                None,
                Some(102.),
                Some(102.001),
                Some(f64::NAN),
                Some(f64::INFINITY),
            ] {
                a.capture_seconds = ta;
                b.capture_seconds = tb;
                let time = ta.zip(tb).is_some_and(|(a, b)| (a - b).abs() <= 2.);
                for ha in [None, Some(0)] {
                    for hb in [None, Some(63), Some(127)] {
                        let hash = ha
                            .zip(hb)
                            .is_some_and(|(a, b): (u64, u64)| (a ^ b).count_ones() <= 6);
                        for enabled in [true, false] {
                            let options = GroupingOptions {
                                near_duplicates: enabled,
                                ..Default::default()
                            };
                            let expected = if !enabled {
                                time
                            } else {
                                semantic.map_or(time && hash, |similar| similar && (time || hash))
                            };
                            assert_eq!(strategy.related(&a, &b, ha, hb, options), expected);
                            assert_eq!(strategy.related(&b, &a, hb, ha, options), expected);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn missing_vectors_use_real_jpeg_hashes_conservatively() {
    let (dir, index) = fixtures();
    let mut session = CullSession::open(&index, Query::default()).unwrap();
    let ids = session.images().to_vec();
    let strategy = EmbeddingGrouping::from_index(&RawVectors(vec![]), &ids).unwrap();
    session.set_grouping_strategy(Box::new(strategy));
    session.regroup(GroupingOptions::default()).unwrap();
    assert_eq!(session.groups().len(), 1);
    let image = Jpeg
        .decode(&std::fs::read(&index.image_info(ids[2]).unwrap().path).unwrap())
        .unwrap();
    std::fs::write(
        &index.image_info(ids[2]).unwrap().path,
        Jpeg.encode(&image::imageops::flip_horizontal(&image))
            .unwrap(),
    )
    .unwrap();
    session.regroup(GroupingOptions::default()).unwrap();
    assert_eq!(session.groups().len(), 2);
    assert_eq!(session.groups()[0].images, [ids[0], ids[1]]);
    assert_eq!(session.groups()[1].images, [ids[2]]);
    assert!(session.preview_errors().is_empty());
    drop(dir);
}

struct Times;
impl MetadataProvider for Times {
    fn read(&self, _: &Path) -> EngineResult<Metadata> {
        Ok(Metadata {
            capture_time: Some("2026-09-01T12:00:00".into()),
            ..Default::default()
        })
    }
}

fn fixtures() -> (tempfile::TempDir, Index) {
    let dir = tempfile::tempdir().unwrap();
    for n in 0..3 {
        let image = RgbImage::from_fn(90, 80, |x, y| Rgb([(20 + x + y / 4 + n) as u8; 3]));
        std::fs::write(
            dir.path().join(format!("{n}.jpg")),
            Jpeg.encode(&image).unwrap(),
        )
        .unwrap();
    }
    let mut index = Index::open(":memory:").unwrap();
    index.scan(dir.path(), &NoopSidecarReader, &Times).unwrap();
    (dir, index)
}

#[test]
fn real_cull_session_uses_snapshot_to_gate_jpeg_near_duplicates() {
    let (dir, index) = fixtures();
    let mut session = CullSession::open(&index, Query::default()).unwrap();
    let ids = session.images().to_vec();
    assert_eq!(session.groups().len(), 1);
    let mut store = SqliteVectorIndex::open(dir.path(), "known-vectors", 2).unwrap();
    store.insert(ids[0], &[2., 0.]).unwrap();
    store.insert(ids[1], &[3., 0.]).unwrap();
    store.insert(ids[2], &[0., 4.]).unwrap();
    let strategy = EmbeddingGrouping::from_index(&store, &ids).unwrap();
    // Mutating the store must not alter an installed snapshot.
    store.insert(ids[1], &[0., 1.]).unwrap();
    drop(store);
    session.set_grouping_strategy(Box::new(strategy));
    session.regroup(GroupingOptions::default()).unwrap();
    assert_eq!(session.groups().len(), 2);
    assert_eq!(session.groups()[0].images, [ids[0], ids[1]]);
    assert_eq!(session.groups()[1].images, [ids[2]]);
    assert!(session.preview_errors().is_empty());
    for id in ids {
        assert_eq!(
            session.selection(id).unwrap().decision,
            cull::Decision::Undecided
        );
    }
}
