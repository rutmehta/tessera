use engine_api::id::ImageId;
use ml_embed::vector::{AutoVectorIndex, HnswVectorIndex, SqliteVectorIndex, VectorIndex};

#[test]
fn auto_selects_and_promotes_only_above_fifty_thousand() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    drop(SqliteVectorIndex::open(dir.path(), "v1", 2)?);
    let mut connection = rusqlite::Connection::open(dir.path().join("embeddings.sqlite"))?;
    let transaction = connection.transaction()?;
    {
        let mut statement =
            transaction.prepare("INSERT INTO embedding(model,id,vector) VALUES ('v1',?1,?2)")?;
        for id in 0..50_000_u128 {
            let angle = id as f32 * 0.12345;
            let bytes: Vec<u8> = [angle.cos(), angle.sin()]
                .iter()
                .flat_map(|x| x.to_le_bytes())
                .collect();
            statement.execute(rusqlite::params![id.to_be_bytes().as_slice(), bytes])?;
        }
    }
    transaction.commit()?;
    let mut index = AutoVectorIndex::open(dir.path(), "v1", 2)?;
    assert!(!index.is_hnsw());
    index.insert(ImageId(0), &[1.0, 0.0])?;
    assert!(!index.is_hnsw());
    index.insert(ImageId(50_000), &[1.0, 0.0])?;
    assert!(index.is_hnsw());
    assert_eq!(index.rows()?.len(), 50_001);
    assert_eq!(index.get(ImageId(50_000))?, Some(vec![1.0, 0.0]));
    assert_eq!(index.search(&[1.0, 0.0], 5)?.len(), 5);
    drop(index);
    assert!(AutoVectorIndex::open(dir.path(), "v1", 2)?.is_hnsw());
    assert!(!AutoVectorIndex::open(dir.path(), "v2", 2)?.is_hnsw());
    Ok(())
}

/// A deterministic stream of 32-dimensional vectors (only the data is seeded:
/// hnsw_rs draws graph levels from the OS RNG, so every run builds a different
/// graph).
fn lcg_vectors() -> impl FnMut() -> Vec<f32> {
    let mut seed = 0x1234_5678_u64;
    move || {
        (0..32)
            .map(|_| {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                ((seed >> 32) as u32 as f64 / u32::MAX as f64 * 2.0 - 1.0) as f32
            })
            .collect()
    }
}

fn thousand_vector_indexes(
    dir: &std::path::Path,
    random_vector: &mut impl FnMut() -> Vec<f32>,
) -> anyhow::Result<(HnswVectorIndex, SqliteVectorIndex)> {
    let mut approximate = HnswVectorIndex::open(dir, "v1", 32)?;
    for id in 0..1000 {
        approximate.insert(ImageId(id), &random_vector())?;
    }
    Ok((approximate, SqliteVectorIndex::open(dir, "v1", 32)?))
}

#[test]
fn hnsw_top_five_recall_on_a_thousand_random_vectors() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let mut random_vector = lcg_vectors();
    let (approximate, exact) = thousand_vector_indexes(dir.path(), &mut random_vector)?;
    for _ in 0..20 {
        let query = random_vector();
        let expected = exact.search(&query, 5)?;
        let actual = approximate.search(&query, 5)?;
        assert_eq!(actual.len(), 5);
        // Both indexes rank the same stored vectors by the same cosine and
        // tie-break, so the exact nearest neighbour, once found, is first.
        // HNSW is approximate: graph construction may omit at most one
        // lower-ranked neighbour.
        assert_eq!(actual[0].0, expected[0].0, "{actual:?} vs {expected:?}");
        let overlap = actual
            .iter()
            .filter(|(id, _)| expected.iter().any(|(exact_id, _)| id == exact_id))
            .count();
        assert!(overlap >= 4, "top-five recall was {overlap}/5");
        for window in actual.windows(2) {
            assert!(window[0].1 >= window[1].1);
        }
        for (id, score) in &actual {
            if let Some((_, exact_score)) = expected.iter().find(|(exact_id, _)| exact_id == id) {
                assert!((score - exact_score).abs() < 1e-5);
            }
        }
    }
    Ok(())
}

/// Photo embeddings are clustered (bursts, near-duplicates), the case HNSW
/// link pruning handles worst: 200 clusters of 20 points with 5% noise, and
/// 100 queries near random cluster centres. The exact nearest neighbour must
/// be in the approximate top-5 for at least 96 of them (600 runs without
/// keep_pruned: 100 in 598, 99 once, 98 once; with it: 85 to 99, below 96 in
/// about 88% of runs).
#[test]
fn hnsw_recall_on_clustered_vectors() -> anyhow::Result<()> {
    const DIMENSION: usize = 64;
    let dir = tempfile::tempdir()?;
    let mut seed = 0x9e37_79b9_u64;
    let mut uniform = move || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        ((seed >> 32) as u32 as f64 / u32::MAX as f64 * 2.0 - 1.0) as f32
    };
    let centres: Vec<Vec<f32>> = (0..200)
        .map(|_| (0..DIMENSION).map(|_| uniform()).collect())
        .collect();
    let mut near =
        |centre: &[f32]| -> Vec<f32> { centre.iter().map(|c| c + 0.05 * uniform()).collect() };
    let mut approximate = HnswVectorIndex::open(dir.path(), "v1", DIMENSION)?;
    for (cluster, centre) in centres.iter().enumerate() {
        for member in 0..20 {
            approximate.insert(ImageId((cluster * 20 + member) as u128), &near(centre))?;
        }
    }
    let exact = SqliteVectorIndex::open(dir.path(), "v1", DIMENSION)?;
    let mut found = 0;
    for query in 0..100 {
        let query = near(&centres[(query * 37) % centres.len()]);
        let expected = exact.search(&query, 1)?;
        let actual = approximate.search(&query, 5)?;
        if actual.iter().any(|(id, _)| *id == expected[0].0) {
            found += 1;
        }
    }
    eprintln!("clustered recall: exact top-1 in the top-5 for {found} of 100 queries");
    assert!(
        found >= 96,
        "exact top-1 in the top-5 for {found} of 100 queries"
    );
    Ok(())
}

#[test]
fn hnsw_rebuilds_replacements_and_clamps_unbounded_search() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let mut index = HnswVectorIndex::open(dir.path(), "v1", 2)?;
    assert!(index.search(&[1.0, 0.0], usize::MAX)?.is_empty());
    for id in 0..12 {
        index.insert(ImageId(id), &[1.0, id as f32])?;
    }
    index.insert(ImageId(0), &[-1.0, 0.0])?;
    assert_eq!(index.get(ImageId(0))?, Some(vec![-1.0, 0.0]));
    assert_eq!(index.rows()?.len(), 12);
    assert!(index.insert(ImageId(0), &[f32::NAN, 1.0]).is_err());
    assert!(index.search(&[0.0, 0.0], 2).is_err());
    assert!(index.search(&[1.0, 0.0], 0)?.is_empty());
    let expected = SqliteVectorIndex::open(dir.path(), "v1", 2)?.search(&[1.0, 0.0], usize::MAX)?;
    assert_eq!(index.search(&[1.0, 0.0], usize::MAX)?, expected);
    drop(index);
    let reopened = HnswVectorIndex::open(dir.path(), "v1", 2)?;
    assert_eq!(reopened.search(&[1.0, 0.0], usize::MAX)?, expected);
    assert_eq!(reopened.search(&[-1.0, 0.0], 1)?[0].0, ImageId(0));
    assert!(
        HnswVectorIndex::open(dir.path(), "v2", 2)?
            .rows()?
            .is_empty()
    );
    Ok(())
}

#[test]
fn sqlite_rejects_invalid_shapes_and_partitions_model_dimensions() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    assert!(SqliteVectorIndex::open(dir.path(), "v1", 0).is_err());
    let mut index = SqliteVectorIndex::open(dir.path(), "v1", 2)?;
    assert!(SqliteVectorIndex::open(dir.path(), "v1", 3).is_err());
    for invalid in [
        vec![],
        vec![1.0],
        vec![0.0, 0.0],
        vec![f32::NAN, 1.0],
        vec![f32::INFINITY, 1.0],
    ] {
        assert!(index.insert(ImageId(1), &invalid).is_err());
        assert!(index.search(&invalid, 0).is_err());
    }
    index.insert(ImageId(1), &[f32::MAX, f32::MAX])?;
    index.insert(ImageId(1), &[0.0, 2.0])?;
    assert_eq!(index.rows()?.len(), 1);
    assert_eq!(index.get(ImageId(1))?, Some(vec![0.0, 1.0]));
    assert!(index.search(&[1.0, 0.0], 0)?.is_empty());
    let mut other = SqliteVectorIndex::open(dir.path(), "v2", 3)?;
    assert!(other.rows()?.is_empty());
    other.insert(ImageId(1), &[1.0, 0.0, 0.0])?;
    assert_eq!(index.get(ImageId(1))?, Some(vec![0.0, 1.0]));
    Ok(())
}

#[test]
fn sqlite_persists_normalized_vectors_and_ranks_cosine() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    {
        let mut index = SqliteVectorIndex::open(dir.path(), "v1", 2)?;
        index.insert(ImageId(1), &[3.0, 4.0])?;
        index.insert(ImageId(u128::MAX), &[-1.0, 0.0])?;
        assert_eq!(index.get(ImageId(1))?, Some(vec![0.6, 0.8]));
    }
    let index = SqliteVectorIndex::open(dir.path(), "v1", 2)?;
    let results = index.search(&[3.0, 4.0], usize::MAX)?;
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].0, ImageId(1));
    assert!((results[0].1 - 1.0).abs() < 1e-6);
    assert_eq!(index.rows()?.len(), 2);
    assert_eq!(index.get(ImageId(99))?, None);
    Ok(())
}
