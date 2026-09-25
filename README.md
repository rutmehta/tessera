# Lightroom: Photoshop

A shared Rust engine for raw photo development, cataloguing, previews, culling, and layered image editing.

The workspace separates core image data, CPU/GPU pipelines, catalog services, editing recipes, and app-facing APIs.

## Build

Install the stable Rust toolchain with rustfmt and clippy, then run:

```sh
cargo build --workspace
bash ci.sh
```

The CLI target is `pe-cli`. Workspace modules are under `crates/`.

## Documentation

See [docs/](docs/README.md) for product and architecture notes.
