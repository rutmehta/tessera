//! Pure diagnostic storage/interpreter only: no renderer/cache observation is wired.
//! Request is not admission; lookup Some has no pending/persistent provenance.
#![allow(dead_code)]

use engine_api::stage::MemoKey;

pub const CAPACITY: usize = 256;
pub const MAX_BYTES: usize = 32 * 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bucket {
    Detail,
    PaddedWb,
    TileWb,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Some,
    None,
    Error,
    Request,
    Begin,
    End,
    Abort,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Identity {
    pub operator: u64,
    pub phase: u64,
    pub transaction: u64,
    pub generation: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    pub identity: Identity,
    /// Exact copied lookup key: image u128, StageId, ParamHash and TileCoord.
    pub key: MemoKey,
    pub bucket: Bucket,
    pub outcome: Outcome,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Unsupported,
    Overflow,
    Incomplete,
}
/// One snapshot belongs to one render transaction. These scalar fields bind
/// every record to the full recipe and actual resolved presentation/WB state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Context {
    pub recipe_fingerprint: [u8; 32],
    pub resolved_wb: [u8; 32],
    /// 0=scene linear, 1=SDR display, 2=display linear; no implicit defaults.
    pub output_tag: u8,
    pub headroom_bits: u32,
    /// Requested render level; key.tile.level may instead be L0 tail input.
    pub render_level: u8,
}
#[derive(Debug)]
pub struct Snapshot {
    pub context: Context,
    pub records: [Option<Record>; CAPACITY],
    pub len: usize,
    pub overflow: u64,
}
pub struct Recorder {
    storage: Snapshot,
}
#[derive(Debug, PartialEq, Eq)]
pub enum Reach {
    Hit,
    Miss,
    Failed,
    NotReached,
    Inconclusive,
}
impl Default for Recorder {
    fn default() -> Self {
        Self {
            storage: Snapshot {
                context: Context::default(),
                records: [None; CAPACITY],
                len: 0,
                overflow: 0,
            },
        }
    }
}
impl Recorder {
    pub fn push(&mut self, record: Record) -> Result<(), Error> {
        if self.storage.len >= CAPACITY {
            self.storage.overflow = self.storage.overflow.saturating_add(1);
            return Err(Error::Overflow);
        }
        self.storage.records[self.storage.len] = Some(record);
        self.storage.len += 1;
        Ok(())
    }
    pub fn drain(&mut self) -> Result<Snapshot, Error> {
        Ok(std::mem::take(self).storage)
    }
}
/// Observe one real lookup without changing its result or ownership.
/// Overflow lives only in the recorder: it never skips a lookup or replaces
/// Some/None/error. The outer result is retained for the scaffold API contract,
/// but observation itself has no failing return path.
pub fn observe_once<T, E>(
    recorder: &mut Recorder,
    mut record: Record,
    lookup: impl FnOnce() -> Result<Option<T>, E>,
) -> Result<Result<Option<T>, E>, Error> {
    let result = lookup();
    record.outcome = match &result {
        Ok(Some(_)) => Outcome::Some,
        Ok(None) => Outcome::None,
        Err(_) => Outcome::Error,
    };
    let _ = recorder.push(record);
    Ok(result)
}
/// Interpret only one complete, bounded, same-identity render transaction.
/// Missing/nested records prove nothing unless a successful enclosing hit
/// explains why that lookup was not reached. A request is never a lookup hit.
pub fn reach(snapshot: &Snapshot, identity: Identity, bucket: Bucket) -> Result<Reach, Error> {
    if snapshot.overflow != 0 || snapshot.len < 2 || snapshot.len > CAPACITY {
        return Ok(Reach::Inconclusive);
    }
    let records = &snapshot.records[..snapshot.len];
    if records
        .iter()
        .any(|r| r.is_none_or(|r| r.identity != identity))
    {
        return Ok(Reach::Inconclusive);
    }
    let first = records[0].as_ref().unwrap();
    let last = records[records.len() - 1].as_ref().unwrap();
    if first.outcome != Outcome::Begin || last.outcome != Outcome::End {
        return Ok(Reach::Inconclusive);
    }
    let body = &records[1..records.len() - 1];
    if body
        .iter()
        .flatten()
        .any(|r| matches!(r.outcome, Outcome::Begin | Outcome::End | Outcome::Abort))
    {
        return Ok(Reach::Inconclusive);
    }
    // Each exact key has one lookup followed by at most one request, and a
    // request is legal only after its miss. Parent requests close descendants.
    // This is request order, not any assertion about cache admission.
    fn step(previous: Option<(MemoKey, Outcome)>, record: &Record) -> Option<(MemoKey, Outcome)> {
        match (previous, record.outcome) {
            (None, Outcome::Some | Outcome::None | Outcome::Error) => {
                Some((record.key, record.outcome))
            }
            (Some((key, Outcome::None)), Outcome::Request) if key == record.key => {
                Some((key, Outcome::Request))
            }
            _ => None,
        }
    }
    let mut detail_state = None;
    let mut padded_state = None;
    for (index, record) in body.iter().flatten().enumerate() {
        match record.bucket {
            Bucket::Detail => {
                let Some(next) = step(detail_state, record) else {
                    return Ok(Reach::Inconclusive);
                };
                detail_state = Some(next);
            }
            Bucket::PaddedWb => {
                if !matches!(detail_state, Some((_, Outcome::None))) {
                    return Ok(Reach::Inconclusive);
                }
                let Some(next) = step(padded_state, record) else {
                    return Ok(Reach::Inconclusive);
                };
                padded_state = Some(next);
            }
            Bucket::TileWb => {
                if !matches!(detail_state, Some((_, Outcome::None)))
                    || !matches!(padded_state, Some((_, Outcome::None)))
                {
                    return Ok(Reach::Inconclusive);
                }
                // Fixed bounded prefix scan, no map/allocation. Each tile key
                // has its own lifecycle, so another tile's miss is no proof.
                let previous = body[..index]
                    .iter()
                    .flatten()
                    .rev()
                    .find(|r| r.bucket == Bucket::TileWb && r.key == record.key)
                    .map(|r| (r.key, r.outcome));
                if step(previous, record).is_none() {
                    return Ok(Reach::Inconclusive);
                }
            }
        }
    }
    // Any render lookup error prevents attribution of skipped downstream work.
    if body.iter().flatten().any(|r| r.outcome == Outcome::Error) {
        return Ok(
            if body
                .iter()
                .flatten()
                .any(|r| r.bucket == bucket && r.outcome == Outcome::Error)
            {
                Reach::Failed
            } else {
                Reach::Inconclusive
            },
        );
    }
    let outcomes = |target| {
        let mut hits = 0usize;
        let mut misses = 0usize;
        for r in body.iter().flatten().filter(|r| r.bucket == target) {
            match r.outcome {
                Outcome::Some => hits += 1,
                Outcome::None => misses += 1,
                _ => {}
            }
        }
        (hits, misses)
    };
    let activity = |target| body.iter().flatten().any(|r| r.bucket == target);
    let detail = outcomes(Bucket::Detail);
    let padded = outcomes(Bucket::PaddedWb);
    let tile = outcomes(Bucket::TileWb);
    // Full-level lookups execute at most once. Contradictory nested activity
    // after an enclosing hit is untrustworthy, not evidence for either branch.
    if detail.0 + detail.1 > 1
        || padded.0 + padded.1 > 1
        || (detail.0 != 0 && (activity(Bucket::PaddedWb) || activity(Bucket::TileWb)))
        || (padded.0 != 0 && activity(Bucket::TileWb))
    {
        return Ok(Reach::Inconclusive);
    }
    // A lower-level event without its enclosing misses is not a complete
    // control-flow trace, even when that individual event says Some.
    if (padded.0 + padded.1 != 0 && detail != (0, 1))
        || (tile.0 + tile.1 != 0 && (detail != (0, 1) || padded != (0, 1)))
    {
        return Ok(Reach::Inconclusive);
    }
    let (hits, misses) = outcomes(bucket);
    if hits != 0 && misses == 0 {
        return Ok(Reach::Hit);
    }
    if misses != 0 && hits == 0 {
        return Ok(Reach::Miss);
    }
    if hits != 0 || misses != 0 {
        return Ok(Reach::Inconclusive);
    }
    let not_reached = match bucket {
        Bucket::Detail => false,
        Bucket::PaddedWb => detail == (1, 0),
        Bucket::TileWb => detail == (1, 0) || (detail == (0, 1) && padded == (1, 0)),
    };
    Ok(if not_reached {
        Reach::NotReached
    } else {
        Reach::Inconclusive
    })
}
const _: () = assert!(std::mem::size_of::<Recorder>() <= MAX_BYTES);
const _: () = assert!(!std::mem::needs_drop::<Recorder>());

#[cfg(test)]
mod tests {
    use super::*;
    fn rec(bucket: Bucket, outcome: Outcome) -> Record {
        Record {
            identity: Identity {
                operator: 1,
                phase: 2,
                transaction: 3,
                generation: 4,
            },
            key: MemoKey {
                image_id: engine_api::id::ImageId(99),
                stage: engine_api::stage::StageId::WhiteBalance,
                params_hash: engine_api::stage::ParamHash::of(
                    engine_api::stage::StageId::WhiteBalance,
                    &5u32,
                ),
                tile: engine_api::tile::TileCoord::new(0, 1, 2),
            },
            bucket,
            outcome,
        }
    }
    fn snapshot(records: &[Record]) -> Snapshot {
        let mut s = Snapshot {
            context: Context::default(),
            records: [None; CAPACITY],
            len: records.len(),
            overflow: 0,
        };
        for (dst, src) in s.records.iter_mut().zip(records) {
            *dst = Some(*src);
        }
        s
    }
    #[test]
    fn fixed_scalar_storage_has_no_drop_and_fits_byte_bound() {
        assert!(std::mem::size_of::<Recorder>() <= MAX_BYTES);
        assert!(!std::mem::needs_drop::<Record>());
        assert!(!std::mem::needs_drop::<Snapshot>());
        assert!(!std::mem::needs_drop::<Recorder>());
    }
    #[test]
    fn copies_exact_key_identity_matrix_and_drains_once() {
        let mut r = Recorder::default();
        let event = rec(Bucket::TileWb, Outcome::Some);
        r.push(event).unwrap();
        let s = r.drain().unwrap();
        assert_eq!(s.records[0], Some(event));
        assert_eq!(s.len, 1);
        assert_eq!(r.drain().unwrap().len, 0);
    }
    #[test]
    fn cap_plus_one_preserves_prefix_and_refuses_attribution() {
        let mut r = Recorder::default();
        for i in 0..CAPACITY {
            let mut e = rec(Bucket::TileWb, Outcome::None);
            e.identity.transaction = i as u64;
            r.push(e).unwrap();
        }
        assert_eq!(
            r.push(rec(Bucket::Detail, Outcome::Some)),
            Err(Error::Overflow)
        );
        let s = r.drain().unwrap();
        assert_eq!(s.len, CAPACITY);
        assert_eq!(s.overflow, 1);
        assert_eq!(
            s.records[CAPACITY - 1].unwrap().identity.transaction,
            (CAPACITY - 1) as u64
        );
        assert_eq!(
            reach(
                &s,
                rec(Bucket::Detail, Outcome::Some).identity,
                Bucket::Detail
            )
            .unwrap(),
            Reach::Inconclusive
        );
    }
    #[test]
    fn real_some_none_and_error_results_are_returned_once_without_ownership() {
        use std::{cell::Cell, sync::Arc};
        let mut r = Recorder::default();
        let calls = Cell::new(0);
        let value = Arc::new(7);
        let weak = Arc::downgrade(&value);
        let got = observe_once(&mut r, rec(Bucket::Detail, Outcome::None), || {
            calls.set(calls.get() + 1);
            Ok::<_, u8>(Some(value))
        })
        .unwrap()
        .unwrap()
        .unwrap();
        assert_eq!(calls.get(), 1);
        drop(got);
        assert!(weak.upgrade().is_none());
        let none = observe_once(&mut r, rec(Bucket::TileWb, Outcome::None), || {
            calls.set(calls.get() + 1);
            Ok::<Option<u8>, u8>(None)
        })
        .unwrap();
        assert_eq!(none, Ok(None));
        let err = observe_once(&mut r, rec(Bucket::PaddedWb, Outcome::None), || {
            calls.set(calls.get() + 1);
            Err::<Option<u8>, _>(42)
        })
        .unwrap();
        assert_eq!(err, Err(42));
        assert_eq!(calls.get(), 3);
        let s = r.drain().unwrap();
        assert_eq!(s.len, 3);
        assert_eq!(
            s.records.map(|r| r.map(|r| r.outcome))[..3],
            [
                Some(Outcome::Some),
                Some(Outcome::None),
                Some(Outcome::Error)
            ]
        );
    }
    #[test]
    fn full_or_already_overflowed_recorder_never_changes_or_skips_lookup() {
        use std::{cell::Cell, sync::Arc};
        for previous_overflow in [0, 7] {
            let mut r = Recorder {
                storage: snapshot(&[rec(Bucket::TileWb, Outcome::None); CAPACITY]),
            };
            r.storage.overflow = previous_overflow;
            let calls = Cell::new(0);
            let value = Arc::new(23);
            let weak = Arc::downgrade(&value);
            let original_ptr = Arc::as_ptr(&value);
            let got = observe_once(&mut r, rec(Bucket::Detail, Outcome::None), || {
                calls.set(calls.get() + 1);
                Ok::<_, u8>(Some(value))
            })
            .expect("overflow is diagnostic state, never replacement renderer result")
            .unwrap()
            .unwrap();
            assert_eq!(calls.get(), 1);
            assert_eq!(Arc::as_ptr(&got), original_ptr);
            drop(got);
            assert!(weak.upgrade().is_none());
            let none = observe_once(&mut r, rec(Bucket::Detail, Outcome::None), || {
                calls.set(calls.get() + 1);
                Ok::<Option<u8>, u8>(None)
            })
            .expect("must return real None despite overflow");
            assert_eq!(none, Ok(None));
            let err = observe_once(&mut r, rec(Bucket::Detail, Outcome::None), || {
                calls.set(calls.get() + 1);
                Err::<Option<u8>, _>(42)
            })
            .expect("must return original error despite overflow");
            assert_eq!(err, Err(42));
            assert_eq!(calls.get(), 3);
            let s = r.drain().unwrap();
            assert_eq!(s.len, CAPACITY);
            assert_eq!(s.overflow, previous_overflow + 3);
            assert_eq!(s.records[0], Some(rec(Bucket::TileWb, Outcome::None)));
        }
    }
    #[test]
    fn full_memo_identity_and_render_context_survive_drain_distinctly() {
        let mut r = Recorder::default();
        let context = Context {
            recipe_fingerprint: [11; 32],
            resolved_wb: [12; 32],
            output_tag: 2,
            headroom_bits: 4f32.to_bits(),
            render_level: 1,
        };
        r.storage.context = context;
        let original = rec(Bucket::TileWb, Outcome::Some);
        let mut variants = [original; 5];
        variants[1].key.image_id = engine_api::id::ImageId(100);
        variants[2].key.stage = engine_api::stage::StageId::Detail;
        variants[3].key.params_hash =
            engine_api::stage::ParamHash::of(engine_api::stage::StageId::WhiteBalance, &6u32);
        variants[4].key.tile = engine_api::tile::TileCoord::new(1, 2, 3);
        for record in variants {
            r.push(record).unwrap();
        }
        let s = r.drain().unwrap();
        assert_eq!(s.context, context);
        for (i, record) in variants.iter().enumerate() {
            assert_eq!(s.records[i], Some(*record));
        }
        assert_eq!(s.records[0].unwrap().key.tile.level, 0);
        assert_eq!(s.context.render_level, 1);
    }
    #[test]
    fn complete_detail_hit_marks_nested_not_reached_only() {
        let s = snapshot(&[
            rec(Bucket::Detail, Outcome::Begin),
            rec(Bucket::Detail, Outcome::Some),
            rec(Bucket::Detail, Outcome::End),
        ]);
        let id = rec(Bucket::Detail, Outcome::Some).identity;
        assert_eq!(reach(&s, id, Bucket::Detail).unwrap(), Reach::Hit);
        assert_eq!(reach(&s, id, Bucket::PaddedWb).unwrap(), Reach::NotReached);
        assert_eq!(reach(&s, id, Bucket::TileWb).unwrap(), Reach::NotReached);
    }
    #[test]
    fn padded_hit_after_detail_miss_skips_only_tiles() {
        let s = snapshot(&[
            rec(Bucket::Detail, Outcome::Begin),
            rec(Bucket::Detail, Outcome::None),
            rec(Bucket::PaddedWb, Outcome::Some),
            rec(Bucket::Detail, Outcome::End),
        ]);
        let id = rec(Bucket::Detail, Outcome::Some).identity;
        assert_eq!(reach(&s, id, Bucket::Detail).unwrap(), Reach::Miss);
        assert_eq!(reach(&s, id, Bucket::PaddedWb).unwrap(), Reach::Hit);
        assert_eq!(reach(&s, id, Bucket::TileWb).unwrap(), Reach::NotReached);
    }
    #[test]
    fn missing_error_abort_and_wrong_operator_cannot_prove_nested_hit() {
        let id = rec(Bucket::Detail, Outcome::Some).identity;
        for records in [
            vec![],
            vec![rec(Bucket::Detail, Outcome::Begin)],
            vec![
                rec(Bucket::Detail, Outcome::Begin),
                rec(Bucket::Detail, Outcome::Error),
                rec(Bucket::Detail, Outcome::Abort),
            ],
        ] {
            assert_eq!(
                reach(&snapshot(&records), id, Bucket::TileWb).unwrap(),
                Reach::Inconclusive
            );
        }
        let mut other = id;
        other.operator += 1;
        let s = snapshot(&[
            rec(Bucket::Detail, Outcome::Begin),
            rec(Bucket::Detail, Outcome::Some),
            rec(Bucket::Detail, Outcome::End),
        ]);
        assert_eq!(
            reach(&s, other, Bucket::TileWb).unwrap(),
            Reach::Inconclusive
        );
    }
    #[test]
    fn request_only_is_not_successful_lookup_or_publication() {
        let s = snapshot(&[
            rec(Bucket::Detail, Outcome::Begin),
            rec(Bucket::Detail, Outcome::Request),
            rec(Bucket::Detail, Outcome::End),
        ]);
        assert_eq!(
            reach(
                &s,
                rec(Bucket::Detail, Outcome::Request).identity,
                Bucket::Detail
            )
            .unwrap(),
            Reach::Inconclusive
        );
    }
    #[test]
    fn any_request_under_detail_hit_is_contradictory_not_not_reached() {
        let id = rec(Bucket::Detail, Outcome::Begin).identity;
        for child in [Bucket::PaddedWb, Bucket::TileWb] {
            let s = snapshot(&[
                rec(Bucket::Detail, Outcome::Begin),
                rec(Bucket::Detail, Outcome::Some),
                rec(child, Outcome::Request),
                rec(Bucket::Detail, Outcome::End),
            ]);
            for target in [Bucket::Detail, Bucket::PaddedWb, Bucket::TileWb] {
                assert_eq!(reach(&s, id, target).unwrap(), Reach::Inconclusive);
            }
        }
    }
    #[test]
    fn tile_request_under_padded_hit_is_contradictory_not_not_reached() {
        let id = rec(Bucket::Detail, Outcome::Begin).identity;
        let s = snapshot(&[
            rec(Bucket::Detail, Outcome::Begin),
            rec(Bucket::Detail, Outcome::None),
            rec(Bucket::PaddedWb, Outcome::Some),
            rec(Bucket::TileWb, Outcome::Request),
            rec(Bucket::Detail, Outcome::End),
        ]);
        for target in [Bucket::Detail, Bucket::PaddedWb, Bucket::TileWb] {
            assert_eq!(reach(&s, id, target).unwrap(), Reach::Inconclusive);
        }
    }
    #[test]
    fn child_lookup_before_enclosing_miss_is_inconclusive() {
        let id = rec(Bucket::Detail, Outcome::Begin).identity;
        for body in [
            vec![
                rec(Bucket::PaddedWb, Outcome::Some),
                rec(Bucket::Detail, Outcome::None),
            ],
            vec![
                rec(Bucket::Detail, Outcome::None),
                rec(Bucket::TileWb, Outcome::Some),
                rec(Bucket::PaddedWb, Outcome::None),
            ],
        ] {
            let mut records = vec![rec(Bucket::Detail, Outcome::Begin)];
            records.extend(body);
            records.push(rec(Bucket::Detail, Outcome::End));
            let s = snapshot(&records);
            for target in [Bucket::Detail, Bucket::PaddedWb, Bucket::TileWb] {
                assert_eq!(reach(&s, id, target).unwrap(), Reach::Inconclusive);
            }
        }
    }
    #[test]
    fn request_after_same_key_hit_and_child_after_parent_request_are_inconclusive() {
        let id = rec(Bucket::Detail, Outcome::Begin).identity;
        for body in [
            vec![
                rec(Bucket::Detail, Outcome::None),
                rec(Bucket::PaddedWb, Outcome::Some),
                rec(Bucket::PaddedWb, Outcome::Request),
            ],
            vec![
                rec(Bucket::Detail, Outcome::None),
                rec(Bucket::Detail, Outcome::Request),
                rec(Bucket::PaddedWb, Outcome::Some),
            ],
            vec![
                rec(Bucket::Detail, Outcome::None),
                rec(Bucket::PaddedWb, Outcome::None),
                rec(Bucket::PaddedWb, Outcome::Request),
                rec(Bucket::TileWb, Outcome::Some),
            ],
            vec![
                rec(Bucket::Detail, Outcome::None),
                rec(Bucket::PaddedWb, Outcome::None),
                rec(Bucket::TileWb, Outcome::Some),
                rec(Bucket::TileWb, Outcome::Request),
            ],
        ] {
            let mut records = vec![rec(Bucket::Detail, Outcome::Begin)];
            records.extend(body);
            records.push(rec(Bucket::Detail, Outcome::End));
            for target in [Bucket::Detail, Bucket::PaddedWb, Bucket::TileWb] {
                assert_eq!(
                    reach(&snapshot(&records), id, target).unwrap(),
                    Reach::Inconclusive
                );
            }
        }
    }
    #[test]
    fn duplicate_and_unknown_key_requests_cannot_borrow_other_miss() {
        let id = rec(Bucket::Detail, Outcome::Begin).identity;
        let mut unknown = rec(Bucket::TileWb, Outcome::Request);
        unknown.key.tile.x += 1;
        for requests in [
            vec![unknown],
            vec![rec(Bucket::TileWb, Outcome::Request); 2],
        ] {
            let mut records = vec![
                rec(Bucket::Detail, Outcome::Begin),
                rec(Bucket::Detail, Outcome::None),
                rec(Bucket::PaddedWb, Outcome::None),
                rec(Bucket::TileWb, Outcome::None),
            ];
            records.extend(requests);
            records.push(rec(Bucket::Detail, Outcome::End));
            for target in [Bucket::Detail, Bucket::PaddedWb, Bucket::TileWb] {
                assert_eq!(
                    reach(&snapshot(&records), id, target).unwrap(),
                    Reach::Inconclusive
                );
            }
        }
    }
    #[test]
    fn cold_requests_close_children_in_order_and_padded_hit_stays_valid() {
        let id = rec(Bucket::Detail, Outcome::Begin).identity;
        let mut second = rec(Bucket::TileWb, Outcome::None);
        second.key.tile.x += 1;
        let mut second_request = second;
        second_request.outcome = Outcome::Request;
        let cold = snapshot(&[
            rec(Bucket::Detail, Outcome::Begin),
            rec(Bucket::Detail, Outcome::None),
            rec(Bucket::PaddedWb, Outcome::None),
            rec(Bucket::TileWb, Outcome::None),
            rec(Bucket::TileWb, Outcome::Request),
            second,
            second_request,
            rec(Bucket::PaddedWb, Outcome::Request),
            rec(Bucket::Detail, Outcome::Request),
            rec(Bucket::Detail, Outcome::End),
        ]);
        for target in [Bucket::Detail, Bucket::PaddedWb, Bucket::TileWb] {
            assert_eq!(reach(&cold, id, target).unwrap(), Reach::Miss);
        }
        let hit = snapshot(&[
            rec(Bucket::Detail, Outcome::Begin),
            rec(Bucket::Detail, Outcome::None),
            rec(Bucket::PaddedWb, Outcome::Some),
            rec(Bucket::Detail, Outcome::Request),
            rec(Bucket::Detail, Outcome::End),
        ]);
        assert_eq!(reach(&hit, id, Bucket::Detail).unwrap(), Reach::Miss);
        assert_eq!(reach(&hit, id, Bucket::PaddedWb).unwrap(), Reach::Hit);
        assert_eq!(reach(&hit, id, Bucket::TileWb).unwrap(), Reach::NotReached);
    }
}

// Explicit-token transport; the whole module is feature-gated (lib.rs).
mod fingerprint;
pub mod harness;
pub(crate) mod live;
mod transport;
pub use fingerprint::{output_tag, process_identity, recipe_fingerprint, settings_fingerprint};
pub use transport::{
    ARENA, Arena, BeginContext, Counts, Drain, DrainView, Epoch, Lease, PhaseKind, RequestContext,
    Reservation, Route, RouteState, SlotMeta, State, Token,
};
