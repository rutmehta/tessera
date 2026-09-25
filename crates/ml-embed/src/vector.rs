//! Model-partitioned, normalized cosine vector indexes.
use std::path::Path;

use anyhow::{Context, Result, ensure};
use engine_api::id::ImageId;
use hnsw_rs::prelude::{DistCosine, Hnsw};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::HashMap;

/// SQLite is the durable source of truth; the in-memory HNSW graph is rebuilt
/// on open and on replacement (hnsw_rs does not support removing old points).
/// Use one writer per model; reopen after writes from another index handle.
pub struct HnswVectorIndex {
    store: SqliteVectorIndex,
    graph: Hnsw<'static, f32, DistCosine>,
    ids: Vec<ImageId>,
    slots: HashMap<ImageId, usize>,
}

impl HnswVectorIndex {
    pub fn open(index_dir: &Path, model: &str, dimension: usize) -> Result<Self> {
        Self::from_store(SqliteVectorIndex::open(index_dir, model, dimension)?)
    }

    fn from_store(store: SqliteVectorIndex) -> Result<Self> {
        let mut index = Self {
            store,
            graph: Hnsw::new(32, 0, 16, 200, DistCosine {}),
            ids: Vec::new(),
            slots: HashMap::new(),
        };
        index.rebuild()?;
        Ok(index)
    }

    fn rebuild(&mut self) -> Result<()> {
        let rows = self.store.rows()?;
        self.graph = Hnsw::new(32, rows.len(), 16, 200, DistCosine {});
        self.ids.clear();
        self.slots.clear();
        for (id, vector) in rows {
            let slot = self.ids.len();
            self.graph.insert((&vector, slot));
            self.ids.push(id);
            self.slots.insert(id, slot);
        }
        Ok(())
    }

    pub fn rows(&self) -> Result<Vec<(ImageId, Vec<f32>)>> {
        self.store.rows()
    }
}

impl VectorIndex for HnswVectorIndex {
    fn insert(&mut self, id: ImageId, vector: &[f32]) -> Result<()> {
        let vector = normalize(vector, self.store.dimension)?;
        self.store.insert(id, &vector)?;
        if self.slots.contains_key(&id) {
            self.rebuild()?;
        } else {
            let slot = self.ids.len();
            self.graph.insert((&vector, slot));
            self.ids.push(id);
            self.slots.insert(id, slot);
        }
        Ok(())
    }

    fn search(&self, query: &[f32], k: usize) -> Result<Vec<(ImageId, f32)>> {
        let query = normalize(query, self.store.dimension)?;
        let k = k.min(self.ids.len());
        if k == 0 {
            return Ok(Vec::new());
        }
        // Full enumeration supports facet filtering without ANN omissions, and
        // avoids passing usize::MAX into graph search allocation arithmetic.
        if k == self.ids.len() {
            return self.store.search(&query, k);
        }
        let ef = k.saturating_mul(4).max(256).min(self.ids.len());
        let mut scores = Vec::with_capacity(k);
        for neighbour in self.graph.search(&query, k, ef) {
            let id = self.ids[neighbour.d_id];
            if let Some(vector) = self.store.get(id)? {
                scores.push((id, cosine(&query, &vector)));
            }
        }
        scores.sort_unstable_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.0.cmp(&b.0.0)));
        Ok(scores)
    }

    fn get(&self, id: ImageId) -> Result<Option<Vec<f32>>> {
        self.store.get(id)
    }
}

/// Uses exact SQLite search through 50,000 rows in the selected model, then
/// promotes to HNSW, including when inserts cross the threshold.
pub struct AutoVectorIndex {
    backend: Backend,
    directory: std::path::PathBuf,
    model: String,
    dimension: usize,
}

enum Backend {
    Sqlite(SqliteVectorIndex),
    Hnsw(Box<HnswVectorIndex>),
}

impl AutoVectorIndex {
    pub fn open(index_dir: &Path, model: &str, dimension: usize) -> Result<Self> {
        let store = SqliteVectorIndex::open(index_dir, model, dimension)?;
        let backend = if store.count()? > 50_000 {
            Backend::Hnsw(Box::new(HnswVectorIndex::from_store(store)?))
        } else {
            Backend::Sqlite(store)
        };
        Ok(Self {
            backend,
            directory: index_dir.to_owned(),
            model: model.to_owned(),
            dimension,
        })
    }

    pub fn is_hnsw(&self) -> bool {
        matches!(self.backend, Backend::Hnsw(_))
    }

    pub fn rows(&self) -> Result<Vec<(ImageId, Vec<f32>)>> {
        match &self.backend {
            Backend::Sqlite(index) => index.rows(),
            Backend::Hnsw(index) => index.rows(),
        }
    }
}

impl VectorIndex for AutoVectorIndex {
    fn insert(&mut self, id: ImageId, vector: &[f32]) -> Result<()> {
        match &mut self.backend {
            Backend::Sqlite(index) => {
                index.insert(id, vector)?;
                if index.count()? > 50_000 {
                    self.backend = Backend::Hnsw(Box::new(HnswVectorIndex::open(
                        &self.directory,
                        &self.model,
                        self.dimension,
                    )?));
                }
                Ok(())
            }
            Backend::Hnsw(index) => index.insert(id, vector),
        }
    }

    fn search(&self, query: &[f32], k: usize) -> Result<Vec<(ImageId, f32)>> {
        match &self.backend {
            Backend::Sqlite(index) => index.search(query, k),
            Backend::Hnsw(index) => index.search(query, k),
        }
    }

    fn get(&self, id: ImageId) -> Result<Option<Vec<f32>>> {
        match &self.backend {
            Backend::Sqlite(index) => index.get(id),
            Backend::Hnsw(index) => index.get(id),
        }
    }
}

/// Scores are cosine similarities (larger is better).
pub trait VectorIndex {
    fn insert(&mut self, id: ImageId, vector: &[f32]) -> Result<()>;
    fn search(&self, query: &[f32], k: usize) -> Result<Vec<(ImageId, f32)>>;
    fn get(&self, id: ImageId) -> Result<Option<Vec<f32>>>;
}

pub struct SqliteVectorIndex {
    connection: Connection,
    model: String,
    dimension: usize,
}

impl SqliteVectorIndex {
    fn count(&self) -> Result<usize> {
        Ok(self.connection.query_row(
            "SELECT COUNT(*) FROM embedding WHERE model = ?1",
            [&self.model],
            |row| row.get::<_, i64>(0),
        )? as usize)
    }

    pub fn open(index_dir: &Path, model: &str, dimension: usize) -> Result<Self> {
        ensure!(
            dimension > 0 && dimension <= i64::MAX as usize,
            "invalid dimension"
        );
        std::fs::create_dir_all(index_dir)?;
        let connection = Connection::open(index_dir.join("embeddings.sqlite"))?;
        connection.execute_batch(
            "PRAGMA journal_mode=WAL;
            PRAGMA synchronous=NORMAL;
            CREATE TABLE IF NOT EXISTS embedding (
            model TEXT NOT NULL, id BLOB NOT NULL, vector BLOB NOT NULL,
            PRIMARY KEY(model, id));",
        )?;
        connection.execute_batch("CREATE TABLE IF NOT EXISTS embedding_models (model TEXT PRIMARY KEY, dimension INTEGER NOT NULL);")?;
        connection.execute(
            "INSERT OR IGNORE INTO embedding_models(model, dimension) VALUES (?1, ?2)",
            params![model, dimension as i64],
        )?;
        let stored: i64 = connection.query_row(
            "SELECT dimension FROM embedding_models WHERE model = ?1",
            [model],
            |row| row.get(0),
        )?;
        ensure!(stored == dimension as i64, "model dimension mismatch");
        Ok(Self {
            connection,
            model: model.to_owned(),
            dimension,
        })
    }

    pub fn rows(&self) -> Result<Vec<(ImageId, Vec<f32>)>> {
        let mut statement = self
            .connection
            .prepare("SELECT id, vector FROM embedding WHERE model = ?1 ORDER BY id")?;
        let rows = statement.query_map([&self.model], |row| {
            Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?;
        rows.map(|row| {
            let (id, vector) = row?;
            let id = ImageId(u128::from_be_bytes(
                id.try_into()
                    .map_err(|_| anyhow::anyhow!("invalid image id"))?,
            ));
            Ok((id, self.decode(&vector)?))
        })
        .collect()
    }

    fn decode(&self, bytes: &[u8]) -> Result<Vec<f32>> {
        ensure!(
            bytes.len() / 4 == self.dimension && bytes.len().is_multiple_of(4),
            "stored vector dimension mismatch"
        );
        Ok(bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|chunk| f32::from_le_bytes(*chunk))
            .collect())
    }
}

fn normalize(vector: &[f32], dimension: usize) -> Result<Vec<f32>> {
    ensure!(vector.len() == dimension, "vector dimension mismatch");
    let norm = vector
        .iter()
        .map(|&x| f64::from(x).powi(2))
        .sum::<f64>()
        .sqrt();
    ensure!(
        norm.is_finite() && norm > 0.0,
        "vector must be finite and nonzero"
    );
    Ok(vector
        .iter()
        .map(|&x| (f64::from(x) / norm) as f32)
        .collect())
}

// Contiguous slices and independent partial sums permit auto-vectorization.
fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let (a_chunks, a_tail) = a.as_chunks::<8>();
    let (b_chunks, b_tail) = b.as_chunks::<8>();
    let mut sums = [0.0_f32; 8];
    for (a, b) in a_chunks.iter().zip(b_chunks) {
        for lane in 0..8 {
            sums[lane] += a[lane] * b[lane];
        }
    }
    let tail: f32 = a_tail.iter().zip(b_tail).map(|(a, b)| a * b).sum();
    (sums.iter().sum::<f32>() + tail).clamp(-1.0, 1.0)
}

impl VectorIndex for SqliteVectorIndex {
    fn insert(&mut self, id: ImageId, vector: &[f32]) -> Result<()> {
        let vector = normalize(vector, self.dimension)?;
        let bytes: Vec<u8> = vector.iter().flat_map(|x| x.to_le_bytes()).collect();
        self.connection.execute(
            "INSERT INTO embedding(model, id, vector) VALUES (?1, ?2, ?3)
            ON CONFLICT(model, id) DO UPDATE SET vector = excluded.vector",
            params![self.model, id.0.to_be_bytes().as_slice(), bytes],
        )?;
        Ok(())
    }

    fn search(&self, query: &[f32], k: usize) -> Result<Vec<(ImageId, f32)>> {
        let query = normalize(query, self.dimension)?;
        let mut scores: Vec<_> = self
            .rows()?
            .into_iter()
            .map(|(id, vector)| (id, cosine(&query, &vector)))
            .collect();
        scores.sort_unstable_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.0.cmp(&b.0.0)));
        scores.truncate(k);
        Ok(scores)
    }

    fn get(&self, id: ImageId) -> Result<Option<Vec<f32>>> {
        let bytes: Option<Vec<u8>> = self
            .connection
            .query_row(
                "SELECT vector FROM embedding WHERE model = ?1 AND id = ?2",
                params![self.model, id.0.to_be_bytes().as_slice()],
                |row| row.get(0),
            )
            .optional()?;
        bytes
            .map(|bytes| self.decode(&bytes).context("decode embedding"))
            .transpose()
    }
}
