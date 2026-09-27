# PSD conversion cancellation candidate (unvalidated)

Base: `78492f7fef5963a72af5577a2ecc5a1c69f423ca` on `codex/psd-conversion-cancellation`.
The three source files and their hashes are frozen in `manifest.json`. No compiler
or test gate has run for this snapshot yet; `cargo fmt -p compositor -- --check`
and `git diff --check` passed before freezing.

The public `to_psd_with_cancel(&Document, &CancellationToken)` shares conversion
internals with the unchanged generic `PsdExport` contract. The caller's token
reaches merged rendering, node traversal, raster/mask/channel assembly, both
placed-object renders, and recursive conversion of embedded PSD sources. The
export returns `Cancelled` without publishing a partial `PsdDocument`. Its
source `Document` and retained metadata are not mutated.

Checks occur at entry, before large output buffers, between nodes/channels,
each row, every 1,024 pixels in long loops, and before success. Multiplication
for the planar and composite output sizes is checked. Metadata cloning,
`rasterize_layer`, vector/text helpers, `Raster::edit_region` in placed export,
linked metadata processing, and embedded `PsdDocument::write` remain boundary
checked opaque operations. This is neither a total memory limit nor an encoded
output writer cancellation guarantee.

The current FFI Save Document caller still uses generic `to_psd(doc)` and a
fresh token. The B-owned operation/FFI/Swift slice must call the new API with
its live token before app-level save can claim caller cancellation.

The public tests assert exact planar bytes for a nonempty colored/alpha raster
at U8/U16/F32, wrapper agreement, pre-cancellation and retry, retained opaque
resources, and a tiny embedded placement. Private tests inject cancellation at
node, saved-channel, row, and recursive embedded-conversion checkpoints, and
check arithmetic overflow. Existing PSD roundtrip targets are in `run_gate.py`.
The runner must only be launched after the parent grants the A compiler slot.
