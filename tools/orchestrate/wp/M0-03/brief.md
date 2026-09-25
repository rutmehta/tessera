# WP M0-03 — engine-api contracts (Opus)

Read docs/04, 05, 06, 07, 08, 10 and docs/11-execution-plan.md. Produce `crates/engine-api/` as a standalone crate (own Cargo.toml, do not touch root Cargo.toml which may not exist yet) containing the shared contracts every other crate will link against. No implementations beyond what is needed for the types to be usable and tested:
- `tile`: `TileCoord {level, x, y}`, `TileFormat {F32Planar, F16Planar, U16, U8}`, `Tile` (Arc'd copy-on-write buffer, halo width), `Pyramid` trait.
- `color`: `WorkingSpace` (linear Rec.2020 default), `ColorMatrix3`, `WhitePoint`, `Illuminant`, ICC profile handle newtype.
- `recipe`: versioned `Recipe` struct: ordered stage parameters mirroring the pipeline order in docs/04 §3; `crs` compatibility enum listing every Adobe crs: key named in docs/05 §3.2 with its type; `Selection {decision, grade, mark}` per docs/06 §2; `History` append-only entries with named snapshots; `recipe_hash()` deterministic over the current state (blake3 or sha2). serde JSON with `#[serde(default)]` everywhere for forward-compat; round-trip tests.
- `jobs`: `Job` trait, `Priority {Ui, Viewport, Prefetch, Preview, Score, Export}`, `Cancellation` token, `Scheduler` trait.
- `stage`: `StageId` enum in fixed order, `StageParams` trait with `param_hash()`, `MemoKey {image_id, stage, params_hash, tile, level}`.
- `tools`: the typed tool API surface from docs/10 §2 as an enum of commands + result types (`SetTone`, `CreateMask`, `AdjustMask`, `RemoveObject`, `RetouchSkin`, `ApplyStyle`, `Crop`, `Compare`, `GetHistogram`, `GetScores`, plus library ops: index folder, set selection, export). serde-tagged so it can serve as the MCP schema later.
- `error`: one `EngineError` enum.
Write `crates/engine-api/CONTRACTS.md` explaining each module in a paragraph and the invariants other crates must keep. Tests + `cargo clippy -- -D warnings` clean. Taste matters: names should read like a mature engine, not a prototype.
