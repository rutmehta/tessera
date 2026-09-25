# Agentic base edits (M3-10)

`agent::Agent` builds a metadata/perception packet, asks a `Planner` for typed
`ToolRequest`s, validates the complete tool allowlist before executing, and calls
`tessera_mcp::Console`. Console persists normal reversible `Author::Agent` history
entries with rationales in `Agent base edit`. A CPU preview is rendered after each
step. Objective criticism accepts or asks for another plan (three iterations by
default). Rejected final edits remain reversible history and are explicitly marked
for review, never reported as accepted.

## CLI

    tessera agent edit photo.jpg --dry-run
    tessera agent edit shoot/ --provider anthropic --model claude-fable-5-1
    tessera agent edit photo.jpg --redo "warmer, keep the sky"
    tessera agent edit photo.jpg --provider openai --visual-critic

Output contains plans, rationales, metrics, acceptance, stop reason and a
lowest-confidence-first review queue. `--iterations` defaults to 3;
`--budget-seconds` defaults to 60. Dry-run can index/read/render and can contact an
explicit provider, but never executes edits or writes recipe sidecars. No provider
is selected implicitly from the presence of a key. Without `--provider`, execution
is offline style-profile only. `--provider none` is an explicit offline selection.
`--library` selects the profile saved using `style_profile::Profile::save`; a
missing profile uses a neutral questionnaire, not a fabricated learned style.

Cloud providers send metadata and a 512-pixel JPEG attachment. Their text packet
contains no RGB arrays or base64 preview. Credentials are loaded only from the
environment, never written to recipes or included in HTTP error messages.

- Anthropic: `ANTHROPIC_API_KEY`, `ANTHROPIC_MODEL` (default `claude-fable-5-1`),
  `ANTHROPIC_BASE_URL` (origin).
- OpenAI Responses: `OPENAI_API_KEY`, `OPENAI_MODEL` (default `gpt-4.1`),
  `OPENAI_BASE_URL` (including `/v1`). `store=false` is explicit.
- Ollama: `OLLAMA_HOST` (default `http://localhost:11434`), `OLLAMA_MODEL`
  (default `qwen2.5:7b`). Set `OLLAMA_VISION=true` only with a model supporting both
  vision and tools. Otherwise attachments are omitted and VLM critique is disabled.

`--visual-critic` requests a separate structured VLM judgement. It can reject an
objectively acceptable edit but cannot override an objective rejection. HTTP calls
have the remaining per-image timeout and redirects are disabled. CPU rendering and
catalog scans are synchronous and cannot be preempted mid-call: the deadline is
checked between operations, and expired jobs never report acceptance.

## Metrics and safety

Metrics include any-channel clipped-pixel fractions (not summed channel fractions),
linear Rec.709 mean luminance, p10/p50/p90, luminance standard deviation, and
`ml_quality` noise. Highlight clipping above 5% rejects. Luminance target/tolerance,
noise limit, mask/crop allowances, and an optional photographer-supplied Lab skin
band are configurable in `Config`. Skin distance is CIE76 distance outside the
configured interval using normalized face boxes. There is deliberately no universal
skin-colour target. Missing skin data is `None`, not a passing measured zero.

Generative tools, arbitrary preset application, catalog writes and export are not
allowed in plans. Body reshaping is always disallowed. Skin retouch is off by default;
enabling the allowance still results in Console's explicit unsupported error until
it has a reversible implementation. No fallback generates pixels.

The recipe extension `tessera_agent_v1` records `AI-assisted, non-generative edits`
and the report. Provenance is saved after each successful step, before rendering or
another network request can fail. This is recipe provenance, not a signed C2PA
credential. Callers must serialize editing with other writers, as for Console.

Redo is fail-closed: warmth/coolness/WB/tint instructions permit only temperature
and tint; brightness/exposure permits exposure; contrast permits contrast. Unknown
intent errors instead of granting broad access. Offline redo supports warmer/cooler.
“Keep the sky” preserves sky masks and other controls; global WB still changes the
rendered sky colour. Pixel-exact regional preservation requires a mask-aware future
redo policy and is not claimed here.

## Perception and batch integration

Scene labels use cached nearest-caption hints when supplied, otherwise explicitly
labelled histogram placeholders. The host may populate `Agent::perception_hints`
with embedding/model pairs, identified faces and depth availability. Missing data
remains unknown. Console supplies cached `ml-faces` geometry and `ml-quality`
signals. Style features are measured from the unedited source, not the current edit.

`BatchInput` reuses style-profile burst/person keys and `apply_batch` consensus.
Grouped base edits pin scene WB/tone and person exposure/texture to that consensus,
including during revisions, with a rationale documenting the constraint. This is
intentionally stronger than asking a language model to remain consistent. Grouped
plans cannot bypass constraints with local operators. CLI directory mode uses the
existing culling sequence groups; it does not invent persistent person identities.
A per-image redo intentionally overrides shoot consistency only for its scoped tools.

## Upstream integration gaps (engine-api unchanged)

- Console has no nearest-caption/embedding/depth/identity lookup. Its current
  FaceScore person IDs are `None`; host hints preserve real identities when known.
- Embedded EXIF is read with kamadak-exif, augmenting Console RAW camera/lens/
  exposure metadata. Absent or unparseable EXIF remains unavailable.
- Face boxes are source-normalized. General crop/rotation/lens-warp-aware face-box
  remapping and RAW orientation mapping need an engine transform API. Until then,
  use skin criticism only on geometry-aligned source previews.
- `set_tone` covers global basic tone/WB/vibrance/saturation, not all learned
  HSL/detail/curve fields. Offline style execution uses those exposed controls;
  the full prediction and rationales remain in the planner packet. A typed safe
  partial-settings/style tool is needed to apply the remaining learned controls.
- Full group amount/toggle UI and typed content credentials are future API/UI work;
  ordinary recipe undo/redo already works.

## Verification and dependencies

Offline tests exercise real Console sidecars and CPU renders with generated JPEG
fixtures. Provider wire tests replay fixed synthetic JSON fixtures, not live API
recordings. No network/provider availability or paid-model quality is claimed.

The new HTTP client uses reqwest 0.12 with default features disabled and
`blocking,json,rustls-tls`. Cargo package metadata was checked: reqwest/base64 and
tokio-rustls are MIT OR Apache-2.0; rustls/hyper-rustls are Apache-2.0 OR ISC OR MIT;
ring is Apache-2.0 AND ISC; webpki-roots is CDLA-Permissive-2.0. No new copyleft
requirement in these HTTP/TLS components. Existing workspace native/ML dependencies
retain their own licenses.

    cargo test -p agent -p tessera-cli --release
    cargo clippy -p agent -p tessera-cli --all-targets -- -D warnings
    cargo fmt --check

On this worktree keep `CARGO_TARGET_DIR=/Users/rutmehta/.cache/tessera-target/M3-10`.
