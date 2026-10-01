//! One serial full-level render's pinned style results. Limits cover retained
//! tile-buffer capacity and estimated in-flight payload reservations, NOT scratch, metadata,
//! compositor caches, output, GPU, RSS, or other concurrent frame passes.
use super::styles::{BevelKind, LayerStyles, StyleEffect, StylePlane};
use crate::raster::Raster;
use engine_api::{EngineResult, jobs::CancellationToken, tile::Extent};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub(super) type Key = (u64, u64, u64); // exact DocRef namespace, snapshot rev, layer ID

pub(super) struct Entry {
    pub source: Raster,
    pub planes: Vec<StylePlane>,
    // Distinct stable namespaces for mip reads; never alias isolated-source
    // compilation keys or other planes, even when all use node=0.
    pub source_key: u64,
    pub plane_keys: Vec<u64>,
}

impl Entry {
    pub(super) fn bytes(&self) -> Option<usize> {
        std::iter::once(&self.source)
            .chain(self.planes.iter().map(|p| &p.raster))
            .try_fold(0usize, |sum, raster| {
                raster.slots().try_fold(sum, |sum, (_, slot)| {
                    sum.checked_add(slot.tile.as_ref().map_or(0, |t| t.allocated_byte_len()))
                })
            })
    }
}

#[derive(Default)]
struct State {
    entries: HashMap<Key, Arc<Entry>>,
    // Includes live reservations; published entries remain charged until pass drop.
    bytes: usize,
    count: usize,
}

pub(crate) struct StylePass {
    byte_limit: usize,
    entry_limit: usize,
    state: Mutex<State>,
}
impl Default for StylePass {
    fn default() -> Self {
        Self::new(1 << 30, 256)
    }
}
impl StylePass {
    pub(super) fn new(byte_limit: usize, entry_limit: usize) -> Self {
        Self {
            byte_limit,
            entry_limit,
            state: Mutex::new(State::default()),
        }
    }
    // Full-level traversal is serial. Recursive calls use distinct isolated
    // namespaces, so no concurrent single-flight waiter or recursive lock is needed.
    pub(super) fn get_or_build(
        &self,
        key: Key,
        predicted: Option<usize>,
        cancel: Option<&CancellationToken>,
        build: impl FnOnce(bool) -> EngineResult<Entry>,
    ) -> EngineResult<Arc<Entry>> {
        super::smart_filters::check_render_cancel(cancel)?;
        if let Some(entry) = self.get(key) {
            return Ok(entry);
        }
        let reservation = predicted.and_then(|bytes| self.reserve(bytes));
        let entry = Arc::new(build(reservation.is_some())?);
        // In particular, cancellation during the final style operation must
        // not publish a completed result. The reservation drops on every error.
        super::smart_filters::check_render_cancel(cancel)?;
        Ok(match reservation {
            Some(reservation) => reservation.publish(key, entry),
            None => entry,
        })
    }
    #[cfg(test)]
    pub(super) fn usage(&self) -> (usize, usize, usize) {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        (state.bytes, state.count, state.entries.len())
    }
    pub(super) fn get(&self, key: Key) -> Option<Arc<Entry>> {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entries
            .get(&key)
            .cloned()
    }
    pub(super) fn reserve(&self, bytes: usize) -> Option<Reservation<'_>> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let total = state.bytes.checked_add(bytes)?;
        let count = state.count.checked_add(1)?;
        if total > self.byte_limit || count > self.entry_limit {
            return None;
        }
        state.bytes = total;
        state.count = count;
        Some(Reservation {
            pass: self,
            bytes,
            active: true,
        })
    }
}

pub(super) struct Reservation<'a> {
    pass: &'a StylePass,
    bytes: usize,
    active: bool,
}
impl Reservation<'_> {
    /// Reconcile actual buffer capacities before retaining. A larger-than-
    /// predicted allocation can refuse retention, never exceed the budget.
    pub(super) fn publish(mut self, key: Key, entry: Arc<Entry>) -> Arc<Entry> {
        let Some(actual) = entry.bytes() else {
            return entry;
        };
        {
            let mut state = self.pass.state.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(existing) = state.entries.get(&key).cloned() {
                return existing;
            }
            let Some(total) = (state.bytes - self.bytes).checked_add(actual) else {
                return entry;
            };
            if total > self.pass.byte_limit {
                return entry;
            }
            state.bytes = total;
            state.entries.insert(key, entry.clone());
            self.active = false;
        }
        entry
    }
}
impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        if self.active {
            let mut state = self.pass.state.lock().unwrap_or_else(|e| e.into_inner());
            state.bytes -= self.bytes;
            state.count -= 1;
        }
    }
}

/// Dense F32 source and all emitted F32 planes. Current builders create exact
/// no-halo edge layouts, so this estimates retained sample payload while
/// building. Scratch and allocator capacity growth are not reserved. Publication
/// still checks every actual tile capacity, including any future padding/slack.
/// Overflow declines caching; it does not reject the underlying render.
pub(super) fn predicted_bytes(extent: Extent, styles: &LayerStyles) -> Option<usize> {
    let mut planes = 1usize;
    for effect in &styles.effects {
        let emitted = match effect {
            StyleEffect::DropShadow(s) | StyleEffect::InnerShadow(s) => usize::from(s.enabled),
            StyleEffect::OuterGlow(s) | StyleEffect::InnerGlow(s) => usize::from(s.enabled),
            StyleEffect::Satin(s) => usize::from(s.enabled),
            StyleEffect::Overlay(s)
            | StyleEffect::ColorOverlay(s)
            | StyleEffect::GradientOverlay(s)
            | StyleEffect::PatternOverlay(s) => usize::from(s.enabled),
            StyleEffect::Stroke(s) => usize::from(s.enabled),
            StyleEffect::Bevel(s) if s.enabled => match s.kind {
                BevelKind::Inner | BevelKind::Outer => 2,
                BevelKind::Emboss | BevelKind::Pillow => 4,
            },
            StyleEffect::Bevel(_) => 0,
        };
        planes = planes.checked_add(emitted)?;
    }
    (extent.width as usize)
        .checked_mul(extent.height as usize)?
        .checked_mul(16)?
        .checked_mul(planes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raster::Depth;
    use engine_api::{
        EngineError,
        tile::{Tile, TileCoord},
    };

    fn entry(capacity: usize) -> Entry {
        let mut source = Raster::new(Extent::new(1, 1), 4, Depth::F32, 0.0);
        let mut samples = Vec::with_capacity(capacity);
        samples.resize(4, 0.0f32);
        let tile =
            Tile::from_samples(TileCoord::new(0, 0, 0), source.layout(0, 0), samples).unwrap();
        source.set_slot(0, 0, Some(tile), 1).unwrap();
        Entry {
            source,
            planes: vec![],
            source_key: 100,
            plane_keys: vec![],
        }
    }

    #[test]
    fn reservations_bound_nested_entries_and_release_on_drop() {
        let pass = StylePass::new(32, 2);
        let outer = pass.reserve(16).unwrap();
        let inner = pass.reserve(16).unwrap();
        assert_eq!(pass.usage(), (32, 2, 0));
        assert!(pass.reserve(0).is_none()); // entry cap, even zero bytes
        assert!(pass.reserve(1).is_none());
        drop(inner);
        assert!(pass.reserve(17).is_none());
        drop(outer);
        assert_eq!(pass.usage(), (0, 0, 0));
        let pass = StylePass::new(usize::MAX, usize::MAX);
        let held = pass.reserve(usize::MAX).unwrap();
        assert!(pass.reserve(1).is_none()); // checked-add overflow
        drop(held);
        assert_eq!(pass.usage(), (0, 0, 0));
    }

    #[test]
    fn actual_capacity_refuses_oversized_publication_and_exact_fit_reuses() {
        let candidate = entry(32);
        let actual = candidate.bytes().unwrap();
        assert!(actual > 16);
        let pass = StylePass::new(actual - 1, 1);
        let result = pass
            .get_or_build((1, 1, 1), Some(16), None, |admitted| {
                assert!(admitted);
                Ok(candidate)
            })
            .unwrap();
        assert_eq!(result.bytes(), Some(actual)); // valid result, no new render error
        assert_eq!(pass.usage(), (0, 0, 0));
        let pass = StylePass::new(actual, 1);
        let first = pass
            .get_or_build((1, 1, 1), Some(actual), None, |_| Ok(entry(32)))
            .unwrap();
        let second = pass
            .get_or_build((1, 1, 1), Some(actual), None, |_| panic!("cache miss"))
            .unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(pass.usage(), (actual, 1, 1));
    }

    #[test]
    fn build_error_and_final_cancellation_release_reservation_and_allow_retry() {
        let pass = StylePass::new(64, 2);
        let key = (1, 1, 1);
        let failure = pass.get_or_build(key, Some(16), None, |_| {
            assert_eq!(pass.usage(), (16, 1, 0)); // build owns no cache lock
            Err(EngineError::internal("synthetic build error"))
        });
        assert!(failure.is_err());
        assert_eq!(pass.usage(), (0, 0, 0));
        let cancel = CancellationToken::new();
        let failure = pass.get_or_build(key, Some(16), Some(&cancel), |_| {
            cancel.cancel();
            Ok(entry(4))
        });
        assert!(matches!(failure, Err(EngineError::Cancelled)));
        assert_eq!(pass.usage(), (0, 0, 0));
        let retry = pass
            .get_or_build(key, Some(16), None, |_| Ok(entry(4)))
            .unwrap();
        assert_eq!(pass.usage(), (retry.bytes().unwrap(), 1, 1));
        // Cancelled warm hits also preserve cancellation rather than returning pixels.
        assert!(matches!(
            pass.get_or_build(key, Some(16), Some(&cancel), |_| panic!("build")),
            Err(EngineError::Cancelled)
        ));
    }

    #[test]
    fn declined_admission_builds_uncached_without_reserving() {
        for predicted in [Some(16), None] {
            let pass = StylePass::new(0, 0);
            let value = pass
                .get_or_build((1, 1, 1), predicted, None, |admitted| {
                    assert!(!admitted); // caller disables admission for this subtree
                    assert_eq!(pass.usage(), (0, 0, 0));
                    Ok(entry(4))
                })
                .unwrap();
            assert_eq!(value.bytes(), Some(16));
            assert_eq!(pass.usage(), (0, 0, 0));
        }
    }

    #[test]
    fn nested_build_shares_reservations_without_holding_mutex() {
        let pass = StylePass::new(32, 2);
        pass.get_or_build((1, 1, 1), Some(16), None, |admitted| {
            assert!(admitted);
            pass.get_or_build((2, 1, 1), Some(16), None, |child| {
                assert!(child);
                assert_eq!(pass.usage(), (32, 2, 0));
                Ok(entry(4))
            })?;
            Ok(entry(4))
        })
        .unwrap();
        assert_eq!(pass.usage(), (32, 2, 2));
    }
}
