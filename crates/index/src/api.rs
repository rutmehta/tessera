//! Contract-facing catalog API. SQLite errors remain private.
use super::*;
use engine_api::error::{EngineError, EngineResult};

/// Minimal catalog facts needed for culling. Unknown capture times remain absent.
#[derive(Debug, Clone)]
pub struct ImageInfo {
    pub id: ImageId,
    pub path: std::path::PathBuf,
    pub size: u64,
    pub capture_seconds: Option<f64>,
}

/// Latest value for a named signal; model includes the producing model version.
#[derive(Debug, Clone, PartialEq)]
pub struct Score {
    pub signal: String,
    pub value: f64,
    pub model: String,
}

/// A face ordinal is local to an image, not a persistent person identity.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceRecord {
    pub id: u32,
    /// Image-pixel coordinates [x, y, width, height].
    pub bbox: [f32; 4],
    /// Five image-pixel landmark coordinates in detector order.
    pub landmarks5: [[f32; 2]; 5],
    pub confidence: f32,
    /// Optional finite 128-component SFace descriptor; not necessarily normalized.
    pub embedding: Option<Vec<f32>>,
    /// Normalized sharpness in [0, 1].
    pub sharpness: f64,
    /// Optional [0, 1] confidence/proxy, NOT a measured eyelid-closure signal.
    /// Five-point landmarks alone cannot establish whether eyes are open.
    pub eyes_open: Option<f64>,
}

fn sql_error(error: rusqlite::Error) -> EngineError {
    IndexError::Sql(error).into()
}

impl From<IndexError> for EngineError {
    fn from(error: IndexError) -> Self {
        match error {
            IndexError::Engine(error) => error,
            IndexError::Io(error) => error.into(),
            IndexError::Metadata(message) => Self::Decode {
                format: "index".into(),
                message,
            },
            error => Self::Io {
                path: None,
                message: error.to_string(),
            },
        }
    }
}

/// Rebuildable SQLite catalog. Each instance owns one connection.
#[derive(Debug)]
pub struct Index(Core);
impl Index {
    pub fn open(path: impl AsRef<Path>) -> EngineResult<Self> {
        Core::open(path).map(Self).map_err(Into::into)
    }
    pub fn scan(
        &mut self,
        root: impl AsRef<Path>,
        sidecars: &dyn SidecarReader,
        metadata: &dyn MetadataProvider,
    ) -> EngineResult<usize> {
        self.0.scan(root, sidecars, metadata).map_err(Into::into)
    }
    pub fn search(&self, query: &Query) -> EngineResult<Vec<ImageId>> {
        self.0.search(query).map_err(Into::into)
    }
    pub fn facets(&self, query: &Query) -> EngineResult<Facets> {
        self.0.facets(query).map_err(Into::into)
    }
    pub fn set_selection(&self, id: ImageId, selection: &Selection) -> EngineResult<()> {
        self.0.set_selection(id, selection).map_err(Into::into)
    }
    pub fn selection(&self, id: ImageId) -> EngineResult<Option<Selection>> {
        self.0.selection(id).map_err(Into::into)
    }
    pub fn image_info(&self, id: ImageId) -> EngineResult<ImageInfo> {
        self.0.conn.query_row(
            "SELECT f.path,f.size,unixepoch(i.capture_time,'subsec') FROM image i JOIN file f ON f.id=i.file_id WHERE i.id=?",
            [id.to_string()],
            |r| Ok(ImageInfo { id, path: r.get::<_, String>(0)?.into(), size: r.get::<_, i64>(1)?.max(0) as u64, capture_seconds: r.get(2)? }),
        ).optional().map_err(sql_error)?.ok_or_else(|| EngineError::not_found("image", id))
    }
    pub fn set_score(&self, id: ImageId, score: &Score) -> EngineResult<()> {
        if !score.value.is_finite() || score.signal.is_empty() || score.model.is_empty() {
            return Err(EngineError::invalid(
                "score",
                "requires finite value, signal and model",
            ));
        }
        self.0.conn.execute(
            "INSERT INTO score(image_id,signal,value,model) VALUES(?,?,?,?) ON CONFLICT(image_id,signal) DO UPDATE SET value=excluded.value,model=excluded.model",
            params![id.to_string(),score.signal,score.value,score.model],
        ).map_err(sql_error)?;
        Ok(())
    }
    pub fn scores(&self, id: ImageId) -> EngineResult<Vec<Score>> {
        let mut stmt = self
            .0
            .conn
            .prepare("SELECT signal,value,model FROM score WHERE image_id=? ORDER BY signal")
            .map_err(sql_error)?;
        stmt.query_map([id.to_string()], |r| {
            Ok(Score {
                signal: r.get(0)?,
                value: r.get(1)?,
                model: r.get(2)?,
            })
        })
        .map_err(sql_error)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(sql_error)
    }
    /// Atomically replaces the detected faces for an image.
    pub fn replace_faces(&self, id: ImageId, faces: &[FaceRecord]) -> EngineResult<()> {
        let mut ordinals = std::collections::HashSet::new();
        for face in faces {
            let [x, y, w, h] = face.bbox;
            if !ordinals.insert(face.id)
                || !face.bbox.iter().all(|v| v.is_finite() && *v >= 0.0)
                || w <= 0.0
                || h <= 0.0
                || !(x + w).is_finite()
                || !(y + h).is_finite()
                || !face
                    .landmarks5
                    .iter()
                    .flatten()
                    .all(|v| v.is_finite() && *v >= 0.0)
                || !(0.0..=1.0).contains(&face.confidence)
                || !(0.0..=1.0).contains(&face.sharpness)
                || face.eyes_open.is_some_and(|v| !(0.0..=1.0).contains(&v))
                || face
                    .embedding
                    .as_ref()
                    .is_some_and(|v| v.len() != 128 || !v.iter().all(|v| v.is_finite()))
            {
                return Err(EngineError::invalid(
                    "face",
                    "requires unique ordinals, finite nonnegative pixel coordinates, positive extent, 128 finite embedding components, and scores/confidence in [0,1]",
                ));
            }
        }
        let tx = self.0.conn.unchecked_transaction().map_err(sql_error)?;
        let image = id.to_string();
        let exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM image WHERE id=?)",
                [&image],
                |r| r.get(0),
            )
            .map_err(sql_error)?;
        if !exists {
            return Err(EngineError::not_found("image", id));
        }
        tx.execute("DELETE FROM face WHERE image_id=?", [&image])
            .map_err(sql_error)?;
        tx.execute("DELETE FROM score WHERE image_id=? AND (signal GLOB 'face/*' OR signal IN ('face_sharpness','eyes_open'))", [&image]).map_err(sql_error)?;
        for face in faces {
            tx.execute(
                "INSERT INTO face(image_id,id,bbox,landmarks5,confidence,embedding,model) VALUES(?,?,?,?,?,?,?)",
                params![image, face.id, serde_json::to_string(&face.bbox)?,
                    serde_json::to_string(&face.landmarks5)?, face.confidence,
                    face.embedding.as_ref().map(serde_json::to_string).transpose()?, "yunet-sface-v1"],
            ).map_err(sql_error)?;
            for (signal, value) in [
                ("sharpness", Some(face.sharpness)),
                ("eyes_open", face.eyes_open),
            ] {
                if let Some(value) = value {
                    tx.execute("INSERT INTO score(image_id,signal,value,model) VALUES(?,?,?,?) ON CONFLICT(image_id,signal) DO UPDATE SET value=excluded.value,model=excluded.model",
                        params![image, format!("face/{}/{signal}", face.id), value, "yunet-sface-v1"]).map_err(sql_error)?;
                }
            }
        }
        for (signal, value) in [
            (
                "face_sharpness",
                faces.iter().map(|f| f.sharpness).reduce(f64::min),
            ),
            (
                "eyes_open",
                faces.iter().filter_map(|f| f.eyes_open).reduce(f64::min),
            ),
        ] {
            if let Some(value) = value {
                tx.execute(
                    "INSERT INTO score(image_id,signal,value,model) VALUES(?,?,?,?)",
                    params![image, signal, value, "yunet-sface-v1"],
                )
                .map_err(sql_error)?;
            }
        }
        tx.commit().map_err(sql_error)
    }

    /// Returns faces in ascending local ordinal order; absent faces yield an empty list.
    pub fn faces(&self, id: ImageId) -> EngineResult<Vec<FaceRecord>> {
        let mut stmt = self.0.conn.prepare(
            "SELECT f.id,f.bbox,f.landmarks5,f.confidence,f.embedding,s.value,e.value FROM face f
             JOIN score s ON s.image_id=f.image_id AND s.signal='face/'||f.id||'/sharpness'
             LEFT JOIN score e ON e.image_id=f.image_id AND e.signal='face/'||f.id||'/eyes_open'
             WHERE f.image_id=? ORDER BY f.id"
        ).map_err(sql_error)?;
        let rows = stmt
            .query_map([id.to_string()], |r| {
                Ok((
                    r.get::<_, u32>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, f32>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, f64>(5)?,
                    r.get::<_, Option<f64>>(6)?,
                ))
            })
            .map_err(sql_error)?;
        rows.map(|row| {
            let (id, bbox, landmarks5, confidence, embedding, sharpness, eyes_open) =
                row.map_err(sql_error)?;
            Ok(FaceRecord {
                id,
                bbox: serde_json::from_str(&bbox)?,
                landmarks5: serde_json::from_str(&landmarks5)?,
                confidence,
                embedding: embedding.map(|s| serde_json::from_str(&s)).transpose()?,
                sharpness,
                eyes_open,
            })
        })
        .collect()
    }

    pub fn record_export(
        &self,
        id: ImageId,
        destination: &str,
        published: bool,
    ) -> EngineResult<()> {
        self.0
            .conn
            .execute(
                "INSERT INTO export_log(image_id,destination,published) VALUES(?,?,?)",
                params![id.to_string(), destination, published],
            )
            .map_err(sql_error)?;
        Ok(())
    }
    /// (ever exported, ever published). These are historical, not freshness flags.
    pub fn export_status(&self, id: ImageId) -> EngineResult<(bool, bool)> {
        self.0
            .conn
            .query_row(
                "SELECT count(*)>0,coalesce(max(published),0) FROM export_log WHERE image_id=?",
                [id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(sql_error)
    }
    pub fn add_keyword(&self, name: &str, parent: Option<&str>) -> EngineResult<i64> {
        self.0.add_keyword(name, parent).map_err(Into::into)
    }
    pub fn tag(&self, id: ImageId, keyword: &str) -> EngineResult<()> {
        self.0.tag(id, keyword).map_err(Into::into)
    }
    pub fn images_with_keyword(&self, name: &str) -> EngineResult<Vec<ImageId>> {
        self.0.images_with_keyword(name).map_err(Into::into)
    }
}

/// Scanner hooks receive image paths, not sidecar paths. XMP (both naming
/// conventions) and `.edits/<stem>.json` are tracked for invalidation.
pub struct Scanner<'a> {
    pub sidecars: &'a dyn SidecarReader,
    pub metadata: &'a dyn MetadataProvider,
}
impl<'a> Scanner<'a> {
    pub fn new(sidecars: &'a dyn SidecarReader, metadata: &'a dyn MetadataProvider) -> Self {
        Self { sidecars, metadata }
    }
    /// Returns the number of inserted or refreshed images.
    pub fn scan(&self, index: &mut Index, root: impl AsRef<Path>) -> EngineResult<usize> {
        index.scan(root, self.sidecars, self.metadata)
    }
}
