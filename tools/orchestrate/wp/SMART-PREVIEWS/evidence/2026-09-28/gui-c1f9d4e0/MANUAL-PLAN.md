# Packaged, not launched

Source: final08 c1f9d4e0 Release executable; this is not the strict07 artifact. No rebuild or GPU workload was performed. Codesign deep/strict verification passed. Input executable, Sparkle framework files and source Sony fixture hashes remained unchanged. Exact commands and input/output hashes are in commands.json and manifest.json; package-exact.py reproduces packaging and refuses any existing destination.

App/profile/photos/exports are owned by this qualification directory. Fixture is a regular copied file, not a link. No library scan has been performed. Do not replace or close existing apps, touch the M258 notification prompt, change global settings, or work around denied CUA surfaces.

## Deferred launch commands — NOT executed

Only after root releases workload and desktop-access gates:

```sh
open -n '/Volumes/betterSSD/tessera-validation/smart-previews/gui-c1f9d4e0/Tessera Smart Preview c1f9d4e0.app' --args --app-dir /Volumes/betterSSD/tessera-validation/smart-previews/gui-c1f9d4e0/profile --folder /Volumes/betterSSD/tessera-validation/smart-previews/gui-c1f9d4e0/photos
```

Later same-profile relaunch without overriding the remembered folder:

```sh
open -n '/Volumes/betterSSD/tessera-validation/smart-previews/gui-c1f9d4e0/Tessera Smart Preview c1f9d4e0.app' --args --app-dir /Volumes/betterSSD/tessera-validation/smart-previews/gui-c1f9d4e0/profile
```

## Manual acceptance

1. Launch this unique bundle with its explicit profile and copied photos. Confirm empty inherited recents and Original default. Build Smart Preview for selected RAW; check status. Verify source toggle closes/saves current editor before opening the new source.
2. Quit only this test app. Move only this directory's photos folder to photos-held; preserve that directory for restoration. Relaunch without --folder. Verify cached offline Library read-only state, Grid/Loupe/Compare thumbnails and explicit Use Smart Preview.
3. Edit exposure/WB; save and observe local saved status and thumbnail updates. Quit/relaunch with same profile and verify durability. Attempt dirty discard and expect refusal. Offline full-quality export must not succeed using the proxy.
4. Quit test app, restore photos-held to photos, relaunch, synchronize. Verify Original edit source and full-resolution export only into owned exports directory. No user photos are involved.
5. Record actual visible results; no screenshot or GUI result is implied by packaging. Compare original source fixture hash with manifest after acceptance; inspect changed owned files and preserve evidence.

Unique bundle identity isolates app registration; --app-dir isolates index/caches and Preferences.plist. It does not sandbox OS services or Keychain. Do not exercise AI credentials or unrelated settings. Existing permission dialogs remain untouched; blocked desktop access means defer manual acceptance.
