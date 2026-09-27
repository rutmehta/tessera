# Machine B timing: exact 441da3e — failed six-run gate

Machine A executed this verification over authenticated SSH; this is not receipt by the B chat. Candidate: `441da3e3f83f46ee04536fa8b6248c2519d6d619`. B checkout `/Users/rutmehta/Developer/lightroom/.worktrees/B5-16a` is now clean and detached at the candidate. Original `wp/B5-16a` remains `bcd0e792f248b3d5ce20ea4a121b9c20e61c7d6c`; other B document checkouts were untouched.

Hardware: Apple M4 Max, Mac16,5, 16 cores, 48 GiB; macOS 26.1 (25B78), rustc/cargo 1.98.1. Existing internal cache `/Users/rutmehta/.cache/tessera-target/B5-15`, two build jobs, deployment target15.0. Separate no-run compilation passed in28.27s. Six subsequent exact tests used fresh processes and unchanged source/tolerances. Raw commands/exits/timestamps are in `manifest.json`.

| Run | CPU cold ms | CPU warm ms | Resident cold ms | Resident warm ms | Result |
|---|---:|---:|---:|---:|---|
| original-1 | 73.845208 | 79.511792 | 106.084084 | 3.644375 | FAIL (101) |
| original-2 | 98.419125 | 66.406833 | 27.69075 | 2.406083 | pass (0) |
| original-3 | 73.605792 | 59.979958 | 29.10125 | 3.541333 | pass (0) |
| unique-1 | 78.594875 | 65.436083 | 33.438333 | 3.806792 | pass (0) |
| unique-2 | 70.961917 | 56.423625 | 32.494459 | 3.63075 | pass (0) |
| unique-3 | 74.683208 | 64.673959 | 52.725292 | 4.094334 | pass (0) |

The first original fixture failed its resident cold <100ms assertion at106.084084ms. The remaining five pass. All CPU cold/warm values meet<2s and all resident warm values meet<100ms. No failed sample was replaced, dropped, or excused; no fourth samples. This does not clear either this gate or B's previous first-process125.855125ms failure on9f922bf.

Conditions: no competing Cargo/Rust/Swift/Tessera/benchmark process before compile or any timing. Background processes remained untouched, including PID27454 `yes` (~100%CPU, parent1, cwd B5-15/crates/tessera-ffi), WindowServer, browsers, and simulator services. High host load was recorded; it is a condition, not an asserted cause or waiver. Each run has its own full process/load/thermal sample. Native thermal commands were read-only. Tests are synthetic isolated viewport timings, not app input-to-present or export results.

Runner finished2026-09-27T18:13:06.255608Z. Remote ownership note updated to completed; B heavy slot released. All remote evidence files copied to this A directory and SHA256-verified against remote bytes (`sha256.json`). No Codex writer takeover, chat mutation, global configuration, or background-process termination occurred.
