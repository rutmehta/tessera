//! Model-derived suggestions remain separate from user-accepted metadata.
use crate::Index;
use engine_api::id::ImageId;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

/// Latest model output and the version identifying its provenance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Understanding {
    pub model_version: String,
    pub keywords: Vec<KeywordSuggestion>,
    pub caption: String,
    pub alt_text: String,
    pub ocr: Vec<OcrRegion>,
}

/// A suggestion, not an accepted catalog keyword.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeywordSuggestion {
    pub keyword: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OcrRegion {
    pub text: String,
    /// Normalized [x0, y0, x1, y1] coordinates.
    pub bbox: [f32; 4],
    pub confidence: f32,
}

impl Index {
    /// Persist explicit keyword acceptance independently of imported sidecars.
    /// A rescan merges these local tags instead of silently dropping opt-in-only
    /// metadata that was deliberately not exported to XMP.
    pub fn accept_keyword_names(&self, images: &[ImageId], names: &[String]) -> anyhow::Result<()> {
        let tx = self.0.conn.unchecked_transaction()?;
        for &id in images {
            for name in names {
                let keyword: i64 =
                    tx.query_row("SELECT id FROM keyword WHERE name=?", [name], |r| r.get(0))?;
                tx.execute(
                    "INSERT OR IGNORE INTO accepted_keyword VALUES(?,?)",
                    params![id.to_string(), keyword],
                )?;
                tx.execute(
                    "INSERT OR IGNORE INTO image_keyword VALUES(?,?)",
                    params![id.to_string(), keyword],
                )?;
            }
            refresh_fts(&tx, &id.to_string())?;
        }
        tx.commit()?;
        Ok(())
    }
    /// Undo explicit acceptance: drops the local acceptance rows and the image's
    /// tags for `names`. A later sidecar rescan re-adds any keyword the XMP still
    /// carries, so callers removing a keyword should also remove it from XMP.
    pub fn forget_accepted_keyword_names(
        &self,
        images: &[ImageId],
        names: &[String],
    ) -> anyhow::Result<()> {
        let tx = self.0.conn.unchecked_transaction()?;
        for &id in images {
            for name in names {
                for table in ["accepted_keyword", "image_keyword"] {
                    tx.execute(
                        &format!(
                            "DELETE FROM {table} WHERE image_id=? AND keyword_id IN (SELECT id FROM keyword WHERE name=?)"
                        ),
                        params![id.to_string(), name],
                    )?;
                }
            }
            refresh_fts(&tx, &id.to_string())?;
        }
        tx.commit()?;
        Ok(())
    }
    /// Keywords explicitly accepted for `id` (sorted), independent of XMP.
    pub fn accepted_keyword_names(&self, id: ImageId) -> anyhow::Result<Vec<String>> {
        let mut stmt = self.0.conn.prepare(
            "SELECT k.name FROM accepted_keyword a JOIN keyword k ON k.id=a.keyword_id WHERE a.image_id=? ORDER BY k.name",
        )?;
        Ok(stmt
            .query_map([id.to_string()], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }
    /// Atomically replaces model output without accepting any suggested keywords.
    pub fn set_understanding(&self, id: ImageId, value: &Understanding) -> anyhow::Result<()> {
        anyhow::ensure!(
            !value.model_version.trim().is_empty(),
            "model_version must not be blank"
        );
        let normalized = |v: f32| v.is_finite() && (0.0..=1.0).contains(&v);
        for keyword in &value.keywords {
            anyhow::ensure!(
                normalized(keyword.confidence),
                "keyword confidence must be finite and in [0,1]"
            );
        }
        for region in &value.ocr {
            anyhow::ensure!(
                normalized(region.confidence),
                "OCR confidence must be finite and in [0,1]"
            );
            let [x0, y0, x1, y1] = region.bbox;
            anyhow::ensure!(
                region.bbox.into_iter().all(normalized) && x0 <= x1 && y0 <= y1,
                "OCR bbox must contain normalized, ordered, finite coordinates"
            );
        }
        let tx = self.0.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO understanding(image_id,model_version,keywords,caption,alt_text,ocr,ocr_text)
             VALUES(?,?,?,?,?,?,?) ON CONFLICT(image_id) DO UPDATE SET
             model_version=excluded.model_version,keywords=excluded.keywords,
             caption=excluded.caption,alt_text=excluded.alt_text,ocr=excluded.ocr,ocr_text=excluded.ocr_text",
            params![id.to_string(), value.model_version, serde_json::to_string(&value.keywords)?,
                value.caption, value.alt_text, serde_json::to_string(&value.ocr)?,
                value.ocr.iter().map(|r| r.text.as_str()).collect::<Vec<_>>().join(" ")],
        )?;
        refresh_fts(&tx, &id.to_string())?;
        tx.commit()?;
        Ok(())
    }

    /// Returns the latest persisted output, or None when none is stored.
    pub fn understanding(&self, id: ImageId) -> anyhow::Result<Option<Understanding>> {
        let row = self.0.conn.query_row(
            "SELECT model_version,keywords,caption,alt_text,ocr FROM understanding WHERE image_id=?",
            [id.to_string()],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?,
                r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?)),
        ).optional()?;
        row.map(|(model_version, keywords, caption, alt_text, ocr)| {
            Ok(Understanding {
                model_version,
                keywords: serde_json::from_str(&keywords)?,
                caption,
                alt_text,
                ocr: serde_json::from_str(&ocr)?,
            })
        })
        .transpose()
    }
}

/// Rebuild from accepted metadata and model output. OCR shares the caption
/// FTS column; suggestions are deliberately not accepted keywords.
pub(crate) fn refresh_fts(conn: &rusqlite::Connection, id: &str) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM fts WHERE rowid=(SELECT rowid FROM image WHERE id=?)",
        [id],
    )?;
    conn.execute(
        "INSERT INTO fts(rowid,image_id,filename,keywords,caption,camera,lens)
         SELECT i.rowid,i.id,f.name,
         (SELECT group_concat(k.name,' ') FROM keyword k JOIN image_keyword ik ON k.id=ik.keyword_id WHERE ik.image_id=i.id),
         COALESCE(i.caption,'') || ' ' || COALESCE(u.caption,'') || ' ' || COALESCE(u.ocr_text,''),
         i.camera,i.lens FROM image i JOIN file f ON f.id=i.file_id
         LEFT JOIN understanding u ON u.image_id=i.id WHERE i.id=?",
        [id],
    )?;
    Ok(())
}
