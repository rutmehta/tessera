# Rulings addendum — 2026-10-02 (Claude A coordinator)

Continues the "Rulings log" in CODEX-BRIEF-2026-09-30.md.

- **Diagnostics channel has three writers, all in `import_lrcat::diagnostics`:** `push_approximate` (info / approximate / field), `push_ignored` (info / ignored, no field, never shown in the import report) and `push_cloud` (warning / cloud) for edits that require Adobe cloud processing and are not rendered (generative remove/fill, distraction removal). Cloud entries feed the "Requires Adobe cloud (not rendered)" report group and are never removed by no-op suppression. The translation matrix gains the status `cloud`. No other code writes the diagnostics key.
- **Ignored vs cloud:** `ignored` is only for values with no visual effect; anything that changes the image in Lightroom but not in Tessera must be visible to the user (warning or cloud group).
- **ENG-3 floor:** continuous absolute-weighted cancellation floor, D = max(|Y|, ε·clamp(1 − ρ/k, 0, 1)), ρ = |Y| / (0.2627|r| + 0.678|g| + 0.0593|b|), k = 0.25, ε = 1e-3; the switch is at ρ* = k(1 − |Y|/ε).
- **Native pipeline does not apply DNG BaselineExposure; the Adobe pipeline and smart-preview proxies do** (LR-8d). The LR-10 embedded-profile fallback applies only to smart-preview proxies whose recipe names an uninstalled Adobe profile.
- **LR-5 AI masks:** an unavailable AI component (pending, failed, no model) skips its whole local adjustment, never renders full-frame or is ignored; person sub-parts and specific-person instances are unsupported (warn + retain), not broadened to whole-subject.
- **Wall-clock tests** are release-only; gates run serially; a 1 s spawn bound under load is a test-design problem.
- **Attribution:** commits carry a `Co-Authored-By: Claude …` trailer only when a Claude model wrote them; mixed authorship is stated in the body.
