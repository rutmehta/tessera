# M2-09 implementation handoff

Status: partial implementation; verification green, full requested coverage incomplete.
No commits. engine-api unchanged. Build target remains outside the repository at
`/Users/rutmehta/.cache/tessera-target/M2-09`.

## Delivered

- New workspace package `lens`: Brown–Conrady model, Lensfun XML and user LCP
  readers, profile interpolation/fuzzy lens matching, JSON user profile persistence.
- Synthetic-tested image line/CA/vignette calibration and RANSAC/guided Upright.
- Owned DNG opcode bytes in decoder metadata plus bounded standalone TIFF
  extraction and opcode parser. Vendor coverage limitations explicitly documented.
- CPU profile priority and caller-owned profile context, manual distortion,
  vignette and hue-selective defringe. Exact supplied Bayer profile CA occurs on
  same-color CFA sublattices; other CA uses camera RGB before matrix/tone.
- Common lens distortion, Upright, user transform and crop share one inverse
  Lanczos3 geometry resample. Off-path immutable goldens retained.
- Lensfun optional separate data-pack attribution/license policy in docs/13.

## Parent-executed verification

Exact required chain exited 0:
`cargo test -p lens -p pipeline-cpu -p raw-decode -p libraw-ffi --release && cargo clippy -p lens -p pipeline-cpu -p raw-decode --all-targets -- -D warnings && cargo fmt --check`

125 tests passed, zero failed, one external-fixture test ignored by default.
That ignored test was separately executed by the parent:
`cargo test -q -p pipeline-cpu --release --test lens_fixtures -- --ignored --nocapture`
It passed against all five actual files: Canon CR3 500x500, Sony ARW 615x410,
Nikon NEF 923x616, Fuji RAF 612x408, DNG 652x434. Every output finite with
Auto lens and Auto Upright. All five have no opcode payloads, so this is not
real-file embedded-calibration accuracy evidence. Synthetic opcode tests provide
parser/geometry coverage. Existing LibRaw C++ warnings remain; Rust clippy passes.

## Retry verification and repair

This retry reproduced and fixed loss of measured distortion/TCA focal lengths
when loading independent Lensfun calibration grids. The new regression failed
on the inherited loader, then passed after independent component interpolation
onto a shared union grid. The required release-test/clippy/fmt chain was rerun
after the code change and exited 0. `verification.log` is from this retry.
The five-fixture opt-in render was also rerun successfully; see
`fixture-verification.log`. All changed/untracked paths satisfy the allowlist,
no in-repository target exists, and no commits were made.

Overall WP result remains FAIL for the outstanding acceptance gaps below,
not because of compiler warnings or failing tests. No Kanban lifecycle write
was possible: the session has no HERMES_KANBAN_TASK and kanban_show returned
"task_id is required".

## Outstanding acceptance gaps

- Parsed OpcodeList1/2 lens operations are explicitly unsupported by the renderer,
  not executed at their DNG stages. List3 execution is a restricted subset.
- Lensfun physical crop/aspect normalization requires caller adaptation. LCP
  coordinate conventions remain incomplete. Camera Make/Model restriction and
  fuzzy lens selection are now wired; heterogeneous multi-camera LCP files and
  Lensfun mount/crop-factor compatibility remain unsupported.
- A separate download source/license policy is documented, but no versioned
  downloadable Tessera data-pack artifact/updater is shipped.
- Independent persistent manual red/blue CA controls need schema fields; available
  profile CA-strength controls and caller-supplied profiles are not equivalent.
  See crates/pipeline-cpu/MISSING_FIELDS.md for requested fields.
- No proprietary correction coefficients are exposed by bundled LibRaw for the
  inspected vendor structures; identity/status fields are not fabricated as data.

Detailed contracts and scope: crates/lens/API.md, crates/raw-decode/LENS_M2.md,
crates/pipeline-cpu/LENS_M2.md. Full verification output: verification.log.

## Camera identity retry

Fixed LCP Make being misinterpreted as lens manufacturer and CPU database lookup
assuming camera manufacturer equals lens manufacturer. Profiles now retain an
optional camera identity with backward-compatible JSON loading. Camera-aware
lookup accepts normalized matching camera identities and fuzzy lens models,
rejects wrong-camera profiles, and declines equal-ranked ambiguous matches.
Tests reproduced the old failures before implementation. Updated the prior LCP
property-element test to assert that Make is camera metadata, not lens metadata.
The exact required release/clippy/fmt chain exited 0 after these changes, with
125 passing tests and one default-ignored fixture test. Separately executed that
test: all five RAW fixtures rendered finite with Auto lens/Upright. Logs were
refreshed. Allowlist and git diff --check passed; no local target or commit.
Full WP status remains partial for the acceptance gaps listed above.
