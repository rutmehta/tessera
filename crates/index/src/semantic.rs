//! Dependency-inverted vector retrieval; no model/runtime dependency in the catalog.
use crate::{Index, Query};
use engine_api::{
    error::{EngineError, EngineResult},
    id::ImageId,
};
use std::collections::HashSet;

/// Vector retrieval implemented by the embedding owner (or an application adapter).
/// Scores are finite similarities: higher is better. `k` is an upper bound, not
/// an allocation size. `usize::MAX` requests all available hits so catalog facets
/// and pagination cannot discard eligible results behind an arbitrary top-k cutoff.
pub trait SemanticSearch {
    fn search_text(&mut self, query: &str, k: usize) -> EngineResult<Vec<(ImageId, f32)>>;
}

pub(crate) fn require_provider(query: &Query) -> EngineResult<()> {
    if query.semantic.is_some() {
        return Err(EngineError::invalid(
            "semantic",
            "use search_with_semantic with a SemanticSearch provider",
        ));
    }
    Ok(())
}

impl Index {
    /// Counts facets over the complete filtered semantic/hybrid result set,
    /// independently of pagination. Without semantic text, uses legacy facets.
    pub fn facets_with_semantic(
        &self,
        query: &Query,
        provider: &mut dyn SemanticSearch,
    ) -> EngineResult<crate::Facets> {
        if query.semantic.is_none() {
            return self.facets(query);
        }
        let mut unpaged = query.clone();
        unpaged.offset = 0;
        unpaged.limit = usize::MAX;
        let ids: Vec<_> = self
            .search_with_semantic(&unpaged, provider)?
            .into_iter()
            .map(|id| id.to_string())
            .collect();
        let json = serde_json::to_string(&ids)?;
        let counts = |sql: &str| -> EngineResult<Vec<(String, u64)>> {
            let mut stmt = self.0.conn.prepare(sql).map_err(crate::IndexError::from)?;
            Ok(stmt
                .query_map([&json], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? as u64)))
                .map_err(crate::IndexError::from)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(crate::IndexError::from)?)
        };
        Ok(crate::Facets {
            cameras: counts(
                "SELECT coalesce(camera,''),count(*) FROM image WHERE id IN (SELECT value FROM json_each(?)) GROUP BY camera",
            )?,
            lenses: counts(
                "SELECT coalesce(lens,''),count(*) FROM image WHERE id IN (SELECT value FROM json_each(?)) GROUP BY lens",
            )?,
            decisions: counts(
                "SELECT decision,count(*) FROM selection WHERE image_id IN (SELECT value FROM json_each(?)) GROUP BY decision",
            )?,
            keywords: counts(
                "SELECT k.name,count(*) FROM keyword k JOIN image_keyword ik ON ik.keyword_id=k.id WHERE ik.image_id IN (SELECT value FROM json_each(?)) GROUP BY k.name",
            )?,
        })
    }

    /// Vector similarity order after all catalog filters, then offset/limit.
    /// Without semantic text this is exactly the existing catalog search.
    pub fn search_with_semantic(
        &self,
        query: &Query,
        provider: &mut dyn SemanticSearch,
    ) -> EngineResult<Vec<ImageId>> {
        let Some(text) = query.semantic.as_deref() else {
            return self.search(query);
        };
        let mut filters = query.clone();
        filters.semantic = None;
        filters.text = None;
        filters.offset = 0;
        filters.limit = i64::MAX as usize;
        let eligible: HashSet<_> = self.search(&filters)?.into_iter().collect();
        let mut hits = provider.search_text(text, usize::MAX)?;
        hits.retain(|(id, score)| eligible.contains(id) && score.is_finite());
        hits.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let mut seen = HashSet::new();
        hits.retain(|(id, _)| seen.insert(*id));
        let mut ranked: Vec<_> = hits.into_iter().map(|(id, _)| id).collect();
        if let Some(lexical) = &query.text {
            // FTS5 BM25 is lower-is-better. Apply the identical catalog filters
            // to both lists before ranks are assigned; combine their union.
            let mut stmt = self
                .0
                .conn
                .prepare("SELECT image_id FROM fts WHERE fts MATCH ? ORDER BY bm25(fts),image_id")
                .map_err(crate::IndexError::from)?;
            let lexical_ids = stmt
                .query_map([lexical], |r| r.get::<_, String>(0))
                .map_err(crate::IndexError::from)?
                .map(|row| {
                    row.map_err(crate::IndexError::from)
                        .and_then(|id| crate::parse_id(&id))
                })
                .collect::<crate::Result<Vec<_>>>()?;
            let mut scores = std::collections::HashMap::<ImageId, f64>::new();
            for list in [
                ranked,
                lexical_ids
                    .into_iter()
                    .filter(|id| eligible.contains(id))
                    .collect(),
            ] {
                for (rank, id) in list.into_iter().enumerate() {
                    *scores.entry(id).or_default() += 1.0 / (60.0 + rank as f64 + 1.0);
                }
            }
            let mut fused: Vec<_> = scores.into_iter().collect();
            fused.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            ranked = fused.into_iter().map(|(id, _)| id).collect();
        }
        Ok(ranked
            .into_iter()
            .skip(query.offset)
            .take(if query.limit == 0 { 100 } else { query.limit })
            .collect())
    }
}
