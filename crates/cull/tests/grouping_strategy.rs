use cull::{CullSession, GroupingOptions, GroupingStrategy};
use engine_api::EngineResult;
use image::{Rgb, RgbImage};
use index::{ImageInfo, Index, Metadata, MetadataProvider, NoopSidecarReader, Query};
use previews::{Codec, Jpeg};
use std::path::Path;

struct Times;
impl MetadataProvider for Times {
    fn read(&self, path: &Path) -> EngineResult<Metadata> {
        let n = path.file_stem().unwrap().to_str().unwrap();
        Ok(Metadata {
            capture_time: Some(format!("2026-09-01T12:00:00.00{n}")),
            ..Default::default()
        })
    }
}

fn fixtures(count: usize) -> (tempfile::TempDir, Index) {
    let dir = tempfile::tempdir().unwrap();
    let base = RgbImage::from_fn(90, 80, |x, y| Rgb([(20 + x + y / 4) as u8; 3]));
    for n in 0..count {
        let image = if n % 2 == 0 {
            base.clone()
        } else {
            image::imageops::flip_horizontal(&base)
        };
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

// Test-only example of a downstream embedding policy; cull owns no ML state.
struct EmbeddingPolicy(std::collections::BTreeMap<cull::ImageId, [f64; 2]>);
impl GroupingStrategy for EmbeddingPolicy {
    fn related(
        &self,
        a: &ImageInfo,
        b: &ImageInfo,
        hash_a: Option<u64>,
        hash_b: Option<u64>,
        options: GroupingOptions,
    ) -> bool {
        let time = a
            .capture_seconds
            .zip(b.capture_seconds)
            .is_some_and(|(a, b)| (a - b).abs() <= options.burst_gap_seconds);
        let hash = options.near_duplicates
            && hash_a
                .zip(hash_b)
                .is_some_and(|(a, b)| (a ^ b).count_ones() <= 6);
        match (self.0.get(&a.id), self.0.get(&b.id)) {
            (Some(a), Some(b)) => {
                let dot = a[0] * b[0] + a[1] * b[1];
                let norm = |v: &[f64; 2]| (v[0] * v[0] + v[1] * v[1]).sqrt();
                dot / (norm(a) * norm(b)) >= 0.92 && (time || hash)
            }
            _ => time && hash,
        }
    }
}

#[test]
fn embedding_combination_policy_contract() {
    let (_dir, index) = fixtures(2);
    let ids = index.search(&Query::default()).unwrap();
    let mut a = index.image_info(ids[0]).unwrap();
    let mut b = index.image_info(ids[1]).unwrap();
    // Orthogonal and identical vectors, one missing, both missing, and either
    // side missing: semantic evidence gates rather than adds default edges.
    for (va, vb, semantic) in [
        (Some([1., 0.]), Some([1., 0.]), Some(true)),
        (Some([1., 0.]), Some([0., 1.]), Some(false)),
        (
            Some([1., 0.]),
            Some([0.92, (1.0_f64 - 0.92_f64.powi(2)).sqrt()]),
            Some(true),
        ),
        (
            Some([1., 0.]),
            Some([0.919, (1.0_f64 - 0.919_f64.powi(2)).sqrt()]),
            Some(false),
        ),
        (Some([1., 0.]), None, None),
        (None, Some([1., 0.]), None),
        (None, None, None),
    ] {
        let policy = EmbeddingPolicy(
            [(a.id, va), (b.id, vb)]
                .into_iter()
                .filter_map(|(id, v)| v.map(|v| (id, v)))
                .collect(),
        );
        for time in [true, false] {
            a.capture_seconds = Some(100.);
            b.capture_seconds = Some(if time { 102. } else { 103. });
            for distance in [6, 7] {
                for enabled in [true, false] {
                    let options = GroupingOptions {
                        near_duplicates: enabled,
                        ..Default::default()
                    };
                    let hash = enabled && distance == 6;
                    let expected =
                        semantic.map_or(time && hash, |similar| similar && (time || hash));
                    assert_eq!(
                        policy.related(&a, &b, Some(0), Some((1 << distance) - 1), options),
                        expected
                    );
                }
            }
        }
    }
}

#[test]
fn embedding_strategy_gates_real_jpeg_edges_and_falls_back_conservatively() {
    let (_dir, index) = fixtures(3);
    let mut session = CullSession::open(&index, Query::default()).unwrap();
    let ids = session.images().to_vec();
    assert_eq!(session.groups().len(), 1);
    // 0 and 2 have identical hashes but orthogonal vectors. 1 has no vector
    // and an opposite gradient: time alone must not join any of these pairs.
    session.set_grouping_strategy(Box::new(EmbeddingPolicy(
        [(ids[0], [1., 0.]), (ids[2], [0., 1.])].into(),
    )));
    session.regroup(GroupingOptions::default()).unwrap();
    assert_eq!(session.groups().len(), 3);
    session.set_grouping_strategy(Box::new(EmbeddingPolicy(Default::default())));
    session.regroup(GroupingOptions::default()).unwrap();
    assert_eq!(session.groups()[0].images, [ids[0], ids[2]]);
    assert_eq!(session.groups()[1].images, [ids[1]]);
    for id in ids {
        assert_eq!(
            session.selection(id).unwrap().decision,
            cull::Decision::Undecided
        );
    }
}

struct Edges(Vec<(cull::ImageId, cull::ImageId)>);
impl GroupingStrategy for Edges {
    fn related(
        &self,
        a: &ImageInfo,
        b: &ImageInfo,
        hash_a: Option<u64>,
        hash_b: Option<u64>,
        options: GroupingOptions,
    ) -> bool {
        assert!(!options.near_duplicates);
        assert_eq!(options.burst_gap_seconds, 7.);
        assert_eq!((hash_a, hash_b), (None, None));
        self.0.contains(&(a.id, b.id)) || self.0.contains(&(b.id, a.id))
    }
}

#[test]
fn strategy_components_and_members_follow_queue_order_transitively() {
    let (_dir, index) = fixtures(5);
    let mut session = CullSession::open(&index, Query::default()).unwrap();
    let ids = session.images().to_vec();
    session.set_position(3).unwrap();
    session.set_grouping_strategy(Box::new(Edges(vec![
        (ids[0], ids[4]),
        (ids[2], ids[4]),
        (ids[1], ids[3]),
    ])));
    for _ in 0..3 {
        session
            .regroup(GroupingOptions {
                burst_gap_seconds: 7.,
                near_duplicates: false,
            })
            .unwrap();
        assert_eq!(session.groups()[0].images, [ids[0], ids[2], ids[4]]);
        assert_eq!(session.groups()[1].images, [ids[1], ids[3]]);
        assert_eq!(session.groups().len(), 2);
        assert_eq!(session.images(), ids);
        assert_eq!(session.position(), Some(3));
    }
}

struct MissingHash(cull::ImageId);
impl GroupingStrategy for MissingHash {
    fn related(
        &self,
        a: &ImageInfo,
        b: &ImageInfo,
        hash_a: Option<u64>,
        hash_b: Option<u64>,
        _: GroupingOptions,
    ) -> bool {
        assert_eq!(hash_a.is_none(), a.id == self.0);
        assert_eq!(hash_b.is_none(), b.id == self.0);
        true
    }
}

#[test]
fn strategy_retains_preview_errors_and_option_validation() {
    let (_dir, index) = fixtures(3);
    let mut session = CullSession::open(&index, Query::default()).unwrap();
    let ids = session.images().to_vec();
    std::fs::write(index.image_info(ids[1]).unwrap().path, b"broken JPEG").unwrap();
    session.set_grouping_strategy(Box::new(MissingHash(ids[1])));
    session.regroup(GroupingOptions::default()).unwrap();
    assert_eq!(session.groups()[0].images, ids);
    assert_eq!(session.preview_errors().len(), 1);
    assert_eq!(session.preview_errors()[0].0, ids[1]);
    assert!(matches!(
        session.preview_errors()[0].1,
        engine_api::EngineError::Decode { .. }
    ));
    for gap in [-1., f64::NAN, f64::INFINITY] {
        assert!(
            session
                .regroup(GroupingOptions {
                    burst_gap_seconds: gap,
                    near_duplicates: true
                })
                .is_err()
        );
        assert_eq!(session.groups()[0].images, ids);
        assert_eq!(session.preview_errors().len(), 1);
    }
    session.set_grouping_strategy(Box::new(Unrelated));
    session
        .regroup(GroupingOptions {
            near_duplicates: false,
            ..Default::default()
        })
        .unwrap();
    assert!(session.preview_errors().is_empty());
}

struct Unrelated;
impl GroupingStrategy for Unrelated {
    fn related(
        &self,
        _: &ImageInfo,
        _: &ImageInfo,
        _: Option<u64>,
        _: Option<u64>,
        _: GroupingOptions,
    ) -> bool {
        false
    }
}

#[test]
fn strategy_replaces_time_only_edges_instead_of_augmenting_them() {
    let (_dir, index) = fixtures(2);
    let mut session = CullSession::open(&index, Query::default()).unwrap();
    assert_eq!(session.groups().len(), 1);
    session.set_grouping_strategy(Box::new(Unrelated));
    // Installing a strategy is inert until explicit regrouping.
    assert_eq!(session.groups().len(), 1);
    session.regroup(GroupingOptions::default()).unwrap();
    assert_eq!(session.groups().len(), 2);
    assert!(session.preview_errors().is_empty());
}
