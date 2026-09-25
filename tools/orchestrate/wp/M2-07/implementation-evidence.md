# M2-07 implementation evidence

## Implemented

- Sparkle 2.10.0 (latest stable 2.x observed from upstream tags) pinned exactly,
  Package.resolved included. Persistent SPUStandardUpdaterController, app-menu
  Check for Updates…, daily defaults, public key and GitHub feed in Info.plist.
- Complete versioned Sparkle framework embedded with both XPC services and
  updater helpers; inside-out signing, hardened runtime, no app sandbox.
- Optional Developer ID signing and notarization, hdiutil DMG packaging,
  Keychain/stdin-secret appcast generation and automatic delta support.
- Push/PR CI and tag-triggered release workflow, cached RAW fixtures, temporary
  Apple signing keychain cleanup, previous DMGs restored for deltas.
- Release/key provisioning documentation. Public key deliberately empty with
  build warning. No Sparkle private key generated or exported.

## Passed locally

The exact requested command was executed successfully twice, including after the
final implementation changes:

```sh
(cd apps/mac && ./build-ffi.sh && swift build && swift test && bash Support/make-app.sh release && codesign --verify --deep --strict build/Tessera.app)
```

Final run: 14 XCTest tests and 5 Swift Testing tests passed, release bundle created,
strict deep codesign verification exited 0. CARGO_TARGET_DIR remained
`/Users/rutmehta/.cache/tessera-target/M2-07` for every Cargo invocation.
Full latest acceptance log: `apps/mac/build/build-acceptance.log` (ignored).

Additional checks:

- `bash Support/release/test-release.sh`: passed ad-hoc signature, embedded
  framework/symlinks/XPCs, hardened runtime, plist defaults, and missing-key
  rejection of an unsigned dummy ZIP using a unique nonexistent Keychain account.
- `bash Support/release/notarize.sh`: explicitly skipped with NOTARY_PROFILE unset.
- `bash Support/release/make-dmg.sh`: created and verified
  `apps/mac/dist/Tessera-0.0.0-dev.3700382.dmg`.
- Mounted DMG read-only inside `apps/mac/build/dmg-verify`, verified its app with
  codesign --verify --deep --strict, checked Applications symlink, then detached.
- CI missing-credential branch explicitly produced CODESIGN_IDENTITY=-.
- `actionlint` v1.7.12 (without optional shellcheck): both workflows passed.
- `bash -n`: all new/modified shell scripts passed. `git diff --check`: passed.

## Broader checks and limits

`bash ci.sh` passed fmt/clippy and progressed through workspace tests but first
stopped because an image-core test executable was missing (OS error 2). Retrying
`cargo test -p image-core --test m2` passed all four tests. A full ci.sh retry
progressed further but hit the tool's 420-second timeout at ml-runtime runtime
tests. The complete workspace suite is therefore NOT reported as passing.
Logs: `apps/mac/build/build-ci.log`, `build-m2-retry.log`, `build-ci-retry.log`.

A separate `cargo deny check licenses bans` reported `bans ok, licenses FAILED`:
examples include jpeg-encoder's IJG license not being allowed and tessera-ffi
missing a license declaration. These unchanged Rust manifests/license policy are
outside M2-07's allowed paths. They remain a blocker to green repository-wide CI.
Log: `apps/mac/build/build-deny.log`.

No Developer ID credentials, notarization submission, private-key signing,
end-to-end installed update, or GitHub release publication was exercised.
These need operator provisioning; CI fails closed before publishing when the
Sparkle public/private key configuration is missing. Ad-hoc verification is not
Gatekeeper/notarization verification.

All source changes are confined to the user-authorized paths. No commit or push
was performed.
