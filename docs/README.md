# Tessera — Product and Architecture Documentation

Tessera means a single tile of a mosaic, reflecting the image-tile foundation of the engine.

Written 2026-09-24. Baselines: Lightroom Classic 15.x / Camera Raw 18.x, Photoshop 27.x.

Documents 01–10 are product specs, not a list of shipped features. For the implemented crate/data-flow map and current gaps, see [ARCHITECTURE.md](ARCHITECTURE.md) and [STATUS.md](STATUS.md).

| Doc | Purpose |
|---|---|
| [01-lightroom-classic-spec.md](01-lightroom-classic-spec.md) | Every Lightroom Classic feature (Library, Develop, Map, Book, Slideshow, Print, Web, Export, tethering, sync) with data model and implementation notes |
| [02-photoshop-spec.md](02-photoshop-spec.md) | Every Photoshop feature (document model, compositor, selections, brushes, retouching, transforms, adjustments, filters, type/vector, generative AI, automation, output, collaboration) with implementation notes |
| [03-competitive-analysis.md](03-competitive-analysis.md) | Competitor-by-competitor comparison and the consolidated list of features rivals have that Adobe does not |
| [04-implementation-architecture.md](04-implementation-architecture.md) | One shared engine (image core, raw pipeline, compositor, ML runtime, catalog) that can back both apps |
| [05-catalog-storage-and-import.md](05-catalog-storage-and-import.md) | Why we don't use `.lrcat`; sidecar-first storage with a rebuildable index; full Lightroom Classic catalog import |
| [06-culling-and-selection.md](06-culling-and-selection.md) | Replacement for flags/stars/labels: decisions, scores, tags; culling UX; what to build for collections |
| [07-image-quality-and-color.md](07-image-quality-and-color.md) | SOTA raw pipeline, camera/lens support, optics correction, camera profiling, display colour management |
| [08-performance.md](08-performance.md) | Performance targets versus Lightroom Classic/Photoshop and the architecture that meets them |
| [09-ai-features.md](09-ai-features.md) | Complete on-device AI inventory (detection, recognition, segmentation, scoring, removal, retouch); generative as phase 2 |
| [10-agentic-editing.md](10-agentic-editing.md) | Agent produces base edits through the engine's tool API; Tessera is for fine-tuning |
| [11-execution-plan.md](11-execution-plan.md) | Build plan and GPU/CPU golden tolerances (some milestones are historical) |
| [12-stack-explainer.md](12-stack-explainer.md) | Rust, Metal/wgpu and Swift stack rationale |
| [13-licensing.md](13-licensing.md) | Apache-2.0, DCO and dependency policy |
| [ARCHITECTURE.md](ARCHITECTURE.md) | Implemented crate map, data flow and unfinished areas |
| [STATUS.md](STATUS.md) | Merged work and remaining gaps; newer sections supersede older lists |
| [research/](research/) | Raw research notes with source URLs (current Adobe releases, raster competitors, raw/DAM competitors) |
