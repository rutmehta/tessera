# Release build provenance (P02)

From the repository root:

```sh
export CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M2-53
bash apps/mac/Support/make-app.sh  # release is now the packaging default
python3 apps/mac/Support/provenance.py verify --app apps/mac/build/Tessera.app
codesign --verify --deep --strict apps/mac/build/Tessera.app
```

Run both verification commands before timing. Neither launches the app. A failed
provenance check exits nonzero; rebuild rather than editing/resealing a receipt.
Ordinary `swift build` debug tooling is unchanged. `make-app.sh debug` remains
available, but the release verifier rejects it. `verify --allow-debug` is only for
checking debug package integrity, not for qualifying a benchmark.

## What gets built and checked

1. Snapshot HEAD and SHA-256 of tracked / nonignored untracked build inputs
   (including dirty contents and missing tracked files): crates, Cargo config/manifests,
   Swift sources/package manifests and packaging support. The three generated bindings
   are excluded here and hashed separately. Measurement logs and reports are not compiler
   inputs and do not invalidate the receipt while a timing run writes them.
2. Invoke the existing `build-ffi.sh` unconditionally. It runs Cargo's locked
   release build and UniFFI generation from that build, then copies the archive
   and bindings. It retains Cargo's content-based incremental cache; no debug
   archive is substituted. The caller's `CARGO_TARGET_DIR` is preserved.
3. Check source stability and checkpoint the copied archive against the Cargo
   release archive, and each copied binding against its generated counterpart.
4. Build Swift in a new `build/provenance-swift.XXXXXX` scratch tree, forcing
   compilation/relinking against that checkpoint rather than an old Swift binary.
   Check the actual SwiftPM `description.json` compiler arguments: all Swift
   commands must be optimized, no command may contain `-Onone`, and Tessera,
   TesseraCore and TesseraFFI must all be present. `-g` is not a failure when
   optimization is enabled. Check source and checkpoint hashes again.
5. After copying the executable and applying `install_name_tool`, put metadata in
   `Contents/Resources/build-provenance.json`: exact commit, source inventory and
   aggregate digest, Swift/Rust configurations, archive/binding digests, actual
   Swift build description and digest, original linked executable digest, and
   packaged pre-sign executable digest.
6. Sign inside-out and verify the signature. Only then write the external
   `Tessera.app.provenance.json` receipt containing the **final signed executable
   digest** and **bundled metadata digest**. Finally read everything back through
   the same verifier.

## Signing and verification boundaries

Both `install_name_tool` and `codesign` change Mach-O bytes. A pre-sign full-file
hash cannot be compared with the signed executable. The bundled hash describes
its pre-sign stage; the sidecar describes its final signed stage and binds that
stage to the exact bundled metadata. Nothing inside the app is modified after
signing, so there is no circular bundle-signing dependency. Re-signing invalidates
the sidecar and requires a new build/receipt through this pipeline.

Keep the sidecar beside the app (renamed correspondingly if the app is renamed),
and retain its recorded Swift scratch directory and Cargo release outputs.
Verification compares the current source tree, copied and generated bindings,
Cargo and copied archives, original linked binary/build description, bundled
metadata, and final signed executable against their recorded hashes. Missing
artifacts, source/commit changes, overwritten build outputs and stale binaries
fail closed. This verifier is for the **original build workspace**; it is not an
installed-app verifier that works without the original source/build artifacts.
Old scratch directories can be deleted once their corresponding receipts are no
longer needed; deleting the active one deliberately makes verification fail.

Receipts are build-pipeline evidence against accidental artifact mixing, not a
signed supply-chain attestation against a malicious party that can rewrite the
receipt, build outputs and source. They depend on successful trusted Cargo/Swift
invocations (and their normal cache validity), not decompilation of the linked
binary to infer its source or compiler flags. Do not run competing builds or edit
sources during packaging. Signature validity is checked separately by `codesign`.
The test suite uses fixture bytes and does not claim to exercise real signing.

## Tests (no full builds or app launch)

```sh
python3 -m unittest tools.bench.test_provenance -v
bash -n apps/mac/Support/make-app.sh
```

Negative cases include `-Onone` (even alongside `-O`), missing optimization or
module evidence, explicit debug packages, archive/binding mismatches, source
changes/new files, artifacts replaced during linking, stale linked/signed
executables, changed compiler descriptions, metadata and receipt damage.
