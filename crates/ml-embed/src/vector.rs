//! Model-partitioned, normalized cosine vector indexes.
use std::path::Path;

use anyhow::{Context, Result, ensure};
use engine_api::id::ImageId;
use hnsw_rs::prelude::{DistCosine, Hnsw};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::HashMap;
use std::sync::Mutex;

/// SQLite is the durable source of truth; the in-memory HNSW graph is rebuilt
/// on open and on replacement (hnsw_rs does not support removing old points).
/// Use one writer per model; reopen after writes from another index handle.
///
/// HNSW search walks the bottom layer from wherever the upper layers lead, so
/// a point that link pruning cut off from the graph's main strongly connected
/// component may not be found at any `ef` (hnsw_rs draws graph levels from
/// the OS RNG, so which points are cut off differs per build). Search
/// therefore also scores those points exactly; the set is recomputed lazily
/// after writes.
pub struct HnswVectorIndex {
    store: SqliteVectorIndex,
    graph: Hnsw<'static, f32, DistCosine>,
    ids: Vec<ImageId>,
    slots: HashMap<ImageId, usize>,
    /// Outside the main component; `None` when stale.
    isolated: Mutex<Option<Isolated>>,
}

/// `(slot, normalized vector)` of each point outside the graph's main
/// component.
type Isolated = Vec<(usize, Vec<f32>)>;

fn new_graph(max_elements: usize) -> Hnsw<'static, f32, DistCosine> {
    let mut graph = Hnsw::new(32, max_elements, 16, 200, DistCosine {});
    // Keep pruned candidates to fill neighbour lists: fewer points end up
    // outside the main component.
    graph.set_keeping_pruned(true);
    graph
}

impl HnswVectorIndex {
    pub fn open(index_dir: &Path, model: &str, dimension: usize) -> Result<Self> {
        Self::from_store(SqliteVectorIndex::open(index_dir, model, dimension)?)
    }

    fn from_store(store: SqliteVectorIndex) -> Result<Self> {
        let mut index = Self {
            store,
            graph: new_graph(0),
            ids: Vec::new(),
            slots: HashMap::new(),
            isolated: Mutex::new(None),
        };
        index.rebuild()?;
        Ok(index)
    }

    fn rebuild(&mut self) -> Result<()> {
        let rows = self.store.rows()?;
        self.graph = new_graph(rows.len());
        self.ids.clear();
        self.slots.clear();
        *self.isolated.get_mut().unwrap() = None;
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

    /// Bottom-layer adjacency by slot (hnsw_rs data ids are our slots).
    fn adjacency(&self) -> Vec<Vec<usize>> {
        let mut adjacency = vec![Vec::new(); self.ids.len()];
        for point in self.graph.get_point_indexation() {
            let neighbours = point.get_neighborhood_id();
            if let (Some(out), Some(layer0)) =
                (adjacency.get_mut(point.get_origin_id()), neighbours.first())
            {
                *out = layer0.iter().map(|n| n.d_id).collect();
            }
        }
        adjacency
    }

    /// The points outside the main component, computed on first use after a
    /// write.
    fn isolated(&self) -> Result<std::sync::MutexGuard<'_, Option<Isolated>>> {
        let mut isolated = self.isolated.lock().unwrap();
        if isolated.is_none() {
            let mut points = Vec::new();
            for slot in outside_largest_component(&self.adjacency()) {
                if let Some(vector) = self.store.get(self.ids[slot])? {
                    points.push((slot, vector));
                }
            }
            *isolated = Some(points);
        }
        Ok(isolated)
    }
}

/// Nodes outside the largest strongly connected component of a directed
/// graph given as adjacency lists (Kosaraju, iterative), in ascending order.
/// Out-of-range targets are ignored.
fn outside_largest_component(adjacency: &[Vec<usize>]) -> Vec<usize> {
    let n = adjacency.len();
    // Pass 1: finish order on the graph.
    let mut visited = vec![false; n];
    let mut order = Vec::with_capacity(n);
    for start in 0..n {
        if visited[start] {
            continue;
        }
        visited[start] = true;
        let mut stack = vec![(start, 0)];
        while let Some((node, next)) = stack.last_mut() {
            if let Some(&child) = adjacency[*node].get(*next) {
                *next += 1;
                if child < n && !visited[child] {
                    visited[child] = true;
                    stack.push((child, 0));
                }
            } else {
                order.push(*node);
                stack.pop();
            }
        }
    }
    // Pass 2: components on the transpose, in reverse finish order.
    let mut transpose = vec![Vec::new(); n];
    for (from, targets) in adjacency.iter().enumerate() {
        for &to in targets.iter().filter(|&&to| to < n) {
            transpose[to].push(from);
        }
    }
    let mut component = vec![usize::MAX; n];
    let mut sizes = Vec::new();
    for &root in order.iter().rev() {
        if component[root] != usize::MAX {
            continue;
        }
        let id = sizes.len();
        component[root] = id;
        let mut size = 0;
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            size += 1;
            for &next in &transpose[node] {
                if component[next] == usize::MAX {
                    component[next] = id;
                    stack.push(next);
                }
            }
        }
        sizes.push(size);
    }
    // Ties go to the first component found.
    let Some(largest) = (0..sizes.len()).max_by_key(|&c| (sizes[c], std::cmp::Reverse(c))) else {
        return Vec::new();
    };
    (0..n).filter(|&node| component[node] != largest).collect()
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
            // Inserting prunes existing neighbour lists.
            *self.isolated.get_mut().unwrap() = None;
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
        let mut found = std::collections::HashSet::with_capacity(k);
        for neighbour in self.graph.search(&query, k, ef) {
            let id = self.ids[neighbour.d_id];
            if let Some(vector) = self.store.get(id)? {
                found.insert(neighbour.d_id);
                scores.push((id, cosine(&query, &vector)));
            }
        }
        // Points the graph walk cannot reach are scored exactly, then the
        // union is ranked by exact cosine.
        for (slot, vector) in self.isolated()?.iter().flatten() {
            if !found.contains(slot) {
                scores.push((self.ids[*slot], cosine(&query, vector)));
            }
        }
        scores.sort_unstable_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.0.cmp(&b.0.0)));
        scores.truncate(k);
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

#[cfg(test)]
mod tests {
    use super::outside_largest_component;

    #[test]
    fn nodes_outside_the_largest_strongly_connected_component() {
        // 0 -> 1 -> 2 -> 0 is the main cycle. 3 points into it but nothing
        // reaches 3; 4 is reachable but a dead end; 5 <-> 6 is a smaller
        // island reached from the cycle with no way back (9 is out of range).
        let adjacency = vec![
            vec![1],
            vec![2, 4],
            vec![0, 5],
            vec![0],
            vec![],
            vec![6],
            vec![5, 9],
        ];
        assert_eq!(outside_largest_component(&adjacency), [3, 4, 5, 6]);
        assert!(outside_largest_component(&[]).is_empty());
        assert!(outside_largest_component(&[vec![0]]).is_empty());
        // Of two equal components one is kept and the other reported.
        assert_eq!(
            outside_largest_component(&[vec![1], vec![0], vec![3], vec![2]]).len(),
            2
        );
    }
}
