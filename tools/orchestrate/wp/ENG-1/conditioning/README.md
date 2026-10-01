# ENG-1b conditioning attribution

`audit.py` makes a disposable archive of the current checkout, overlays the
committed capture tests, and renders the same fixtures twice. The first run
uses `tone_extra.rs` from `d01723659a7a6193ad1e323b311a56f72f7411f7`;
the second uses the current file. It verifies that the only production delta
between those files is the approved divisor floor and its constant/comment.
Instrumentation is injected only into the disposable copy, never the working
production source. The script does not update goldens.

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR=$HOME/.cache/tessera-target/ENG-1-texture-clarity
export CARGO_BUILD_JOBS=3
export RAYON_NUM_THREADS=3
python3 -m unittest discover -s tools/orchestrate/wp/ENG-1/conditioning -v
python3 tools/orchestrate/wp/ENG-1/conditioning/audit.py --output /tmp/eng1b-fresh
```

Choose a new output directory for each run. Python 3.12+ and the repo's normal
Rust/LibRaw build prerequisites are required. `--locked` forbids lockfile
updates. Both renders use the real `fixtures/raw` symlink target read-only.
All output files and logs stay in the requested directory; the disposable
source directory is removed when the run finishes, including on failure.
Existing captures can be rechecked without rebuilding with
`--verify-captures --output /tmp/eng1b-fresh`; this also requires successful,
non-skipped Cargo logs for both profiles and all five RAW files.

## Predicate and support proof

At the recombination divide in `pipeline-cpu/src/tone_extra.rs::presence`,
record pixel index and exact f32 bits of every **active** pre-fix divisor with
`0 < abs(L) < 1e-3`. This excludes the unchanged nonpositive and `out == z`
bypasses. These are the seed set S. The baseline and fixed traces must match.
`filters/src/camera_raw.rs::evaluate` caches the developed image using source,
profile, revision and Develop settings, independently of opacity. The 0.35
case therefore reuses the immediately preceding full case's trace when its
own trace is empty. The report records this as `trace_source`; only the final
pointwise encoded opacity blend differs. Zero opacity bypasses Develop and
has an empty seed set.
The radius predicate for output pixel p is:

```text
exists s in S: max(abs(p.x-s.x), abs(p.y-s.y)) <= 11
```

This bound follows from the operators, not the observed changed-pixel extent:

- Presence computes all guided bands from the same unmodified input in both
  versions. Its changed divide is pointwise, so the initial change has radius
  zero. The presence band radii do not enlarge the change's support.
- Dehaze follows presence in `tone_extra.rs::apply`. Its normalized dark-channel
  minimum uses `range(..., 3)`: Chebyshev radius 3. Its guided transmission uses
  `guided(..., 4, ...)`. `guided` computes local first/second moments and then
  averages coefficients, each with `mean(..., 4)`, giving radius 8. Combined
  transmission support is at most 3 + 8 = **11**. Changes in the guide itself
  have radius at most 8 and are included in that bound.
- Dehaze also has global airlight and confidence statistics. A local bound is
  valid here only because the audit checks exact equality of all three final
  airlight channel bits and the confidence bits between runs. A global-state
  mismatch is a blocker, never a license to expand support to the whole image.
- `image-core/src/rgb_render.rs` runs detail before tone. Following tone, the
  fixture's color operations, linear-mask local exposure/saturation, vignette,
  coordinate-derived grain, profile matrix/TRC, and encoded opacity blend are
  pointwise. Local texture/clarity/dehaze/sharpness/noise are neutral; depth blur
  is absent and geometry is identity. See `pipeline-cpu/src/locals.rs` and
  `geometry_effects.rs`. There is no additional downstream support radius.

The audit compares **every final RGBA f32 bit pattern**, with no tolerance for
identifying changed pixels. Alpha must be exactly unchanged and all samples
finite. `report.json` includes the count, maximum absolute encoded channel
delta, pre-fix divisors, equal global-state bits, and each changed pixel's
index, nearest-seed distance, and predicate result. Any outside pixel causes
failure before a golden can be accepted. The unit tests exercise radius-boundary
acceptance and rejection outside support, including a one-ulp change.

## Synthetic references

The prior three profile tests used only runtime Develop/filter parity; there
were no synthetic files to regenerate. ENG-1b first materializes all nine
pre-fix captures as little-endian, interleaved RGBA f32 files, 259 x 17 pixels,
then replaces them only after the attribution gate passes. Normal Rust tests
read these persistent references in addition to their existing independent
runtime parity and alpha/identity assertions. They retain the existing 1e-4
encoded tolerance for portability across LCMS/platform math. The attribution
audit itself never uses that tolerance.

`ENG1_CAPTURE` is exclusively a diagnostic capture mode; ordinary test gates
must run with it unset. It captures instead of asserting the persistent
reference, while retaining the original independent runtime parity checks.

## Photographic RAW references

`pipeline-cpu/tests/golden.rs::raw_fixture_goldens` enumerates every CR3, ARW,
NEF, RAF, and DNG fixture and compares each lens-off, 1/8-scale Develop render
to its checked-in RGB8 PNG with **zero tolerance**. The audit also captures both
versions and reports their per-pixel difference separately. These fixtures use
neutral texture/clarity, so their authorized change support is empty; any
before/after change is a blocker. They are never rewritten by the script.
