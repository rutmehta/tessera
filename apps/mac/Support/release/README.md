# Tessera release operations

Sparkle 2.10.0 is pinned exactly in Package.swift and Package.resolved. Sparkle is
MIT-licensed, permitted by docs/13-licensing.md. Its bundled license is preserved
inside the embedded framework. See [Sparkle setup](https://sparkle-project.org/documentation/)
and [publishing](https://sparkle-project.org/documentation/publishing/).

## One-time keys (human-operated, not part of a build)

From `apps/mac`, run `swift package resolve`. The tools are in
`.build/artifacts/sparkle/Sparkle/bin`.

1. Run `.build/artifacts/sparkle/Sparkle/bin/generate_keys` once. It stores the
   private EdDSA key in the login Keychain (default account `ed25519`). Never
   generate a new key per build: existing installations trust the original key.
2. Paste the printed **public** key into `Support/Info.plist` as `SUPublicEDKey`.
   Commit only this public key. It is deliberately empty in the initial setup.
   Bundling warns when empty. Updates are not operational until configured.
3. To provision CI, use `generate_keys -x /secure/path/sparkle-private-key` to
   export the private key outside the checkout, upload that file's contents into
   the repository Actions secret `SPARKLE_PRIVATE_KEY`, then remove the exported
   file. Keep a secure backup. Never paste the private key into a command line,
   source file, app plist, log, or release asset.

For a separate Keychain account use `generate_keys --account NAME`, and set
`SPARKLE_KEY_ACCOUNT=NAME` when generating the appcast. No script here creates,
exports, or imports a Sparkle private key. CI pipes the secret through stdin to
`generate_appcast --ed-key-file -`, without temporary private-key files.

## Local release

From `apps/mac` (preserve an existing external `CARGO_TARGET_DIR`):

```sh
./build-ffi.sh
swift build && swift test
# Optional: otherwise ad-hoc, which is NOT Gatekeeper-ready distribution.
export CODESIGN_IDENTITY='Developer ID Application: Your Name (TEAMID)'
# Optional: an existing notarytool Keychain profile, not a password.
export NOTARY_PROFILE='tessera-release'
bash Support/make-app.sh release
bash Support/release/notarize.sh
bash Support/release/make-dmg.sh
for dmg in dist/*.dmg; do bash Support/release/notarize.sh "$dmg"; done
bash Support/release/make-appcast.sh
```

`make-app.sh` embeds the entire SwiftPM Sparkle.framework with `ditto` (preserves
versioned symlinks), adds the executable's Frameworks rpath, signs Downloader.xpc,
Installer.xpc, Autoupdate, Updater.app, the framework, then Tessera.app. It retains
Sparkle helper entitlements. Both ad-hoc and Developer ID app signing use hardened
runtime. Developer ID uses `Tessera.entitlements` (no weakened library validation)
and timestamps. Ad-hoc uses `Tessera-adhoc.entitlements` to disable library
validation: independently ad-hoc-signed framework and app binaries have different
effective Team IDs, so the hardened app otherwise aborts in dyld before launch.
This exception is for local ad-hoc builds only. Signing failures are fatal.

`notarize.sh [artifact]` defaults to `build/Tessera.app`, archives an app for
`xcrun notarytool submit --wait`, then staples and validates the original app.
For a DMG it submits, staples and validates the DMG directly. Without
`NOTARY_PROFILE`, it explicitly skips. Notarize the app before building the DMG,
and notarize the DMG before generating signatures: changing signed archive bytes
later invalidates Sparkle signatures.

`make-dmg.sh` uses only `hdiutil`, stages Tessera.app and an Applications symlink,
and emits `dist/Tessera-<version>.dmg`. Version comes from `git describe --tags`:
exact `vMAJOR.MINOR.PATCH` tags become `MAJOR.MINOR.PATCH` in both bundle version
fields and the filename. Untagged builds use `0.0.0-dev.<description>` and are for
local testing, not publication. Release CI rejects non-numeric release tags.

`make-appcast.sh [dist-folder]` uses the resolved Sparkle `generate_appcast`, or
`SPARKLE_BIN_DIR` if explicitly provided. It requires a Keychain signing key or
`SPARKLE_PRIVATE_KEY` and refuses unsigned/empty feeds. Keep previous DMGs in the
same folder for delta generation (up to five per latest version). Sparkle may omit
deltas if they would not save enough space. Publish the generated `.delta` files,
all referenced DMGs, and `appcast.xml`, not `old_updates/` or extracted caches.
Set `SPARKLE_DOWNLOAD_URL_PREFIX` for a fixed tag's download directory. The default
is the latest-release asset directory. Never change the prefix without uploading
all referenced archives there.

The installed app reads the feed and public key from Info.plist. Only builds with
both non-empty `SUFeedURL` and `SUPublicEDKey` start Sparkle (daily checks by
default). Otherwise Check for Updates… is disabled with the tooltip "updates not
configured for this build" and a one-time log; no updater dialog opens at launch.
Automatic checks do not imply automatic installation. Sparkle respects users'
saved check preferences.

## GitHub Actions

`.github/workflows/ci.yml` runs Rust format/clippy/tests/license checks via `ci.sh`,
FFI generation, Swift build/tests, and the ad-hoc packaging regression test on
push and PR. RAW fixture downloads are cached by `fixtures/fetch.sh` content.
Cargo outputs stay in the runner temp directory, outside the checkout.

`.github/workflows/release.yml` runs on `v*` tags and publishes DMGs, appcast, and
any deltas. It downloads the preceding stable release's DMGs for delta generation
and republishes the archives still referenced by the new feed under the new tag.
The updater feed always points at `releases/latest/download/appcast.xml`.

Repository Actions secrets:

- `SPARKLE_PRIVATE_KEY`: exported Sparkle EdDSA key. Required for every published
  appcast, including ad-hoc builds. The matching public key must be committed.
- `APPLE_CERT_P12`: base64 Developer ID Application certificate including its key.
- `APPLE_CERT_PASSWORD`: password for that P12.
- `NOTARY_APPLE_ID`, `NOTARY_PASSWORD` (app-specific password), `NOTARY_TEAM_ID`:
  notarization credentials. CI creates the `tessera-release` Keychain profile.

If any Apple signing/notary secret is absent, CI explicitly uses ad-hoc signing
and skips notarization. If provided credentials are invalid, it fails rather
than silently downgrading. The temporary signing keychain and certificate are
cleaned up even on failure. No Sparkle key means **no release publication**,
not an unsigned appcast. An ad-hoc release is for testing and will not pass
Gatekeeper as a Developer ID notarized release would.

## Verification

```sh
./build-ffi.sh && swift build && swift test && bash Support/make-app.sh release && codesign --verify --deep --strict build/Tessera.app
bash Support/release/test-release.sh
```

The script rebuilds ad-hoc, verifies the signature, framework symlinks, Installer
and Downloader XPCs with `codesign -dvv`, runtime flag and feed configuration. It
directly executes `build/Tessera.app/Contents/MacOS/Tessera --stub 0
--develop-selftest --bundle-selftest` (the extra flag emits a launch marker and
quits) and requires a zero exit and `bundle-selftest: launched` within five seconds.
This catches missing rpaths and library-validation failures that static signature
checks miss. It creates an unsigned
dummy ZIP in a temporary `build/` dist folder and invokes the real appcast wrapper
with an intentionally nonexistent Keychain account and no environment key. It
must exit nonzero with:

```text
error: no Sparkle signing key; run generate_keys once or set SPARKLE_PRIVATE_KEY (see Support/release/README.md).
```

No key is generated and no developer Keychain item is modified. With actual keys,
also test an end-to-end update from the previous notarized release before broad
publication. Local ad-hoc packaging tests cannot prove Apple notarization,
GitHub credentials, appcast signatures, or real installed update continuity.
