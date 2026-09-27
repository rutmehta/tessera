# Cancellable PSD conversion primitive — 2026-09-27

Base `78492f7fef5963a72af5577a2ecc5a1c69f423ca` on
`codex/psd-conversion-cancellation`. The A-owned change adds
`compositor::psd::to_psd_with_cancel(&Document, &CancellationToken)` while
preserving the generic `PsdExport` contract. The caller token reaches the
merged render, node/raster/mask/channel traversal, vector-mask bridge tile
boundaries, placed-object renders, and recursive embedded conversion. Planar
and composite output products are checked before allocation. `Cancelled`
returns no partial PSD document; original document/retained source remain
untouched.

Each evidence directory contains exact candidate source bytes, SHA-256
manifest, runner, raw logs and exit JSON. Source identity was checked before
every command and after the final gate. All gates used the shared external
Cargo target, `CARGO_BUILD_JOBS=2`, `RAYON_NUM_THREADS=2`, deployment target
15.0, and a 600-second per-process-group watchdog. The runner stopped at the
first nonzero exit; no source changed during a running gate.

The first frozen gate `4aa71a620bb7` stopped at compile exit 101: the
preexisting `psd/vector.rs` caller of private `export_mask` lacked the new
token argument. The repair threads the same token through `vector::export_bridge`
and its tile boundaries, then into `export_mask`. Its complete failure log is
retained. The next frozen gate `0cfe3c58d75e` passed private cancellation tests
4/0 and placed private tests 2/0, then stopped at public-test compile exit 101:
the new test imported the external `vector` crate from `compositor` root. The
test-only import was corrected; its failure remains preserved. All three
production source hashes are identical between `0cfe3c58d75e` and final
`8adcc4d21e70`; the fourth manifest file is the changed test.

Final `8adcc4d21e70` gates:

| Gate | Result |
| --- | --- |
| New public PSD cancellation/byte tests | 5 passed, 0 failed |
| Existing `psd` | 16 passed, 0 failed |
| Existing `psd_channels` | 10 passed, 0 failed |
| Existing `psd_document` | 1 passed, 0 failed |
| Existing `m5_26_psd` | 12 passed, 0 failed |
| Existing `style_psd` | 5 passed, 0 failed |
| Existing `transform_psd` | 1 passed, 0 failed |
| Existing `text_vector_psd` | 10 passed, 0 failed |
| `cargo clippy -p compositor --all-targets -- -D warnings` | Exit 0 |

Together with the two private gates above, 66 tests passed, 0 failed. The
unchanged `text_vector_psd` suite covers the actual shape/vector-mask bridge;
the new public suite exercises it through the cancellable Document API and
asserts its mask channel. `cargo fmt -p compositor -- --check` and
`git diff --check` passed on final source. This validates the compositor
primitive, not B-owned FFI/Swift save cancellation. Current FFI Save Document
still calls generic `to_psd(doc)`; separate operation wiring must pass its
live token.

Metadata clones, text/vector rasterization internals, placed-raster copies,
linked metadata processing and embedded `PsdDocument::write` are boundary-
checked opaque operations. The writer and output allocation are not
preemptible or covered by a total-memory cap. No encoded-output latency or
global memory acceptance is claimed.
