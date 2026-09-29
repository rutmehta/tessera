//! Test-only harness contracts: admission, the Stage D (G12) interpretation
//! of copied records, and the C6 calibration-control guard.
#![allow(dead_code)]
use engine_api::stage::MemoKey;
use image_core::wb_diagnostic::{
    BeginContext, Bucket, CAPACITY, Context, Counts, Error, Identity, Outcome, PhaseKind, Reach,
    Record, RequestContext, RouteState, Snapshot, harness::epoch_inconclusive, reach,
};

#[derive(Clone, Copy)]
struct Admission {
    automatic: bool,
    first_metal: bool,
    reopened_metal: bool,
    candidate: bool,
    second_lookup_hits: u64,
    second_measurements: u64,
    second_publications: u64,
    identical_key: bool,
    original_wb: [u8; 32],
    custom_wb: [u8; 32],
}
fn admit(facts: Admission) -> Result<bool, Error> {
    let common = facts.automatic
        && facts.first_metal
        && facts.reopened_metal
        && facts.original_wb != facts.custom_wb;
    let decision = if facts.candidate {
        facts.second_lookup_hits == 1
            && facts.second_measurements == 0
            && facts.second_publications == 0
            && facts.identical_key
    } else {
        facts.second_lookup_hits == 0
            && facts.second_measurements == 1
            && facts.second_publications == 0
    };
    Ok(common && decision)
}
/// Stage D (D0): run the real four-phase protocol for this composition and
/// return the copied records. The driver persists every phase before any
/// assertion and asserts the protocol contracts (recipe, route receipts, TTL
/// and key facts, frame, fidelity, source hashes, Weak release) before it
/// returns. `Unsupported` off macOS.
fn real_phase_capture(
    candidate: bool,
    edr: bool,
    fixture: &std::path::Path,
) -> Result<Capture, Error> {
    #[cfg(target_os = "macos")]
    {
        crate::develop::preview_qualification::wb_phase_driver::run(candidate, edr, fixture)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (candidate, edr, fixture);
        Err(Error::Unsupported)
    }
}
fn valid() -> Admission {
    Admission {
        automatic: true,
        first_metal: true,
        reopened_metal: true,
        candidate: true,
        second_lookup_hits: 1,
        second_measurements: 0,
        second_publications: 0,
        identical_key: true,
        original_wb: [1; 32],
        custom_wb: [2; 32],
    }
}
#[test]
fn actual_auto_metal_and_live_unchanged_hit_are_required() {
    assert!(admit(valid()).unwrap());
    for f in [
        Admission {
            automatic: false,
            ..valid()
        },
        Admission {
            first_metal: false,
            ..valid()
        },
        Admission {
            reopened_metal: false,
            ..valid()
        },
        Admission {
            second_lookup_hits: 0,
            ..valid()
        },
        Admission {
            second_measurements: 1,
            ..valid()
        },
        Admission {
            second_publications: 1,
            ..valid()
        },
        Admission {
            identical_key: false,
            ..valid()
        },
    ] {
        assert!(!admit(f).unwrap());
    }
}
#[test]
fn equal_resolved_custom_matrix_is_inconclusive_without_substitution() {
    assert!(
        !admit(Admission {
            custom_wb: [1; 32],
            ..valid()
        })
        .unwrap()
    );
}
#[test]
fn baseline_requires_auto_metal_but_does_not_fabricate_cache_hits() {
    assert!(
        admit(Admission {
            candidate: false,
            second_lookup_hits: 0,
            second_measurements: 1,
            second_publications: 0,
            ..valid()
        })
        .unwrap()
    );
}
// ---------------------------------------------------------------------------
// Stage D (D0): copied evidence, pure interpretation and the G12 entry points.
// rev7 §6.4, §8, §9 and the Stage C review "What Stage D must prove" (1-9).
//
// Interpretation notes:
// - Records are the only evidence. They are copied out of the arena after a
//   phase quiesces, persisted by the driver before any assertion, and only
//   interpreted here once the driver's protocol contracts have held.
// - `RequestContext.recipe_fingerprint` hashes the id of the image actually
//   rendered (Stage C review NB-3). For a Smart Preview that is the proxy's
//   render id, not the source RAW's catalog id: comparable within this
//   process, never across differently derived images or rebuilt previews.
// - No timing is evidence (rev7 §8). Nothing here reads a duration.
// ---------------------------------------------------------------------------

/// Armed phase ids. One epoch per process; one armed phase per protocol step.
pub(crate) const OPEN: u64 = 1;
pub(crate) const REOPEN: u64 = 2;
pub(crate) const EXPOSURE: u64 = 3;
pub(crate) const DAYLIGHT: u64 = 4;
pub(crate) const RESTORE: u64 = 5;
pub(crate) const REPEAT: u64 = 6;
pub(crate) const CUSTOM: u64 = 7;
pub(crate) const ORIGINAL: u64 = 8;
pub(crate) fn phase_name(ordinal: u64) -> &'static str {
    match ordinal {
        OPEN => "open",
        REOPEN => "reopen",
        EXPOSURE => "exposure",
        DAYLIGHT => "daylight_first",
        RESTORE => "restore_as_shot",
        REPEAT => "daylight_repeat",
        CUSTOM => "custom",
        ORIGINAL => "restore_original",
        _ => "unknown",
    }
}
/// `RequestContext.phase_kind` values (transport `PhaseKind`).
const KIND_CALIBRATION: u8 = 1;
const KIND_DEVELOP: u8 = 2;
/// Calibration renders per measurement (backend `measure_at`).
const ITERATIONS: u64 = 3;

/// One drained transaction, copied from the arena scratch view.
#[derive(Clone, Debug)]
pub(crate) struct Txn {
    /// Debug form of the arena token (JSON only; the token is opaque).
    pub(crate) token: String,
    pub(crate) request: RequestContext,
    pub(crate) begin: Option<BeginContext>,
    pub(crate) route: RouteState,
    pub(crate) loss_at_finish: u64,
    /// `harness::operator_matches(meta)`, evaluated on the drained meta.
    pub(crate) operator_matches: bool,
    pub(crate) context: Context,
    pub(crate) records: Vec<Option<Record>>,
    pub(crate) overflow: u64,
}
/// A `frame_ready` callback seen by the phase listener.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FrameSeen {
    pub(crate) generation: u64,
    pub(crate) level: u8,
    pub(crate) is_final: bool,
}
/// One armed protocol step, drained after its final frame and quiescence.
#[derive(Clone, Debug)]
pub(crate) struct PhaseCapture {
    pub(crate) ordinal: u64,
    pub(crate) counts_before: Counts,
    /// Arena counts at quiescence, before draining (epoch-cumulative).
    pub(crate) counts_after: Counts,
    pub(crate) quiescent: bool,
    pub(crate) drained_all: bool,
    pub(crate) txns: Vec<Txn>,
    pub(crate) frames: Vec<FrameSeen>,
    pub(crate) final_generation: u64,
    /// The session's backend was automatically selected Metal.
    pub(crate) metal: bool,
}
/// Decision-cache probe deltas across the reopen (candidate only).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ProbeDelta {
    pub(crate) lookups: u64,
    pub(crate) hits: u64,
    pub(crate) measurements: u64,
    pub(crate) publications: u64,
    /// Both opens produced a key and the keys are byte-identical.
    pub(crate) key_identical: bool,
}
/// Copied records of one process, returned only after the protocol held.
#[derive(Clone, Debug)]
pub(crate) struct Capture {
    pub(crate) candidate: bool,
    pub(crate) edr: bool,
    /// No forced backend, no forced winner, no controlled samples.
    pub(crate) automatic: bool,
    pub(crate) out: std::path::PathBuf,
    pub(crate) phases: Vec<PhaseCapture>,
    pub(crate) probe: Option<ProbeDelta>,
}
impl Capture {
    fn phase(&self, ordinal: u64) -> Option<&PhaseCapture> {
        self.phases.iter().find(|p| p.ordinal == ordinal)
    }
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn counts_json(c: &Counts) -> serde_json::Value {
    serde_json::json!({
        "reserved": c.reserved, "active": c.active, "completed": c.completed,
        "scratch": c.scratch, "loss": c.loss, "disabled": c.disabled,
        "disabled_sticky": c.disabled_sticky,
        "attribution_rejected": c.attribution_rejected,
        "cpu_iterations_unobserved": c.cpu_iterations_unobserved,
    })
}
fn record_json(r: &Option<Record>) -> serde_json::Value {
    match r {
        None => serde_json::Value::Null,
        Some(r) => serde_json::json!({
            "operator": r.identity.operator, "phase": r.identity.phase,
            "transaction": r.identity.transaction, "generation": r.identity.generation,
            "bucket": format!("{:?}", r.bucket), "outcome": format!("{:?}", r.outcome),
            "key": serde_json::to_value(r.key).unwrap_or(serde_json::Value::Null),
        }),
    }
}
pub(crate) fn txn_json(t: &Txn) -> serde_json::Value {
    let q = &t.request;
    serde_json::json!({
        "token": t.token,
        "request": {
            "recipe_fingerprint": hex(&q.recipe_fingerprint),
            "settings_fingerprint": hex(&q.settings_fingerprint),
            "process_identity": q.process_identity, "output_tag": q.output_tag,
            "headroom_bits": q.headroom_bits, "requested_level": q.requested_level,
            "phase_kind": q.phase_kind, "expected_operator": q.expected_operator,
            "generation": q.generation,
        },
        "begin": t.begin.map(|b| serde_json::json!({
            "wb_digest": hex(&b.wb_digest),
            "wb_bits": b.wb_bits.iter().map(|v| format!("{v:016x}")).collect::<Vec<_>>(),
            "wb_finite": b.wb_bits.iter().all(|v| f64::from_bits(*v).is_finite()),
            "output_tag": b.output_tag, "headroom_bits": b.headroom_bits,
            "render_level": b.render_level, "operator": b.operator,
        })),
        "route": format!("{:?}", t.route),
        "loss_at_finish": t.loss_at_finish,
        "operator_matches": t.operator_matches,
        "context": {
            "recipe_fingerprint": hex(&t.context.recipe_fingerprint),
            "resolved_wb": hex(&t.context.resolved_wb),
            "output_tag": t.context.output_tag, "headroom_bits": t.context.headroom_bits,
            "render_level": t.context.render_level,
        },
        "overflow": t.overflow,
        "len": t.records.len(),
        "route_kind": format!("{:?}", route_kind(t)),
        "records": t.records.iter().map(record_json).collect::<Vec<_>>(),
    })
}
pub(crate) fn phase_json(p: &PhaseCapture) -> serde_json::Value {
    serde_json::json!({
        "phase": phase_name(p.ordinal), "ordinal": p.ordinal,
        "counts_before": counts_json(&p.counts_before),
        "counts_after": counts_json(&p.counts_after),
        "quiescent": p.quiescent, "drained_all": p.drained_all,
        "final_generation": p.final_generation, "metal": p.metal,
        "frames": p.frames.iter().map(|f| serde_json::json!({
            "generation": f.generation, "level": f.level, "is_final": f.is_final,
        })).collect::<Vec<_>>(),
        "k_superseded": k_superseded(p),
        "txns": p.txns.iter().map(txn_json).collect::<Vec<_>>(),
    })
}

/// Rebuilds the bounded snapshot `reach` interprets.
fn snapshot_of(t: &Txn) -> Box<Snapshot> {
    let mut s = Box::new(Snapshot {
        context: t.context,
        records: [None; CAPACITY],
        len: t.records.len().min(CAPACITY),
        overflow: t.overflow,
    });
    for (d, r) in s.records.iter_mut().zip(&t.records) {
        *d = *r;
    }
    s
}
fn identity_of(t: &Txn) -> Option<Identity> {
    t.records.first().copied().flatten().map(|r| r.identity)
}
fn calibrations(p: &PhaseCapture) -> Vec<&Txn> {
    p.txns
        .iter()
        .filter(|t| t.request.phase_kind == KIND_CALIBRATION)
        .collect()
}
fn develops(p: &PhaseCapture) -> Vec<&Txn> {
    p.txns
        .iter()
        .filter(|t| t.request.phase_kind == KIND_DEVELOP)
        .collect()
}
/// Develop transactions of generations other than the phase's final one:
/// superseded, cancelled or aborted work that consumed a slot (rev7 §3.3 k).
fn k_superseded(p: &PhaseCapture) -> usize {
    develops(p)
        .iter()
        .filter(|t| t.request.generation != p.final_generation)
        .count()
}
/// The one Delivered develop transaction of the phase's final generation.
fn final_develop(p: &PhaseCapture) -> Result<&Txn, String> {
    let found: Vec<_> = develops(p)
        .into_iter()
        .filter(|t| t.request.generation == p.final_generation && t.route == RouteState::Delivered)
        .collect();
    match found.as_slice() {
        [t] => Ok(t),
        other => Err(format!(
            "{}: {} Delivered develop transactions for final generation {}",
            phase_name(p.ordinal),
            other.len(),
            p.final_generation
        )),
    }
}

/// Resident route of a transaction, from its first lookup (rev7 §9.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RouteKind {
    /// Detail lookup at the requested render level.
    Level,
    /// Detail lookup at L0 for a coarser requested level (camera-linear coarse).
    Coarse,
    /// Per-tile WB lookups with no enclosing level lookups: Inconclusive.
    Tile,
    Unknown,
}
pub(crate) fn route_kind(t: &Txn) -> RouteKind {
    let first = t
        .records
        .iter()
        .flatten()
        .find(|r| !matches!(r.outcome, Outcome::Begin | Outcome::End | Outcome::Abort));
    match first {
        Some(r) if r.bucket == Bucket::Detail && r.key.tile.level == t.context.render_level => {
            RouteKind::Level
        }
        Some(r) if r.bucket == Bucket::Detail && r.key.tile.level == 0 => RouteKind::Coarse,
        Some(r) if r.bucket == Bucket::TileWb => RouteKind::Tile,
        _ => RouteKind::Unknown,
    }
}
/// `reach` for every bucket, in enclosing order.
fn chain(t: &Txn) -> Vec<(Bucket, Reach)> {
    let snap = snapshot_of(t);
    let Some(id) = identity_of(t) else {
        return Vec::new();
    };
    [Bucket::Detail, Bucket::PaddedWb, Bucket::TileWb]
        .into_iter()
        .map(|b| (b, reach(&snap, id, b).unwrap_or(Reach::Inconclusive)))
        .collect()
}
/// The highest bucket reached: the first Hit in enclosing order (a higher
/// hit leaves lower buckets correctly not reached), otherwise the deepest
/// Miss of an all-miss chain. Any other outcome first is returned as is.
pub(crate) fn top(t: &Txn) -> (Option<Bucket>, Reach) {
    let mut last = None;
    for (b, r) in chain(t) {
        match r {
            Reach::Hit => return (Some(b), Reach::Hit),
            Reach::Miss => last = Some(b),
            Reach::NotReached => break,
            other => return (Some(b), other),
        }
    }
    match last {
        Some(b) => (Some(b), Reach::Miss),
        None => (None, Reach::Inconclusive),
    }
}
fn keys(t: &Txn, bucket: Bucket, outcome: Outcome) -> Vec<MemoKey> {
    t.records
        .iter()
        .flatten()
        .filter(|r| r.bucket == bucket && r.outcome == outcome)
        .map(|r| r.key)
        .collect()
}

/// Review item 2, per phase.
fn phase_problems(p: &PhaseCapture) -> Vec<String> {
    let name = phase_name(p.ordinal);
    let mut v = Vec::new();
    if !p.quiescent {
        v.push(format!("{name}: leases did not quiesce"));
    }
    if !p.drained_all {
        v.push(format!("{name}: completed slots left undrained"));
    }
    if epoch_inconclusive(&p.counts_after) {
        let c = &p.counts_after;
        v.push(format!(
            "{name}: epoch inconclusive (loss {}, disabled {}, sticky {}, attribution_rejected {}; k={} superseded)",
            c.loss,
            c.disabled,
            c.disabled_sticky,
            c.attribution_rejected,
            k_superseded(p)
        ));
    }
    v
}
/// Review item 2, per transaction used.
fn txn_problems(label: &str, t: &Txn) -> Vec<String> {
    let mut v = Vec::new();
    if t.overflow != 0 || t.records.len() > CAPACITY || t.records.iter().any(Option::is_none) {
        v.push(format!(
            "{label}: overflow {} / incomplete records",
            t.overflow
        ));
    }
    if !t.operator_matches {
        v.push(format!(
            "{label}: Begin operator does not match the reservation"
        ));
    }
    if t.route != RouteState::Delivered {
        v.push(format!("{label}: route {:?}, not Delivered", t.route));
    }
    match t.begin {
        None => v.push(format!("{label}: no Begin")),
        Some(b) if !b.wb_bits.iter().all(|x| f64::from_bits(*x).is_finite()) => {
            v.push(format!("{label}: nonfinite WB matrix"))
        }
        Some(_) => {}
    }
    let first = t.records.first().copied().flatten().map(|r| r.outcome);
    let last = t.records.last().copied().flatten().map(|r| r.outcome);
    if first != Some(Outcome::Begin) || last != Some(Outcome::End) {
        v.push(format!("{label}: not a Begin..End transaction"));
    }
    v
}

/// Findings of one process. Empty lists mean the item was proven.
#[derive(Debug, Default)]
pub(crate) struct Verdict {
    /// Admission or record completeness failed: no conclusion (never rerun).
    pub(crate) inconclusive: Vec<String>,
    /// NB-2: a Delivered transaction for a generation never presented.
    pub(crate) violated: Vec<String>,
    /// Complete evidence contrary to the priming explanation (kept as is).
    pub(crate) contradicted: Vec<String>,
}

pub(crate) fn interpret(c: &Capture) -> (Verdict, serde_json::Value) {
    let mut v = Verdict::default();
    let mut facts = serde_json::Map::new();
    for o in OPEN..=ORIGINAL {
        if c.phase(o).is_none() {
            v.inconclusive
                .push(format!("{}: phase missing", phase_name(o)));
        }
    }
    if !v.inconclusive.is_empty() {
        return (v, serde_json::Value::Object(facts));
    }
    let p = |o| c.phase(o).expect("checked above");
    let (open, reopen) = (p(OPEN), p(REOPEN));
    let (daylight, repeat, custom) = (p(DAYLIGHT), p(REPEAT), p(CUSTOM));

    // Item 2 (phase level) for every phase: loss, disabled, k > 4, quiescence.
    for ph in &c.phases {
        v.inconclusive.extend(phase_problems(ph));
    }
    // Transactions used by the interpretation.
    fn final_of<'a>(ph: &'a PhaseCapture, v: &mut Verdict) -> Option<&'a Txn> {
        match final_develop(ph) {
            Ok(t) => {
                v.inconclusive.extend(txn_problems(
                    &format!("{} final", phase_name(ph.ordinal)),
                    t,
                ));
                Some(t)
            }
            Err(e) => {
                v.inconclusive.push(e);
                None
            }
        }
    }
    let open_final = final_of(open, &mut v);
    let reopen_final = final_of(reopen, &mut v);
    let daylight_t = final_of(daylight, &mut v);
    let repeat_t = final_of(repeat, &mut v);
    let custom_t = final_of(custom, &mut v);
    let reopen_cal = calibrations(reopen);
    for (i, t) in reopen_cal.iter().enumerate() {
        v.inconclusive
            .extend(txn_problems(&format!("reopen calibration {i}"), t));
    }

    // Item 1: admission. Baseline measurements come from the diagnostic
    // itself: the CPU arm counts ITERATIONS unobserved renders and the Metal
    // arm reserves one transaction per iteration. The baseline has no cache,
    // so it has no hits or publications to report (never fabricated).
    let cpu_delta = reopen
        .counts_after
        .cpu_iterations_unobserved
        .saturating_sub(reopen.counts_before.cpu_iterations_unobserved);
    let cal = reopen_cal.len() as u64;
    let derived = if cpu_delta % ITERATIONS == 0 && cal == cpu_delta {
        cpu_delta / ITERATIONS
    } else {
        u64::MAX
    };
    let (hits, measurements, publications, identical_key) = match (c.candidate, c.probe) {
        (true, Some(d)) => (
            d.hits,
            d.measurements.max(derived),
            d.publications,
            d.key_identical,
        ),
        (true, None) => (0, u64::MAX, u64::MAX, false),
        (false, _) => (0, derived, 0, true),
    };
    let wb = |t: Option<&Txn>| t.and_then(|t| t.begin).map_or([0; 32], |b| b.wb_digest);
    let facts_admission = Admission {
        automatic: c.automatic,
        first_metal: open.metal,
        reopened_metal: reopen.metal,
        candidate: c.candidate,
        second_lookup_hits: hits,
        second_measurements: measurements,
        second_publications: publications,
        identical_key,
        original_wb: wb(daylight_t),
        custom_wb: wb(custom_t),
    };
    let admitted = admit(facts_admission).unwrap_or(false);
    let measured = if measurements == u64::MAX {
        serde_json::Value::Null
    } else {
        measurements.into()
    };
    facts.insert(
        "admission".into(),
        serde_json::json!({
            "admitted": admitted, "automatic": c.automatic,
            "first_metal": open.metal, "reopened_metal": reopen.metal,
            "candidate": c.candidate, "second_lookup_hits": hits,
            "second_measurements": measured,
            "second_publications": publications, "identical_key": identical_key,
            "probe": c.probe.map(|d| serde_json::json!({
                "lookups": d.lookups, "hits": d.hits, "measurements": d.measurements,
                "publications": d.publications, "key_identical": d.key_identical,
            })),
            "reopen_calibration_transactions": cal,
            "reopen_cpu_iterations_unobserved": cpu_delta,
            "daylight_resolved_wb": hex(&facts_admission.original_wb),
            "custom_resolved_wb": hex(&facts_admission.custom_wb),
        }),
    );
    if !admitted {
        v.inconclusive
            .push("admission: automatic Metal / TTL hit / baseline measurement / distinct Custom WB not established".into());
    }

    // Item 6 (NB-2): assert the invariant, never force the race. A Delivered
    // develop transaction must belong to a generation the listener presented.
    // Post-bind aborts are recorded only; Aborted routes are never interpreted.
    let mut window = Vec::new();
    for ph in &c.phases {
        for t in develops(ph) {
            let g = t.request.generation;
            if t.route == RouteState::Delivered && !ph.frames.iter().any(|f| f.generation == g) {
                v.violated.push(format!(
                    "{}: Delivered develop transaction for generation {g} with no presented frame",
                    phase_name(ph.ordinal)
                ));
            }
            if t.route == RouteState::Aborted && t.begin.is_some() {
                window.push(serde_json::json!({"phase": phase_name(ph.ordinal), "generation": g}));
            }
        }
    }
    facts.insert("post_bind_aborts_recorded".into(), window.into());

    // Item 8: runtime-only facts, recorded as observed.
    let route_of = |t: Option<&Txn>| t.map(|t| format!("{:?}", route_kind(t)));
    facts.insert(
        "runtime".into(),
        serde_json::json!({
            "route_first_daylight": route_of(daylight_t),
            "route_repeat": route_of(repeat_t),
            "route_custom": route_of(custom_t),
            "k_per_phase": c.phases.iter().map(|ph| serde_json::json!({
                "phase": phase_name(ph.ordinal), "k": k_superseded(ph),
            })).collect::<Vec<_>>(),
            "auto_metal": {"open": open.metal, "reopen": reopen.metal},
            "candidate_ttl_hit": c.probe.map(|d| d.hits == 1),
            "mutex_box_bytes": serde_json::Value::Null,
            "mutex_box_note": "not observable from this harness (no allocator hook); budgeted <= 256 B",
        }),
    );
    let chain_json = |t: Option<&Txn>| {
        t.map(|t| {
            chain(t)
                .into_iter()
                .map(|(b, r)| format!("{b:?}={r:?}"))
                .collect::<Vec<_>>()
        })
    };
    let top_json = |t: Option<&Txn>| t.map(|t| format!("{:?}", top(t)));
    facts.insert(
        "reach".into(),
        serde_json::json!({
            "daylight_first": {"chain": chain_json(daylight_t), "top": top_json(daylight_t)},
            "daylight_repeat": {"chain": chain_json(repeat_t), "top": top_json(repeat_t)},
            "custom": {"chain": chain_json(custom_t), "top": top_json(custom_t)},
        }),
    );

    // Operator identity: fresh operator per open; calibration and display of
    // the selected backend share one operator.
    let op = |t: Option<&Txn>| t.map(|t| t.request.expected_operator);
    let (open_op, reopen_op) = (op(open_final), op(reopen_final));
    facts.insert(
        "operators".into(),
        serde_json::json!({
            "open": open_op, "reopen": reopen_op,
            "open_calibration": calibrations(open).iter().map(|t| t.request.expected_operator).collect::<Vec<_>>(),
            "reopen_calibration": reopen_cal.iter().map(|t| t.request.expected_operator).collect::<Vec<_>>(),
            "edits": [op(daylight_t), op(repeat_t), op(custom_t)],
        }),
    );
    if !v.inconclusive.is_empty() {
        return (v, serde_json::Value::Object(facts));
    }
    let (open_op, reopen_op) = (open_op.unwrap_or(0), reopen_op.unwrap_or(0));
    let (daylight_t, repeat_t, custom_t) = (
        daylight_t.expect("complete"),
        repeat_t.expect("complete"),
        custom_t.expect("complete"),
    );
    if open_op == reopen_op {
        v.contradicted.push(format!(
            "reopen reused operator {open_op}; a fresh operator was expected"
        ));
    }
    for (label, t) in [
        ("daylight_first", daylight_t),
        ("daylight_repeat", repeat_t),
        ("custom", custom_t),
    ] {
        if t.request.expected_operator != reopen_op {
            v.contradicted.push(format!(
                "{label}: operator {} is not the reopened session's {reopen_op}",
                t.request.expected_operator
            ));
        }
    }

    // Item 4: the first Daylight edit's route, then its highest reached bucket.
    if route_kind(daylight_t) == RouteKind::Tile {
        v.inconclusive
            .push("daylight_first: tile route (Inconclusive by design, rev7 §9.1)".into());
        return (v, serde_json::Value::Object(facts));
    }
    let (bucket, first) = top(daylight_t);
    // A reach that `reach` itself could not determine is no conclusion.
    for (label, t) in [
        ("daylight_first", daylight_t),
        ("daylight_repeat", repeat_t),
        ("custom", custom_t),
    ] {
        let (b, r) = top(t);
        if !matches!(r, Reach::Hit | Reach::Miss) {
            v.inconclusive
                .push(format!("{label}: reach {r:?} at {b:?} (undetermined)"));
        }
    }
    if !v.inconclusive.is_empty() {
        return (v, serde_json::Value::Object(facts));
    }
    // Stage D0 review NB-6: the edit's keys at the top bucket must have tile
    // levels that the priming calibration requested at that bucket (reopen
    // calibration for the baseline, first-open calibration for the
    // candidate). An unforeseen level mismatch is no conclusion, never a
    // contradiction. An empty request set is left to the priming checks.
    if let Some(b) = bucket {
        let calibration = if c.candidate {
            calibrations(open)
        } else {
            reopen_cal.clone()
        };
        let requested: Vec<u8> = calibration
            .iter()
            .flat_map(|t| keys(t, b, Outcome::Request))
            .map(|k| k.tile.level)
            .collect();
        let edited: Vec<u8> = [Outcome::Some, Outcome::None]
            .into_iter()
            .flat_map(|o| keys(daylight_t, b, o))
            .map(|k| k.tile.level)
            .collect();
        if !requested.is_empty() && edited.iter().any(|l| !requested.contains(l)) {
            v.inconclusive.push(format!(
                "daylight_first: {b:?} key levels {edited:?} not among calibration request levels {requested:?}"
            ));
            return (v, serde_json::Value::Object(facts));
        }
    }
    if c.candidate {
        // The fresh operator gets None wherever the edit reached.
        if first != Reach::Miss {
            v.contradicted.push(format!(
                "candidate daylight_first: {first:?} at {bucket:?}, expected Miss at every reached bucket"
            ));
        }
        if cal != 0 {
            v.contradicted.push(format!(
                "candidate reopen made {cal} calibration transactions after a TTL hit"
            ));
        }
    } else {
        // Item 3: the selected Metal operator's calibration (<= 3 renders)
        // requested the WB-dependent key later returned as Some.
        if cal > ITERATIONS {
            v.contradicted.push(format!(
                "baseline reopen made {cal} calibration transactions (> {ITERATIONS})"
            ));
        }
        if let Some(t) = reopen_cal
            .iter()
            .find(|t| t.request.expected_operator != reopen_op)
        {
            v.contradicted.push(format!(
                "baseline calibration operator {} is not the selected session's {reopen_op}",
                t.request.expected_operator
            ));
        }
        match (bucket, first) {
            (Some(b), Reach::Hit) => {
                let hit = keys(daylight_t, b, Outcome::Some);
                let requested: Vec<MemoKey> = reopen_cal
                    .iter()
                    .flat_map(|t| keys(t, b, Outcome::Request))
                    .collect();
                let unprimed: Vec<_> = hit.iter().filter(|k| !requested.contains(k)).collect();
                facts.insert(
                    "priming".into(),
                    serde_json::json!({
                        "bucket": format!("{b:?}"), "hit_keys": hit.len(),
                        "requested_by_calibration": requested.len(),
                        "unprimed_hits": unprimed.len(),
                        "calibration_iterations_requesting": reopen_cal.iter()
                            .filter(|t| keys(t, b, Outcome::Request).iter().any(|k| hit.contains(k)))
                            .map(|t| t.request.generation).collect::<Vec<_>>(),
                    }),
                );
                if hit.is_empty() || !unprimed.is_empty() {
                    v.contradicted.push(format!(
                        "baseline daylight_first: {} of {} {b:?} hits were not requested during calibration",
                        unprimed.len(),
                        hit.len()
                    ));
                }
            }
            (b, r) => v.contradicted.push(format!(
                "baseline daylight_first: {r:?} at {b:?}, expected a Hit on a calibration-requested key"
            )),
        }
    }
    // Item 5: repeat Some, distinct Custom misses.
    let (b, r) = top(repeat_t);
    if r != Reach::Hit {
        v.contradicted
            .push(format!("daylight_repeat: {r:?} at {b:?}, expected Hit"));
    }
    let (b, r) = top(custom_t);
    if r != Reach::Miss {
        v.contradicted
            .push(format!("custom: {r:?} at {b:?}, expected Miss"));
    }
    (v, serde_json::Value::Object(facts))
}

/// G12 body: capture (the driver asserts its protocol contracts before it
/// returns), persist the interpretation, then assert review items 1-9.
fn actual(candidate: bool, edr: bool) {
    let path = std::path::PathBuf::from(
        std::env::var_os("TESSERA_SMART_PREVIEW_RAW").expect("explicit read-only fixture required"),
    );
    let capture =
        real_phase_capture(candidate, edr, &path).expect("real phase instrumentation absent");
    let (verdict, facts) = interpret(&capture);
    let report = serde_json::json!({
        "variant": if candidate { "candidate" } else { "baseline" },
        "format": if edr { "edr" } else { "sdr" },
        "inconclusive": verdict.inconclusive,
        "invariant_violations": verdict.violated,
        "contradictions": verdict.contradicted,
        "facts": facts,
        "note": "diagnostic evidence only; no timing, performance, remedy or merge claim (rev7 §8)",
    });
    std::fs::write(
        capture.out.join("interpretation.json"),
        serde_json::to_vec_pretty(&report).expect("interpretation JSON"),
    )
    .expect("persist interpretation before asserting");
    // Stage D0 review NB-2: a correctness violation is never hidden behind
    // an inconclusive verdict, so it is asserted first (with both lists).
    assert!(
        verdict.violated.is_empty(),
        "WB-D INVARIANT VIOLATED (NB-2): {:?}; inconclusive: {:?}",
        verdict.violated,
        verdict.inconclusive
    );
    assert!(
        verdict.inconclusive.is_empty(),
        "WB-D INCONCLUSIVE (kept as observed, not rerun): {:?}",
        verdict.inconclusive
    );
    assert!(
        verdict.contradicted.is_empty(),
        "WB-D CONTRADICTED: {:?}",
        verdict.contradicted
    );
}
#[test]
#[ignore = "exclusive runtime (G12 grant): real Metal, read-only Sony ARW copy, --test-threads=1"]
fn actual_baseline_sdr_phase_capture() {
    actual(false, false);
}
#[test]
#[ignore = "exclusive runtime (G12 grant): real Metal, read-only Sony ARW copy, --test-threads=1"]
fn actual_baseline_edr_phase_capture() {
    actual(false, true);
}
#[test]
#[ignore = "exclusive runtime (G12 grant): real Metal, read-only Sony ARW copy, --test-threads=1"]
fn actual_candidate_sdr_phase_capture() {
    actual(true, false);
}
#[test]
#[ignore = "exclusive runtime (G12 grant): real Metal, read-only Sony ARW copy, --test-threads=1"]
fn actual_candidate_edr_phase_capture() {
    actual(true, true);
}

/// D0 pure contracts: the G12 interpretation over synthetic copied records.
/// No arena, engine or GPU.
mod interpretation {
    use super::*;
    use engine_api::{
        id::ImageId,
        stage::{ParamHash, StageId},
        tile::TileCoord,
    };
    const DAYLIGHT_WB: [u64; 9] = [1, 2, 3, 4, 5, 6, 7, 8, 9];
    fn key(bucket: Bucket, level: u8, seed: u32) -> MemoKey {
        let stage = match bucket {
            Bucket::Detail => StageId::Detail,
            _ => StageId::WhiteBalance,
        };
        MemoKey {
            image_id: ImageId(7),
            stage,
            params_hash: ParamHash::of(stage, &seed),
            tile: TileCoord::new(level, 0, 0),
        }
    }
    struct T {
        kind: u8,
        generation: u64,
        operator: u64,
        phase: u64,
        level: u8,
        wb: [u64; 9],
        route: RouteState,
        body: Vec<(Bucket, Outcome, u8, u32)>,
    }
    fn txn(t: T) -> Txn {
        let phase = match t.kind {
            KIND_CALIBRATION => PhaseKind::Calibration {
                iteration: t.generation as u8,
            },
            _ => PhaseKind::Develop {
                generation: t.generation,
            },
        };
        let request = RequestContext::new([1; 32], [2; 32], 3, 1, 0, t.level, phase, t.operator);
        let digest = [t.wb[0] as u8; 32];
        let begin = BeginContext::new(digest, 1, 0, t.level, t.operator).with_wb_bits(t.wb);
        let id = Identity {
            operator: t.operator,
            phase: t.phase,
            transaction: t.phase * 100 + t.generation,
            generation: request.generation,
        };
        let rec = |bucket, outcome, k| {
            Some(Record {
                identity: id,
                key: k,
                bucket,
                outcome,
            })
        };
        let mut records = vec![rec(
            Bucket::Detail,
            Outcome::Begin,
            key(Bucket::Detail, 0, 0),
        )];
        for (b, o, level, seed) in t.body {
            records.push(rec(b, o, key(b, level, seed)));
        }
        records.push(rec(Bucket::Detail, Outcome::End, key(Bucket::Detail, 0, 0)));
        Txn {
            token: String::new(),
            request,
            begin: Some(begin),
            route: t.route,
            loss_at_finish: 0,
            operator_matches: true,
            context: Context {
                recipe_fingerprint: [1; 32],
                resolved_wb: digest,
                output_tag: 1,
                headroom_bits: 0,
                render_level: t.level,
            },
            records,
            overflow: 0,
        }
    }
    fn develop(
        phase: u64,
        generation: u64,
        operator: u64,
        wb: [u64; 9],
        body: Vec<(Bucket, Outcome, u8, u32)>,
    ) -> Txn {
        txn(T {
            kind: KIND_DEVELOP,
            generation,
            operator,
            phase,
            level: 1,
            wb,
            route: RouteState::Delivered,
            body,
        })
    }
    /// Coarse route cold miss: Detail and padded miss, one tile miss, requests.
    fn cold(seed: u32) -> Vec<(Bucket, Outcome, u8, u32)> {
        vec![
            (Bucket::Detail, Outcome::None, 0, seed),
            (Bucket::PaddedWb, Outcome::None, 0, seed),
            (Bucket::TileWb, Outcome::None, 0, seed),
            (Bucket::TileWb, Outcome::Request, 0, seed),
            (Bucket::PaddedWb, Outcome::Request, 0, seed),
            (Bucket::Detail, Outcome::Request, 0, seed),
        ]
    }
    fn warm(seed: u32) -> Vec<(Bucket, Outcome, u8, u32)> {
        vec![(Bucket::Detail, Outcome::Some, 0, seed)]
    }
    fn phase(ordinal: u64, txns: Vec<Txn>, cpu: (u64, u64)) -> PhaseCapture {
        let generation = txns
            .iter()
            .filter(|t| t.request.phase_kind == KIND_DEVELOP)
            .map(|t| t.request.generation)
            .max()
            .unwrap_or(0);
        PhaseCapture {
            ordinal,
            counts_before: Counts {
                cpu_iterations_unobserved: cpu.0,
                ..Counts::default()
            },
            counts_after: Counts {
                cpu_iterations_unobserved: cpu.1,
                ..Counts::default()
            },
            quiescent: true,
            drained_all: true,
            txns,
            frames: vec![FrameSeen {
                generation,
                level: 1,
                is_final: true,
            }],
            final_generation: generation,
            metal: true,
        }
    }
    /// Seeds: 1 = AsShot chain, 2 = Daylight chain, 3 = Custom chain.
    fn calibration(operator: u64) -> Vec<Txn> {
        (0..3)
            .map(|i| {
                txn(T {
                    kind: KIND_CALIBRATION,
                    generation: i,
                    operator,
                    phase: REOPEN,
                    level: 0,
                    wb: if i == 2 { DAYLIGHT_WB } else { [9; 9] },
                    route: RouteState::Delivered,
                    body: if i == 2 { cold(2) } else { cold(1) },
                })
            })
            .collect()
    }
    fn capture(candidate: bool) -> Capture {
        let custom_wb = [3; 9];
        let (open_cal, reopen_cal, cpu) = if candidate {
            (calibration(1), Vec::new(), (3, 3))
        } else {
            (calibration(1), calibration(2), (3, 6))
        };
        let mut open_txns = open_cal;
        open_txns.push(develop(OPEN, 1, 1, [9; 9], cold(1)));
        let mut reopen_txns = reopen_cal;
        reopen_txns.push(develop(REOPEN, 2, 2, [9; 9], warm(1)));
        let daylight_body = if candidate { cold(2) } else { warm(2) };
        Capture {
            candidate,
            edr: false,
            automatic: true,
            out: std::path::PathBuf::new(),
            phases: vec![
                phase(OPEN, open_txns, (0, 3)),
                phase(REOPEN, reopen_txns, cpu),
                phase(
                    EXPOSURE,
                    vec![develop(EXPOSURE, 3, 2, [9; 9], cold(4))],
                    (cpu.1, cpu.1),
                ),
                phase(
                    DAYLIGHT,
                    vec![develop(DAYLIGHT, 4, 2, DAYLIGHT_WB, daylight_body)],
                    (cpu.1, cpu.1),
                ),
                phase(
                    RESTORE,
                    vec![develop(RESTORE, 5, 2, [9; 9], warm(4))],
                    (cpu.1, cpu.1),
                ),
                phase(
                    REPEAT,
                    vec![develop(REPEAT, 6, 2, DAYLIGHT_WB, warm(2))],
                    (cpu.1, cpu.1),
                ),
                phase(
                    CUSTOM,
                    vec![develop(CUSTOM, 7, 2, custom_wb, cold(3))],
                    (cpu.1, cpu.1),
                ),
                phase(
                    ORIGINAL,
                    vec![develop(ORIGINAL, 8, 2, [9; 9], warm(1))],
                    (cpu.1, cpu.1),
                ),
            ],
            probe: candidate.then_some(ProbeDelta {
                lookups: 1,
                hits: 1,
                measurements: 0,
                publications: 0,
                key_identical: true,
            }),
        }
    }
    fn at(c: &mut Capture, ordinal: u64) -> &mut PhaseCapture {
        c.phases.iter_mut().find(|p| p.ordinal == ordinal).unwrap()
    }
    #[test]
    fn supporting_baseline_and_candidate_records_are_proven() {
        for candidate in [false, true] {
            let (v, facts) = interpret(&capture(candidate));
            assert!(
                v.inconclusive.is_empty(),
                "{candidate}: {:?}",
                v.inconclusive
            );
            assert!(v.violated.is_empty(), "{candidate}: {:?}", v.violated);
            assert!(
                v.contradicted.is_empty(),
                "{candidate}: {:?}",
                v.contradicted
            );
            assert_eq!(facts["runtime"]["route_first_daylight"], "Coarse");
        }
    }
    #[test]
    fn candidate_hit_and_unprimed_baseline_hit_contradict() {
        let mut c = capture(true);
        at(&mut c, DAYLIGHT).txns = vec![develop(DAYLIGHT, 4, 2, DAYLIGHT_WB, warm(2))];
        assert!(!interpret(&c).0.contradicted.is_empty());
        let mut b = capture(false);
        // Baseline calibration never requested the Daylight chain.
        for t in &mut at(&mut b, REOPEN).txns {
            if t.request.phase_kind == KIND_CALIBRATION {
                *t = txn(T {
                    kind: KIND_CALIBRATION,
                    generation: t.request.generation,
                    operator: 2,
                    phase: REOPEN,
                    level: 0,
                    wb: [9; 9],
                    route: RouteState::Delivered,
                    body: cold(1),
                });
            }
        }
        let (v, _) = interpret(&b);
        assert!(v.inconclusive.is_empty(), "{:?}", v.inconclusive);
        assert!(
            v.contradicted
                .iter()
                .any(|m| m.contains("not requested during calibration"))
        );
    }
    #[test]
    fn incomplete_records_and_failed_admission_are_inconclusive() {
        let cases: [fn(&mut Capture); 6] = [
            |c| at(c, DAYLIGHT).txns[0].overflow = 1,
            |c| at(c, DAYLIGHT).txns[0].operator_matches = false,
            |c| at(c, DAYLIGHT).counts_after.loss = 1,
            |c| at(c, REOPEN).metal = false,
            |c| c.automatic = false,
            |c| {
                // Custom resolves to the Daylight matrix: never substituted.
                let daylight = at(c, DAYLIGHT).txns[0].begin;
                at(c, CUSTOM).txns[0].begin = daylight;
            },
        ];
        for (i, mutate) in cases.into_iter().enumerate() {
            for candidate in [false, true] {
                let mut c = capture(candidate);
                mutate(&mut c);
                assert!(
                    !interpret(&c).0.inconclusive.is_empty(),
                    "case {i} {candidate}"
                );
            }
        }
        let mut c = capture(true);
        c.probe = Some(ProbeDelta {
            hits: 0,
            measurements: 1,
            publications: 1,
            ..c.probe.unwrap()
        });
        assert!(!interpret(&c).0.inconclusive.is_empty(), "TTL miss");
        let mut b = capture(false);
        at(&mut b, REOPEN).counts_after.cpu_iterations_unobserved = 3;
        assert!(
            !interpret(&b).0.inconclusive.is_empty(),
            "baseline did not measure"
        );
    }
    #[test]
    fn tile_route_is_inconclusive_by_design() {
        let mut c = capture(false);
        at(&mut c, DAYLIGHT).txns = vec![develop(
            DAYLIGHT,
            4,
            2,
            DAYLIGHT_WB,
            vec![(Bucket::TileWb, Outcome::Some, 0, 2)],
        )];
        let (v, facts) = interpret(&c);
        assert_eq!(facts["runtime"]["route_first_daylight"], "Tile");
        assert!(v.inconclusive.iter().any(|m| m.contains("tile route")));
    }
    #[test]
    fn delivered_without_presented_frame_violates_nb2_and_aborts_are_only_recorded() {
        let mut c = capture(true);
        let p = at(&mut c, EXPOSURE);
        let mut superseded = develop(EXPOSURE, 2, 2, [9; 9], cold(4));
        superseded.request.generation = 99;
        p.txns.push(superseded);
        let (v, _) = interpret(&c);
        assert!(v.violated.iter().any(|m| m.contains("generation 99")));
        let mut c = capture(true);
        let mut aborted = develop(EXPOSURE, 2, 2, [9; 9], cold(4));
        aborted.request.generation = 98;
        aborted.route = RouteState::Aborted;
        at(&mut c, EXPOSURE).txns.push(aborted);
        let (v, facts) = interpret(&c);
        assert!(v.violated.is_empty() && v.contradicted.is_empty(), "{v:?}");
        assert_eq!(facts["post_bind_aborts_recorded"][0]["generation"], 98);
        assert_eq!(facts["runtime"]["k_per_phase"][2]["k"], 1);
    }
    /// Stage D0 review NB-3: inconclusive evidence is never read as contrary.
    #[test]
    fn inconclusive_capture_with_contrary_records_yields_no_contradiction() {
        // Candidate: a contrary Daylight hit, but a Custom transaction overflowed.
        let mut c = capture(true);
        at(&mut c, DAYLIGHT).txns = vec![develop(DAYLIGHT, 4, 2, DAYLIGHT_WB, warm(2))];
        at(&mut c, CUSTOM).txns[0].overflow = 1;
        // Baseline: a contrary Daylight miss, but the phase lost a reservation.
        let mut b = capture(false);
        at(&mut b, DAYLIGHT).txns = vec![develop(DAYLIGHT, 4, 2, DAYLIGHT_WB, cold(2))];
        at(&mut b, REPEAT).counts_after.loss = 1;
        for x in [c, b] {
            let (v, _) = interpret(&x);
            assert!(!v.inconclusive.is_empty(), "{}", x.candidate);
            assert!(
                v.contradicted.is_empty(),
                "{}: {:?}",
                x.candidate,
                v.contradicted
            );
        }
    }
    /// Stage D0 review NB-6: an edit/calibration tile-level mismatch at the
    /// top bucket is inconclusive, never contradicted.
    #[test]
    fn level_mismatch_is_inconclusive() {
        for candidate in [false, true] {
            let mut c = capture(candidate);
            let body = if candidate {
                vec![
                    (Bucket::Detail, Outcome::None, 1, 2),
                    (Bucket::PaddedWb, Outcome::None, 1, 2),
                    (Bucket::TileWb, Outcome::None, 1, 2),
                ]
            } else {
                vec![(Bucket::Detail, Outcome::Some, 1, 2)]
            };
            at(&mut c, DAYLIGHT).txns = vec![develop(DAYLIGHT, 4, 2, DAYLIGHT_WB, body)];
            let (v, _) = interpret(&c);
            assert!(
                v.inconclusive.iter().any(|m| m.contains("key levels")),
                "{candidate}: {v:?}"
            );
            assert!(v.contradicted.is_empty(), "{candidate}: {v:?}");
        }
    }
    /// An undetermined reach (for example a lookup without its enclosing
    /// miss) is inconclusive, never contradicted.
    #[test]
    fn undetermined_reach_is_inconclusive() {
        for candidate in [false, true] {
            let mut c = capture(candidate);
            at(&mut c, DAYLIGHT).txns = vec![develop(
                DAYLIGHT,
                4,
                2,
                DAYLIGHT_WB,
                vec![(Bucket::Detail, Outcome::None, 0, 2)],
            )];
            let (v, _) = interpret(&c);
            assert!(
                v.inconclusive.iter().any(|m| m.contains("undetermined")),
                "{v:?}"
            );
            assert!(v.contradicted.is_empty(), "{candidate}: {v:?}");
        }
    }
    #[test]
    fn repeat_miss_and_custom_hit_contradict() {
        let mut c = capture(false);
        at(&mut c, REPEAT).txns = vec![develop(REPEAT, 6, 2, DAYLIGHT_WB, cold(2))];
        at(&mut c, CUSTOM).txns = vec![develop(CUSTOM, 7, 2, [3; 9], warm(3))];
        let (v, _) = interpret(&c);
        assert!(
            v.contradicted
                .iter()
                .any(|m| m.starts_with("daylight_repeat"))
        );
        assert!(v.contradicted.iter().any(|m| m.starts_with("custom")));
    }
}

/// C6 (rev7 R4), baseline composition: must-pass non-regression guard. The
/// baseline (`5f31f148`) has no calibration controls (`SelectionControl`
/// exists only on the candidate), so its only in-test calibration bypass is
/// the unavailable-device path: under an armed epoch it returns CPU without
/// `measure_at`, making no reservation and leaving the CPU-iteration counter
/// unchanged. C2' (same C run) is the positive control for reservation
/// detection.
#[test]
fn calibration_controls_make_no_reservation() {
    use image_core::wb_diagnostic::{ARENA, State, harness::EpochGuard};
    let g = EpochGuard::open().expect(
        "EpochGuard: requires --test-threads=1 and an Idle ARENA (an earlier test may have leaked a live lease)",
    );
    g.epoch().arm(1).unwrap();
    use crate::backend::common;
    let image = common::synthetic(606, 64, 48, common::RGGB, [0, 0, 64, 48]);
    let before = ARENA.counts().expect("arena counts");
    assert_eq!(ARENA.state().unwrap(), State::Open);
    let backend = crate::backend::select_proxy(
        &image,
        &engine_api::recipe::DevelopSettings::default(),
        || None,
    );
    assert!(!backend.name.starts_with("Metal"), "DeviceUnavailable");
    let after = ARENA.counts().expect("arena counts");
    assert_eq!(ARENA.state().unwrap(), State::Open);
    assert_eq!(
        (after.reserved, after.active, after.completed),
        (0, 0, 0),
        "DeviceUnavailable"
    );
    assert_eq!(
        after.cpu_iterations_unobserved, before.cpu_iterations_unobserved,
        "DeviceUnavailable"
    );
    assert_eq!(after.loss, 0);
}
