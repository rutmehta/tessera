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

#[test]
fn hnsw_top_five_matches_exact_for_seeded_thousand_vectors() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let mut approximate = HnswVectorIndex::open(dir.path(), "v1", 32)?;
    let mut seed = 0x1234_5678_u64;
    let mut random_vector = || -> Vec<f32> {
        (0..32)
            .map(|_| {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                ((seed >> 32) as u32 as f64 / u32::MAX as f64 * 2.0 - 1.0) as f32
            })
            .collect()
    };
    for id in 0..1000 {
        approximate.insert(ImageId(id), &random_vector())?;
    }
    let exact = SqliteVectorIndex::open(dir.path(), "v1", 32)?;
    for _ in 0..20 {
        let query = random_vector();
        let expected = exact.search(&query, 5)?;
        let actual = approximate.search(&query, 5)?;
        assert_eq!(
            actual.iter().map(|x| x.0).collect::<Vec<_>>(),
            expected.iter().map(|x| x.0).collect::<Vec<_>>()
        );
        for (a, b) in actual.iter().zip(&expected) {
            assert!((a.1 - b.1).abs() < 1e-5);
        }
    }
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
