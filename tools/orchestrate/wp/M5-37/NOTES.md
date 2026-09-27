# M5-37 candidate: document render engine APIs (from B5-14 NEEDS.md on origin/wp/B5-14)
1. Document snapshot that keeps its cache key: an edit landing mid-frame changes the key today, so the next frame recomposites the whole viewport. Snapshot/COW with stable per-layer revision keys so unchanged tiles are reused.
2. Public jobs API to register interactive pressure (B5-14 queues placeholder hold jobs on a private pool). Extend crates/jobs pressure.rs with a documented register/unregister (RAII guard) usable by tessera-ffi document workers; export and preview pools yield to it.
3. Rendered region reported in FrameInfo (visible rect + halo actually composited), so hosts and timing spans can verify viewport-only rendering.
4. Investigate memory: app footprint 3.3 GB vs 1.6 GB after B5-14's run — likely GPU mip chains retained for thumbnails; add budgets/eviction.
5. Pans regressed 1.2 → 10 ms (bench), 3.5 → 18 ms (app) with viewport rendering: reuse overlapping tiles on pan (resident render_viewport already GPU-copies overlap per M5-16) and prefetch a margin.
- M2-56 follow-up: ShellBudget arithmetic reads screen-derived values (CI: inspectorFit 380 vs 576 locally); make the budget a pure function of window size so testShellBudgetYieldOrder can run on CI.
