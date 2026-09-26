# PSD/PSB interchange

Standalone `PsdDocument` intermediate representation for work package M5-02.
No dependency on `engine-api` or the concurrent compositor implementation.

## API and representation

- `PsdDocument::read(&[u8]) -> Result<PsdDocument>`
- `PsdDocument::write(&self) -> Result<Vec<u8>>`
- Public fields support construction and editing. Composite bytes and layer
  `Channel::data` are decoded planar bytes, with big-endian samples for 16/32-bit
  data. There is no implicit conversion to RGBA, working-space RGB, or premultiplied
  alpha. Bitmap rows are byte-aligned.
- `Version::{Psd,Psb}`, `ColorMode`, and `Compression` are explicit. The writer
  does not automatically upgrade PSD to PSB or choose another compression.
- `layer_section.layers` preserves file record order. `lsct` records describe
  folder starts/end dividers; `metadata::Group` exposes open/closed/bounding kinds
  and pass-through/isolated blend keys. Group records are not discarded or flattened.
- Blend modes remain four-byte Photoshop keys, including unknown future values.
  Opacity, clipping, all flag bits, Pascal name bytes, split Blend If ranges,
  raster mask data, and additional layer metadata survive write-back.
- `Layer::mask()` exposes mask rectangles, density/feather, and the real-user-mask
  extension. Channel IDs -2/-3 use mask rectangles, not the layer bounds.
- Resources preserve names, signatures, order, duplicate IDs and data. Convenience
  accessors expose ICC, resolution, selected layer IDs and saved path resources.
  Slices are not interpreted but are retained.
- Additional blocks retain signatures, order, duplicate keys and exact payload
  bytes. `metadata::parse` is an optional read-only interpretation; the file reader
  does not reject or discard opaque blocks merely because their optional semantic
  interpretation is unsupported. Metadata views never become the source for
  serialization, so parsing cannot lose unknown descriptor items.

## Coverage

| Area | Behavior |
| --- | --- |
| PSD/PSB | Header versions 1/2, color mode data, 1/8/16/32-bit storage; PSB 64-bit lengths and 32-bit RLE row counts |
| Channels/composite | Raw, PackBits RLE, zlib ZIP, ZIP prediction (8/16/32); per-row 16-bit word prediction and 32-bit shuffle/delta |
| High-depth layers | Main layer info and `Layr`/`Lr16`/`Lr32` global tagged layer info; edits written to the selected location |
| Names/groups/IDs | `luni`, `lsct`/`lsdk`, `lyid` typed views |
| Styles | `lfx2` descriptors with drop shadow/stroke/outer glow helpers; legacy `lrFX` common state/shadow/outer glow basics; entire source retained |
| Fills | `SoCo`/`GdFl`/`PtFl` typed kind and descriptor view |
| Adjustments | `levl`, `curv`, `brit`, `hue2`, `blnc`, `expA`, `vibA`, `phfl`, `blwh`, `mixr`, `selc`, `thrs`, `post`, `nvrt` classified and preserved byte-for-byte; also older hue, color lookup and gradient map keys |
| Smart objects | `SoLd`/`PlLd` descriptors; legacy lowercase `plLd`; embedded `liFD` originals in `lnkD`/`lnk2`/`lnk3`, including versioned child/asset fields |
| Text | `TySh` text and warp descriptors, text string, style runs/font/size; original EngineData borrowed intact; optional bounded basic EngineData typography decoder |
| Vector paths | `vmsk`/`vsms` flags and 26-byte path records with 8.24 coordinates; saved paths retained in image resources |
| Patterns | `Patt`/`Pat2`/`Pat3` headers and opaque virtual-memory array data |
| Future data | Unknown image resources and layer/global tags preserved opaquely |

## Boundaries

This is an interchange crate, not a Photoshop renderer. Adjustment numeric
parameters remain opaque; callers must decode them before applying compositor
operators. It does not rasterize styles, patterns, text, vector masks, fills or
smart objects. In particular, preserving an adjustment is not evidence of matching
Photoshop's rendered output. External/alias smart-object metadata is retained but
not followed or opened by the optional linked-file view. Unsupported descriptor
value types return an explicit semantic-view error, without affecting file
round-trip. EngineData support is basic typography, not full layout/inheritance.

A valid merged image is required. The writer uses the supplied composite; it does
not recompute a composite after edits. A caller modifying layers must also supply
a newly rendered merged image when appropriate.

This is a bounded in-memory codec, not a streaming large-document engine. Decoded
composites/individual channels are capped at 256 MiB and decoded layer samples at
512 MiB per layer-info section. PSB dimensions up to 300,000 are supported within
those limits. All reads and length arithmetic are checked; metadata recursion,
item counts and Unicode strings are bounded. Oversized input returns an error.

Payload fidelity is not whole-file byte identity: compression streams and padding
may be regenerated. Unknown payload bytes, resources, metadata ordering and
sample values are preserved. Callers should not change bit depth or file version
on opaque alternate-depth layer blocks without migrating those blocks as well.

## Compositor adapter boundary

The M5-01 brief proposes canvas/depth/profile plus a tree of pixel, group,
adjustment, fill, smart-object and text layers. A future thin adapter should:

1. Map canvas/depth and ICC resources without silently color-converting samples.
2. Build the layer tree from file-order `lsct` records and use `lyid` identities.
3. Convert planar channels into compositor tiled pixels and masks; map raw blend
   keys, opacity, clipping and Blend If. Retain this IR alongside the tree.
4. Use metadata views for supported layer kinds; keep original opaque data for
   unsupported semantics and export.
5. Request a fresh merged image on export after editing.

No compositor adapter is compiled against a guessed concurrent API.

## Verification

Run from the workspace root, retaining the externally supplied `CARGO_TARGET_DIR`:

```
cargo test -p psd --release
cargo clippy -p psd --all-targets -- -D warnings
cargo fmt -p psd --check
```

Tests include independent minimal file bytes for all four compression selectors,
randomized PackBits properties, hand-built layers, 8/16/32-bit channel/composite
round-trips, mask dimensions, every requested additional key, a 30,001-pixel PSB,
high-depth tagged layer storage, all truncation prefixes of synthetic/real files,
mutations/random data, and five permissively licensed Photoshop fixtures.
See `tests/fixtures/README.md` for provenance.

Specification consulted directly:
https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/PhotoshopFileFormats.htm
(November 2019). Adobe's documentation was downloaded for inspection, not vendored.
Dependencies are permissive: flate2/crc32fast/cfg-if (MIT OR Apache-2.0),
miniz_oxide (MIT OR Zlib OR Apache-2.0), adler2 (0BSD OR MIT OR Apache-2.0),
simd-adler32 (MIT). No GPL dependencies.
