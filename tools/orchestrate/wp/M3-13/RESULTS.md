# M3-13 verification

Implemented the `ml-caption` crate, the 2,080-concept original vocabulary,
SigLIP independent temperature-scaled suggestions, pinned MIT Florence-2-base-ft
ONNX caption/alt-text/OCR inference, keyword-tree name/synonym mapping, explicit
bulk acceptance and optional XMP writes, Priority::Score batched jobs, index v6
storage/FTS, and `tessera ml keywords|caption|ocr` commands.

## Final command

Executed in this worktree with
`CARGO_TARGET_DIR=/Users/rutmehta/.cache/tessera-target/M3-13`:

```
cargo test -p ml-caption -p index -p tessera-cli --release && cargo clippy -p ml-caption -p tessera-cli --all-targets -- -D warnings && cargo fmt --check
```

Exit 0. Release test summaries: 87 passed, 0 failed, 1 ignored (existing index
benchmark). Clippy and workspace formatting passed. Native LibRaw compiler
warnings are pre-existing and do not represent new Rust clippy findings.
The local full log is `verification-final.log` (ignored).

## Real model and CLI checks

- Downloaded and hash-verified actual Florence and SigLIP cached artifacts; new
  caption/OCR and zero-shot integration tests ran with those weights, not skips.
- Fontdue-rendered `HELLO WORLD` recovered exactly with a normalized region box.
- Synthetic red disc ranked `circle` first, `red` second in a ten-label vocabulary.
- RAW fixture caption/alt text generated successfully. See COREML.md for actual
  CoreML/CPU partitions, timings, observed captions and fallback limitations.
- Release CLI caption stored output and preserved it when keywords were stored.
- Release CLI OCR recovered `HELLO WORLD` from a rendered JPEG; `ls --query HELLO`
  found that indexed image through FTS.
- Release CLI explicit keyword acceptance populated Suggested hierarchy and
  created no XMP by default. Repeating with `--write-xmp` wrote verified
  Suggested|cube, Suggested|table and Suggested|red paths. SHA-256 comparison
  confirmed the copied RAW original remained byte-for-byte unchanged.
- Scope check found no modifications outside permitted paths. No engine-api
  changes, committed model weights, target directories, or commits.

## Known limits (not hidden as test successes)

Confidence defaults are heuristic temperature scaling, not held-out probability
calibration. OCR confidence is sequence-level, shared across detected regions.
Florence uses bounded greedy full-prefix decoding without a KV cache; dense text
can exceed the token budget and returns an error. Dynamic CoreML graphs retain
CPU partitions and expensive initialization, so CLI CPU is the default.
Background jobs are exposed for explicit scheduler submission, not automatically
enabled in unrelated indexing/engine call sites. PNG inference is supported,
but the existing catalog scanner does not index PNG, so use indexed JPEG/RAW
inputs for `--store`. These constraints are documented in the crate README.
