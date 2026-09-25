# Tessera

Tessera is a photo editing platform built on image tiles, with a shared Rust engine for raw development and layered editing.

## Build

Install stable Rust with rustfmt and clippy, then run:

```sh
cargo build --workspace
```
See [docs/](docs/) for product and architecture documentation.

Licensed under Apache-2.0.

## Releases

The macOS app uses Sparkle 2 for daily update checks and a **Check for Updates…**
menu command. Tagged `vMAJOR.MINOR.PATCH` builds run the GitHub release pipeline,
producing `Tessera-<version>.dmg`, a signed `appcast.xml`, and eligible delta updates.
See [release setup and verification](apps/mac/Support/release/README.md) for the
one-time public-key setup, CI secrets, Developer ID signing, and notarization.

Local builds default to ad-hoc signing, not notarization. The initial Sparkle
public key is intentionally empty: configure it and the private CI signing secret
before publishing an update. Private keys must never be committed.
