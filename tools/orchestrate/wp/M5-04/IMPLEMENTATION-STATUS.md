# M5-04 implementation status: incomplete

## Verified implementation

- `Document::from_psd(PsdDocument)` and `psd::to_psd(&Document)` plus the `ImportedPsd` editing wrapper.
- RGB pixel channel import/export in 8/16/32-bit, file-order/group-order conversion, pass-through and isolated groups, opacity/fill/clipping, Blend If, masks, Unicode names, document resources and retained opaque records keyed by compositor layer IDs.
- Exported native multilayer groups with clipping and masks render identically after PSD serialization/import in all three depths for the tested blend modes. Fixed new-layer export dropping mask channels.
- Editable PSD adjustments: Levels, Exposure, Invert, Posterize, Threshold, Channel Mixer (`mixr`, signed RGB/monochrome percentages), control-point Curves (version 1 RGB bitmaps/version 4 counts), and Hue/Saturation master/colorize (`hue2`). Selective hue bands and sampled curve maps stay opaque and unsupported rendering is reported in wrapper warnings.
- Curves read PSD output/input coordinates into compositor input/output points and export at PSD's 8-bit coordinate precision. Unedited blocks retain original bytes. Hue/Saturation master edits retain inactive parameters, band boundaries, and extension bytes. Explicitly replacing a selective-band adjustment clears its old band effects.
- Added regression coverage for both curve channel-selection formats, editable round trips, signed hue/colorize parameters, selective-band preservation/replacement, and native-vs-imported adjustment composites through PSD serialization at 8/16/32-bit depth.
- `GpuCompositor::from_device(device, queue, adapter)` permits device sharing without importing pipeline-gpu or LibRaw. A caller may clone pipeline-gpu's public context device and queue into it. No new gpu-core crate or automatic context wiring.
- Explicit `ResidentComposite` flat-stack API retains planar f32 GPU buffers by ID/revision, lazily builds alpha-weighted GPU mip chains, reuses step storage, dispatches whole levels or dirty rectangles, and supports all compositor adjustment variants as explicit GPU operations.
- Region uploads update only requested source rows and accumulate outward-rounded damage separately for every allocated mip level. Existing GPU mip buffers are retained and only damaged rectangles are recomputed. Pending higher-level damage survives frames requesting a lower level. `mip_texel_count()` exposes submitted pixel work; the regression compares every level bit-for-bit with a cold rebuild after multiple revisions and an odd-edge alpha edit.
- Resident CPU comparisons across blend modes and adjustments pass at 1e-4 in the tested small fixtures. Odd dimensions, revision reuse, dirty exterior preservation and 20MP extent creation are covered.

## Acceptance criteria NOT completed

- Resident storage uses buffers, not layer textures. Existing `render_tile` still uploads tile sources and rejects adjustments. Resident rendering is not wired to document traversal, history damage, masks, groups, clipping, Blend If or knockout.
- No 100-layer 20MP benchmark has been measured. Neither L2 <16ms nor L0 <100ms is established. A 20MP extent-creation test is not a throughput/memory acceptance test.
- Embedded smart-object originals are preserved opaquely; nested rendering uses the stored proxy rather than parsing the embedded original into an editable child document.
- Sampled curve maps and legacy `hue ` records are not converted to compositor adjustments yet. Selective Hue/Saturation is not implemented by the compositor.
- Text engine data is preserved but editing text descriptors is rejected. Vector masks, layer effects, procedural fill descriptors and mask feather are retained rather than fully rendered.
- Opaque PSD metadata survives Document edits/clones/history in memory but not native tessera-doc serialization. The native container has not been revised.
- No assertion that full docs/11 chain/performance gates have been met by the new resident API.

## Verification

Executed personally in the M5-04 worktree with `CARGO_TARGET_DIR=/Users/rutmehta/.cache/tessera-target/M5-04`:

```
cargo test -p compositor -p psd --release && cargo clippy -p compositor -p psd --all-targets -- -D warnings && cargo fmt --check
```

The complete required command exited 0 in this retry after fixing one Clippy iterator warning. This retry additionally exercised the new Channel Mixer fixture (observed failing before implementation, then passing), native mixer serialization/render parity at every supported depth, and the local mip damage regression. There are now 16 adapter tests and 8 resident GPU tests. The existing CPU benchmark is ignored by the required command. No commits. engine-api unchanged. Only allowed paths changed. `kanban_show()` could not resolve a task because this run has no `HERMES_KANBAN_TASK`; no board transition was made.

Wire-format references consulted for the adjustment additions: Adobe Photoshop File Formats Specification (November 2019), Curves/Additional Layer Information; psd-tools `src/psd_tools/psd/adjustments.py` for PSD-specific curve headers and `hue2` field offsets; ag-psd `src/additionalInfo.ts` (`mixr` handler) for the RGB versus monochrome row layout. Same-mode Channel Mixer edits preserve reserved fields, inactive rows, and trailing data. These tests establish agreement with this compositor's operators, not Photoshop rendering equivalence.

Overall work-package result: FAIL (remaining acceptance criteria above), despite passing implemented tests/lints.
