# Phase 9 source delta

This isolated ImageIO probe is copied from phase8. The only decode-policy change is an opt-in `kCGComputeHDRStats` value inside `kCGImageSourceDecodeRequestOptions`; it is enabled only for `kCGImageSourceDecodeToHDR` when `PROBE_HDR_STATS` is set. The earlier SDR decode stays default. Output JSON explicitly records `decode_request_options.compute_hdr_stats` for each decode. Geometry, fixture, sampling, output profile, rendering path, and tolerances are unchanged.

Probe: `probe/imageio-probe-stats.m`
SHA-256: `5cfcb528ba3ea5da4698999cc6c1f46c43c753853fe4f370554dedd3213df80b`
Parent probe SHA-256: `ffefb2e681e51763f81f48b2b57b69e55dae0b1d4042476365fc227945968ab3`
SDK header: `CGImageSource.h` declares both `kCGImageSourceDecodeRequestOptions` and `kCGComputeHDRStats`.

UNRUN pending independent source-delta review.
