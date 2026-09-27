# M2-54 results

## Disposition

The scoped implementation and required build/test gate are verified. Overall performance acceptance is **not complete**: the model-operation measurements below do not establish the maximum duration of every main-thread task, and a separate synchronous face-strip SQL read remains in a file outside the allowed paths. No claim of a fully nonblocking library, first-visible-thumbnail latency, or Swift 6.2.4 verification is made.

### Current verification

Re-executed the exact required gate in this worktree with the external Cargo
target preserved. `gate-current.log` records exit 0: 52 Rust library tests,
Clippy with warnings denied, Cargo formatting, FFI generation, Swift build,
288 XCTest cases (one opt-in skip, zero failures), and five Swift Testing cases.
`measurement-current.log` records eight passing Rust integration tests and the
separately enabled generated-20k engine measurement passing. No app was launched.

Fresh measured boundaries (milliseconds):

| Boundary | Before | After |
|---|---:|---:|
| Warm bottom 100 cells, 1k median | 0.285983 | 0.092030 |
| Warm bottom 100 cells, 20k median | 0.788927 | 0.092983 |
| Generated 20k core install main call | Not measured | 0.452995 |
| Generated 20k search submission main call | Not measured | 0.009060 |
| Generated 20k query wall time | Not measured | 50.466061 |
| Generated 20k Select All main call | Not measured | 0.082016 |

The warm-cell ratio is 1.010363, below 1.2. These measurements still exclude
the complete observer graph and do not establish the maximum duration of every
main-thread task. Reinspection confirmed the synchronous face-strip call at
`AppModel.swift:1026` reaches SQL through the excluded
`Cull/AssistController.swift:241-245` and
`TesseraCore/Assist/CullController+Assist.swift:106-110`. Its state setters are
private to AssistController, so moving that operation safely requires widening
the allowed paths rather than bypassing encapsulation or dropping face data.
The full acceptance verdict remains FAIL, independently of the passing gate.

### Earlier retry verification

Moved `LibraryRequestGeneration` into the explicitly allowed `LibraryCatalog.swift`
and removed the standalone helper file named by the previous scope violation.
The complete requested gate was executed again after this change, with
`CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M2-54`, and exited 0.
`gate-retry.log` records 52 Rust library tests passing, successful Clippy/format/build
steps, 288 XCTest cases with one opt-in case skipped and no failures, and five
Swift Testing cases passing. `measurement-retry.log` records eight Rust library
integration tests passing and the explicitly enabled generated-20k engine test
passing. No app was launched.

Fresh release measurements from this retry (milliseconds):

| Boundary | Before | After |
|---|---:|---:|
| Warm bottom 100 cells, 1k library, median | 0.114083 | 0.088930 |
| Warm bottom 100 cells, 20k library, median | 0.624061 | 0.091910 |
| Generated 20k engine core install main call | Not measured | 0.452042 |
| Generated 20k search submission main call | Not measured | 0.010014 |
| Generated 20k query wall time | Not measured | 57.929039 |
| Generated 20k Select All main call | Not measured | 0.106931 |

The warm-cell 20k/1k ratio was 1.033512, below 1.2. These retain the
measurement boundaries and limitations explained below, especially that query
submission is not result publication and model calls exclude the full observer
graph. A programmatic check of all tracked changes and untracked files found no
allowlist violations, no standalone generation file, and no repository `target/`.
An additional `git diff --check` reports whitespace produced by UniFFI in its
generated Swift/header files; the required Cargo formatting gate passed.

## Implementation

- P03: AppModel's cell and focused-item suggested-best paths use `PhotoItem.groupID` and constant-time range/best arrays. Invalid group/membership pairs and singletons return false. Tests generate singleton and mixed-group 20k libraries, including inconsistent remapped items.
- P04: folder opening prepares a value-only `CullController.InitialSnapshot` on its worker. Catalog construction takes copied item maps, runs on a worker, and publishes only for the current library installation. Sidebar nodes, keywords, facets and understanding startup are deferred. Optional enrichment does not reset selection.
- Quick win 4: a facets-only FFI endpoint shares search parsing/scope/facet semantics but skips the primary matching-ID query. Installation uses it instead of collecting an unused All Photos result. Facet computations and album-count queries still run, off main.
- P05: search/facet collection and mixed-selection metadata run in detached workers. Request generations are invalidated at filter-intent time, before debounce expiry. Results check catalog identity, layout revision, source and filter. Node/keyword/understanding reads also reject obsolete generations. Metadata publication increments the inspector revision after the result arrives.
- Metadata batch: one Swift-to-Rust crossing and one catalog lock for the bounded selection sample, preserving order, duplicates, empty input and first-error behavior. Per-image SQL/XMP reads remain inside that worker batch. The UI takes at most 500 selected IDs before mapping rather than materializing all 20k first.
- In-place updates collect raw matching image identities on the catalog worker and map them after the layout changes. Tether/import updates keep selection and use incremental grid notifications, not full reloads. Same-size selection changes refresh mixed metadata too.

## Measurements

Run on September 27, 2026, on the shared Apple Silicon host. Swift release (`-O`, `-enable-testing`) and Rust release. Toolchain: Apple Swift 6.3.3. Base commit: `84fc6f23abeea5365394bb1dff0449c2ac85f33b`, branch `wp/M2-54`, with the uncommitted changes in this worktree. Only this Swift toolchain is installed; 6.2.4 compatibility was not executed.

The 20k fixtures are generated by tests and removed afterwards. No large fixture is committed. `METRICS.json` contains unrounded values and the linked FFI archive SHA-256. Host load was not isolated, so these are observed samples, not a guaranteed latency distribution.

### P03: actual warm cell configuration

The test preloads thumbnail images, creates 100 retained `ThumbnailCell`s, and measures 30 repetitions of their configuration on the main actor. The before arm calls the unchanged original linear `CullController.isSuggestedBest(Int)`; the after arm uses the new item/group-ID overload. This is a same-process A/B, not a historical app FPS comparison.

| Bottom 100 cells | Before median | After median | After maximum |
|---|---:|---:|---:|
| 1k singleton library | 0.115991 ms | 0.090003 ms | 0.108957 ms |
| 20k singleton library | 0.633955 ms | 0.092030 ms | 0.096917 ms |

After at 20k is **2.252%** above 1k, within the requested 20%. The 20k configuration A/B is **6.889x** faster. Separate lookup-only measurements are retained in `EVIDENCE.txt`; the full cell configuration comparison is the acceptance-relevant row.

### P04/P05: generated 20k engine library

The opt-in test creates 20,000 tiny JPEGs, scans them with the real engine off main, prepares the cull snapshot off main, then times AppModel operations. It searches for one filename token, clears the filter to restore all 20k items, and selects all. Fixture generation/scanning is outside these measurements.

| Operation | Observed after |
|---|---:|
| Core-state install main call | 0.868917 ms |
| Search submission main call | 0.013947 ms |
| Search wall time, including worker and actor return | 60.811043 ms |
| Select All main call | 0.086069 ms |

These calls are below 8 ms, but the test does **not** attach the complete window/observer graph or measure every main-actor continuation. In particular, search submission time is not search-result installation time. A separate synthetic 20k model test observed 0.148058 ms install and 0.008941 ms Select All. No before timings were obtained for these complete operations; they are not fabricated from the audit's complexity estimates.

Structural before/after: mixed metadata goes from up to 500 main-thread FFI calls/lock acquisitions to one worker batch. The unused primary All Photos matching-ID query goes from one during installation to zero, with optional facet work retained on workers.

## Verification

Required command executed successfully, exit 0:

```sh
export CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M2-54
export MACOSX_DEPLOYMENT_TARGET=15.0
cargo test -p tessera-ffi --release --lib && \
  cargo clippy -p tessera-ffi --all-targets -- -D warnings && \
  cargo fmt --check && \
  (cd apps/mac && ./build-ffi.sh && swift build && \
   swift test -c release -Xswiftc -enable-testing)
```

- Rust library: 52 passed.
- Rust integration (`cargo test -p tessera-ffi --release --test library`): 8 passed, including batch metadata and facets-only parity.
- Swift XCTest: 288 tests, one opt-in test skipped, zero failures.
- Swift Testing: 5 tests passed.
- The skipped 20k engine measurement was then explicitly enabled and passed:

```sh
cd apps/mac
TESSERA_RUN_20K_LIBRARY=1 swift test -c release -Xswiftc -enable-testing \
  --skip-build --filter LibraryTests.testTwentyThousandEngineLibraryMeasurement
```

Rapid-query/library-replacement tests, singleton/mixed/remap fixtures, batch metadata mixed-field checks, and existing incremental/tethered-import tests passed. An intermediate run caught unwanted full reloads on catalog updates; that regression was fixed before the successful gate. A minimal existing export-test constructor mismatch was fixed by supplying `workflowErrors: []`. The existing People layout test now orders its window below other windows rather than making it key/front. No Tessera app executable was launched or activated.

## Remaining acceptance work / scope boundary

1. `AppModel.refreshFocusSummary` still calls `AssistController.refreshFaces`; `apps/mac/Sources/Tessera/Cull/AssistController.swift:241-245` synchronously calls `cull.faceStrip`, an index read. That file and the assist extension are outside this package's allowed edits. Moving the catalog metadata reads does not eliminate this other main-thread SQL path.
2. A nonactivating full-app trace is still needed to establish **no main-thread task over 8 ms**, including result installation, AppKit observers and selection synchronization. The scalar model timings above are not a substitute.
3. First-visible-thumbnail presentation was not measured. The cell test waits for warm thumbnail-cache readiness before timing configuration; that is deliberately not labeled display latency. Integrate the M2-53/P01 nonactivating harness for the presentation measurement.
4. Swift 6.2.4 was not installed. New code introduces no CGFloat/Double arithmetic, but compilation on that toolchain remains unverified.

`EVIDENCE.txt` and `METRICS.json` retain compact actual outputs. No source edits were made outside the requested allowlist, no Cargo target directory was created in the repository, and no commits were made.

RESULT: FAIL full main-thread/presentation acceptance remains unverified, with an out-of-scope synchronous face-strip read still present.
