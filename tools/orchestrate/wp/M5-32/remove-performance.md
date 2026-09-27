# M5-32 CPU Remove performance

Status: RED benchmark added in `crates/filters/tests/m532_remove_perf.rs`; implementation not started pending runnable RED.

Build blocker (outside my ownership): `crates/compositor/src/resident/program.rs:493` matches `Adjustment::ColorLookup { size, data }` but new `source_filename` and `dither` fields require handling or `..`. Other initial compositor compile failures were fixed concurrently. Parent/compositor owner: please fix remaining pattern so I can run RED and continue.

Command: `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-32 cargo test -q -p filters --release --test m532_remove_perf -- --ignored --nocapture`.
