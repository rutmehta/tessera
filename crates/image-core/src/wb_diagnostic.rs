//! Unsupported diagnostic contracts only: no renderer/cache observation is wired.
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
    pub fn push(&mut self, _record: Record) -> Result<(), Error> {
        Err(Error::Unsupported)
    }
    pub fn drain(&mut self) -> Result<Snapshot, Error> {
        Err(Error::Unsupported)
    }
}
/// Future observation wraps one real result, never performs a second lookup.
/// Regardless of recorder overflow, the closure MUST run once and its original
/// Some/None/error MUST return as Ok(original_result). Outer Unsupported is a
/// scaffold-only boundary and cannot remain as a production observer failure.
/// Diagnostic overflow is recorded separately in Snapshot::overflow; it may
/// never skip lookup, consume its value or replace its renderer error.
pub fn observe_once<T, E>(
    _recorder: &mut Recorder,
    _record: Record,
    _lookup: impl FnOnce() -> Result<Option<T>, E>,
) -> Result<Result<Option<T>, E>, Error> {
    Err(Error::Unsupported)
}
/// Interprets complete same-identity phases; absence alone is never a hit.
pub fn reach(_snapshot: &Snapshot, _identity: Identity, _bucket: Bucket) -> Result<Reach, Error> {
    Err(Error::Unsupported)
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
}
