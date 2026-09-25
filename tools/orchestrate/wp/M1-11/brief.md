# WP M1-11 — `tessera` CLI (headless driver)

Read docs/11 §1.6, crates/index, cull, sidecar, previews, pipeline-cpu, raw-decode, import-lrcat, ml-runtime (all merged). Implement `apps/tessera-cli` (binary `tessera`, `clap` derive):
- `tessera index <dir> [--app-dir <path>]`: incremental scan; prints counts and ms. `tessera ls [--query <text>] [--decision keep|reject|undecided] [--json]`.
- `tessera cull set <image> --decision X|U|P [--grade 1|2|3] [--mark <name>]`, `tessera cull groups <dir>`, `tessera cull sweep <dir> --json` (from the cull crate).
- `tessera develop set <image> --exposure 0.5 --contrast 10 ...` (map CLI flags to `DevelopSettings` fields for the Basic panel), `tessera develop show <image> --json`.
- `tessera render <image> --out <png|jpg> [--scale 1/8|1/4|1/2|1] [--settings <json>]` via pipeline-cpu (use `image_core::Renderer` if merged by the time you start; otherwise pipeline-cpu, with a TODO).
- `tessera preview <image> --out <jpg> --max 1024` (embedded fast path, fallback to render).
- `tessera import lrcat <file.lrcat> --inspect | --apply --dest <dir>` (from import-lrcat).
- `tessera ml models` (registry list), `tessera ml check` (CoreML partition report for registered models).
- `tessera export ...` only if `crates/export` exists on main when you start; otherwise leave a stub subcommand that says not available yet.
- Exit codes, `--json` output for every read command, `--app-dir` default under `~/Library/Application Support/Tessera`.
- Tests: `assert_cmd` integration tests over fixtures/raw in a temp app dir: index → ls shows 5 → cull set → ls filters → develop set + show round trip → render 1/8 writes a PNG of the right size → preview of canon-cr3 yields a JPEG. Skip cleanly if fixtures are absent.
`cargo test -p tessera-cli --release`, clippy -D warnings, fmt.
