# M5-36 candidate: transform performance and fidelity (after M5-35), from B5-12 NEEDS.md
- Warp and puppet geometry is per-pixel CPU: exact 20 MP preview 4.6 s; rasterised PSD copy of a 4-stage stack 33.6 s. Target: GPU displacement evaluation (reuse M5-21/M5-23 resident displacement stage) or tiled/parallel CPU with per-level caching; interactive preview < 50 ms at Fit.
- Content-aware protection masks stored inline in the stage (B5-12 caps at 4 MP): store as a raster reference (channel/raster store) instead.
- Warp grid always spans the whole smart-object canvas and output is clipped to it: bound the grid to the transformed content and allow output beyond the source canvas.
- Puppet mesh built from the original layer, not the stages below it.
- Layer thumbnails don't show transform stages.
- Acceptance numbers from Machine B's on-screen pass of B5-12 (pre-M2-56 build 108ed76): Puppet preview 5.5–6.0 s even on a 1200×800 layer; Warp drag preview median 365–522 ms, p95 905 ms. Targets: puppet mesh solve + preview < 100 ms for a 1200×800 layer and < 250 ms at 20 MP Fit level; warp drag preview median < 33 ms, p95 < 66 ms at the Fit level.
