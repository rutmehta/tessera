# GPU benchmark (M0-07)

Standalone macOS/Apple-silicon crate, deliberately outside the root Cargo workspace.

```sh
export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/M0-07"
cd spikes/gpu-bench
cargo test --release
cargo build --release
cargo run --release
python3 validate_report.py
```

The normal run always measures the full 4096×4096 workload, three warmups and twenty measured executions per kernel/backend. No CPU fallback or wall-clock proxy substitutes for GPU timing. REPORT.md and report.json are generated in the crate directory. The extra unchecked WGSL measurements are diagnostic only, not used to relax the default wgpu acceptance gate. Shader compilation, allocation and initial uploads are outside the timed loop. Readback is included only in wall_ms.

The self-guided filter is channelwise on RGBA. Each box average is separable horizontal/vertical (two passes); the complete guided filter needs two such box averages, hence four dispatches. The CPU reference uses scalar f32 operations in parallel independent rows. This is a backend arithmetic comparison on deterministic synthetic inputs, not proof of quality for arbitrary camera images or optimal kernel performance. Retest on additional Apple GPUs before generalizing the result.

## HDR probe isolation

Each color-space configuration runs in its own fresh process with a hidden winit window. This matters because a native crash cannot be caught by a Rust panic handler or wgpu validation error scope. The display query is emitted before configuring so the parent preserves it on failure. The booleans indicate successful configuration, not demonstrated HDR presentation: a hidden window can return `Occluded` when acquiring a frame.

On the tested Apple M4, ExtendedSrgbLinear configured successfully. ExtendedDisplayP3 terminated with SIGBUS despite being advertised. LLDB located the crash in `CGColorSpaceCreateWithName`, called from `wgpu_hal::metal::Surface::configure`. Both calls to `Surface::display_hdr_info` reported current/potential headroom 1.0 and `high_dynamic_range: Some(false)`. Thus the compute recommendation is not an HDR presentation approval. Investigate the native P3 configuration path and retest on an HDR-capable display before relying on it.

The pre-existing in-repo target directory is ignored by git. All builds for this work package use the external CARGO_TARGET_DIR above.
