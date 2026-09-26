# Architecture (current implementation)

Tessera is a Cargo workspace plus a native Mac app, not a single monolithic image editor. This map describes implemented boundaries; [STATUS.md](STATUS.md) records merged work and open gaps. The [product architecture spec](04-implementation-architecture.md) and [execution plan](11-execution-plan.md) include aspirations that must not be read as a current-feature checklist.

## Images, rendering and output

```text
RAW file → libraw-ffi/raw-decode → image-core tiled pyramid
         → pipeline-cpu (reference operators) / pipeline-gpu (wgpu/Metal)
         → stage-memoized scene tiles → colour-managed display or export
                                      ↘ resident GPU surface → Mac Metal loupe
```

[`engine-api`](../crates/engine-api/CONTRACTS.md) defines 256-pixel tiles, stage order, recipe/history, cache keys, job and tool contracts. [`libraw-ffi`](../crates/libraw-ffi/) and [`raw-decode`](../crates/raw-decode/) supply raw samples and metadata; [`image-core`](../crates/image-core/) orchestrates the progressive tiled stage graph and memoized intermediates, with [`jobs`](../crates/jobs/) scheduling/cancellation. [`pipeline-cpu`](../crates/pipeline-cpu/OPERATORS.md) is the reference for demosaic, white balance, tone, colour, detail, masks/local processing and output; [`pipeline-gpu`](../crates/pipeline-gpu/OPERATORS.md) ports supported paths and runs resident frames where possible. [`pipeline-adobe`](../crates/pipeline-adobe/) handles imported Adobe process-version compatibility. [`lens`](../crates/lens/) handles optical/geometry corrections; [`color-mgmt`](../crates/color-mgmt/) handles ICC profiles, soft proof and transforms. Some local edits and DNG opcode paths fall back or have integration gaps rather than silently being rendered by every GPU path.

[`previews`](../crates/previews/) caches pyramids; [`export`](../crates/export/README.md) renders, resizes and encodes JPEG/PNG/TIFF with metadata/profile handling; [`merge`](../crates/merge/README.md) covers HDR/panorama output. [`tessera-ffi`](../crates/tessera-ffi/) exposes commands and metadata to [`apps/mac`](../apps/mac/README.md) via UniFFI. The loupe uses IOSurface/Metal on supported resident paths rather than copying full pixel buffers over UniFFI; fallback rendering and export have their own paths. The app is AppKit for the grid/viewport/latency-sensitive controls and SwiftUI for other panels; its tokens live in [DESIGN.md](../apps/mac/DESIGN.md).

## Catalog, edits and culling

Folder scans and searches use [`index`](../crates/index/README.md), a rebuildable SQLite/FTS5 catalog. [`recipe`](../crates/recipe/) owns the versioned nondestructive edit document and history; [`sidecar`](../crates/sidecar/) persists its JSON and XMP beside the originals. [`library`](../crates/library/README.md) stores albums/groups/saved searches in `library.json`; [`cull`](../crates/cull/README.md) owns decisions, grouping and review signals. [`import-lrcat`](../crates/import-lrcat/README.md) reads Lightroom catalogs without writing to them and produces a plan for Tessera-side data; fidelity for undocumented Adobe fields still needs real source samples. [`tether`](../crates/tether/README.md) provides capture/ingest interfaces; camera hardware verification is outstanding.

## Models and automation

[`ml-runtime`](../crates/ml-runtime/README.md) loads the pinned [`models.toml`](../crates/ml-runtime/models.toml), verifies downloaded weights by SHA-256 and runs ONNX Runtime/CoreML or fallback CPU. Specialized crates own [faces](../crates/ml-faces/README.md), [quality](../crates/ml-quality/README.md), [embeddings](../crates/ml-embed/README.md), [segmentation](../crates/ml-segment/README.md), [depth](../crates/ml-depth/), [enhancement](../crates/ml-enhance/README.md) and [captions/OCR](../crates/ml-caption/README.md). [`mask-ai`](../crates/mask-ai/) bridges AI mask inference; procedural and local mask processing also lives in the pipeline/image graph. [`style-profile`](../crates/style-profile/README.md) learns editing preferences; [`agent`](../crates/agent/README.md) runs planner/critic base edits against engine tools. The [`tessera` CLI](../apps/tessera-cli/) drives headless workflows, and [`tessera-mcp`](../crates/tessera-mcp/README.md) serves JSON-RPC over stdio (including document tools). A remote planner, if configured, is distinct from the on-device model runtime.

## Layered documents

The [`compositor`](../crates/compositor/COMPOSITOR.md) holds the document scene graph, blend/adjustment math, tiled COW raster/history and resident GPU path through [`gpu-core`](../crates/gpu-core/). [`psd`](../crates/psd/README.md) reads/writes supported PSD/PSB interchange. [`filters`](../crates/filters/README.md), [`brush`](../crates/brush/), [`selection`](../crates/selection/) and [`vector`](../crates/vector/README.md) implement their respective engine tools. Text layers currently have a raster proxy in the compositor, not a completed Mac typography workflow. This engine work is not the same as a finished layered-document app UI.

## What is not built or finished yet

From the latest merged and next-work sections of [STATUS.md](STATUS.md) (whose older “Known gaps” list also includes items subsequently merged): the full Mac layered-editor window/panels and text-layer UI, compositor level-0/viewport performance target, and GPU-resident CFA handoff remain work in progress. The status also flags missing real Lightroom-exported sidecars for undocumented XMP fidelity and unfinished AI-mask export/fast local GPU integrations. JPEG/TIFF/HEIC Develop was listed next; physical tether camera verification and release signing/notarization require hardware/credentials. Vector work appears in the repository, but do not assume the Mac layered UI integrates it. Consult current code and [STATUS.md](STATUS.md) before describing any of these as shipped.
