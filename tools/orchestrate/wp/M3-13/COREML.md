# M3-13 actual partition audit

Executed on macOS Apple Silicon with the pinned fp16 Florence-2-base-ft graphs
through the unchanged ml-runtime profiler. Commands:

```
TESSERA_CAPTION_COREML=1 cargo test -p ml-caption --test florence -- --nocapture
cargo test -p ml-caption --test florence -- --nocapture
cargo test -p ml-caption --test siglip -- --nocapture
```

Florence CoreML opt-in run passed in 372.09 seconds (debug build). Apple emitted
unbounded-dimension and scalar squeeze compilation diagnostics; ORT retained
unsupported nodes on CPU. These are measured optimized executed node counts,
not percentages of FLOPs or claims of all-CoreML execution:

| Graph | CoreML fused nodes | CPU nodes |
|---|---:|---:|
| vision | 53 | 151 |
| embed | 1 | 0 |
| encoder | 15 | 32 |
| decoder | 28 | 63 |

CPU-only Florence run passed in 31.56 seconds: vision 2559, embed 2, encoder 619,
decoder 1107 CPU nodes. CPU is therefore the CLI default for this dynamic decoder.
CoreML is opt-in; compiling many dynamic subgraphs increases startup cost and
peak temporary disk usage significantly. No claim of accelerator speedup.

Both Florence runs recovered the fontdue-rendered `HELLO WORLD`. The CPU run
reported bbox [0.0835, 0.3885, 0.7685, 0.6375], confidence 0.45158756. The Sony RAW
fixture generated `A colorful rubik's cube sitting on a wooden table.` with alt
text `In this image we can see a colorful cube on the wooden surface.`

The synthetic red-disc SigLIP test ran real cached weights on CPU, ranking
`circle` first and `red` second among ten concepts. Observed CPU partitions:
vision 719 nodes, text 674 nodes. The reusable SigLIP loader retains its existing
NeuralNetwork-format CoreML support; this report does not relabel its CPU run as
an accelerator audit. Raw logs are local ignored *.log files; no weights or
machine-specific temporary CoreML bundles are committed.
