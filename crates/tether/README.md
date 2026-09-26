# Tethered capture

`TetherBackend` isolates discovery, session/download polling, capture and stop from
ImageCaptureCore. A future dynamically linked libgphoto2 backend can implement the
same trait. Live view deliberately returns `Error::Unsupported`.

## macOS ownership

ImageCaptureCore is accessed through the ARC Objective-C bridge, compiled by `cc`
and linked with Foundation and ImageCaptureCore on macOS only. Construct, call,
and drop `NativeBackend` on the process main thread. Its Rust type is !Send/!Sync.
Discovery pumps the main run loop for a bounded interval. Starting waits for the
session and initial content catalog. Multiple connected cameras are rejected
rather than choosing an arbitrary device. Only items marked by ImageCaptureCore
as added after catalog completion are downloaded; existing card inventory is not
imported. Camera originals are never deleted. Capture checks the remote shutter
capability, and command acceptance is not presented as download completion.

Poll regularly, including for physical-shutter captures. Stop disables new
transfers, waits up to 30 seconds for accepted downloads, then closes the session.
Timeouts and disconnects are errors, not fabricated empty captures. Outstanding
SDK callbacks retain their native delegate even after Rust drops the backend;
no delegate retains a Rust pointer. A driver that never replies may retain that
inert delegate until process exit rather than cause a use-after-free.

## Ingest and naming

`Session::start(backend, folder, naming, db, model_dir)` downloads into a private
staging directory. The serial `Priority::Score` worker copies each completed file
to a temporary file in the session folder, syncs it, and publishes without
clobbering an existing original. It then incrementally scans the folder, extracts
the embedded JPEG (or uses the camera JPEG), creates a bounded JPEG preview,
persists classical quality scores and, when configured, YuNet/SFace results.
Only then does it publish a `Frame` through `Session::events()` in download-arrival
order. This is completion order, not EXIF time order. Decisions are never changed.
Failed frames have an explicit error and do not block later frames. Partial
results retain the saved file and image ID when available.

Naming tokens are `{sequence}` (four digits, growing beyond 9999), `{original}`
(original stem), and `{ext}` (original extension). Example:
`portrait_{sequence}_{original}.{ext}`. Paths, hidden filenames, unknown tokens,
and extension changes are rejected. Existing names receive `-1`, `-2`, etc.
before the extension. Sequence numbers describe this session's arrival order.
JPEG previews are stored beside the database in `tether-previews/*.preview`, not
as image files inside the scanned session namespace.

The optional model directory uses the existing app layout: `models.toml` and
`models/`, resolved by `ml-runtime`. `ml-faces::FaceModels::analyze_and_store`
computes detection, embeddings, per-face focus and the existing geometric
open-eyes heuristic. Missing weights or inference errors set `face_warning`;
they are never recorded as a successful zero-face result. Quality scoring and
manual culling remain usable without weights. No weights are bundled here.

## Test camera

`fake::FolderDropBackend` stands in for a camera without hardware: it "shoots"
the image files of a source folder in file-name order, one per `capture` (on the
next `poll`) and, with a non-zero interval, on a timer like a physical shutter.
Frames are copied (never moved) into staging under a hidden name, then renamed,
so the ingest never sees a partial file. It reports one device with an 80 %
battery and the frames left in the folder as `shots_remaining`. `Device` carries
`battery_percent` (ImageCaptureCore's `batteryLevel` when available) and
`shots_remaining` (unknown for ImageCaptureCore before a session). `Session`
works with `Box<dyn TetherBackend>`, so hosts choose a backend at run time.

## FFI and CLI

Engine exports `tether_devices`, `tether_start(session_folder, naming)`,
`tether_capture`, `tether_set_listener`, `tether_poll`, `tether_stop`,
`tether_live_view`, `tether_active` and the hidden test aid
`tether_use_fake(source_folder, interval_ms)` (the Mac app's `--fake-tether`), which
makes later sessions on that thread use the test camera. Published `TetherFrame`s
also carry the stored scores: whole-frame `sharpness`, and when faces were
analysed `faces`, `face_focus` (the largest face) and `eyes_open` (the lowest
proxy); without face models these stay `None` next to `face_warning`. During a
session `tether_devices` asks the session's own backend. There is one native session on the main thread, associated
with its catalog. Start and stop explicitly. Call `tether_poll` from a main-thread
timer (for example every 100 ms). It returns frames and calls the optional
`TetherEventListener.on_frame` outside all session borrows, permitting reentry.
The scoring work runs off the main thread. Do not dispatch these native methods
to the generic FFI worker used for other commands.

- `tessera tether list`: bounded device discovery, JSON output, no catalog writes.
- `tessera tether start SESSION_FOLDER --naming '{sequence}_{original}.{ext}'`:
  receive captures until Ctrl-C; frame events are JSON lines.
- `tessera tether capture SESSION_FOLDER --timeout 60`: open its own session,
  trigger a shutter request, wait for a frame and stop. This is not an IPC command
  to a different running `start` process.

The no-camera smoke test uses a harness-free executable so it runs on the actual
process main thread. Set `TESSERA_EXPECT_NO_CAMERA=1` to also assert the empty
list. Physical camera capture/download requires attached hardware. Normal Cargo
tests compile and link the Objective-C bridge on macOS. Additional delegate
lifetime/policy checks can be run with:

```
clang -fobjc-arc -fblocks -Wall -Wextra -Werror -Wno-unused-parameter \
  -DTETHER_BRIDGE_TEST crates/tether/native/bridge.m \
  -framework Foundation -framework ImageCaptureCore \
  -o "$CARGO_TARGET_DIR/tether-bridge-test"
TETHER_LIVE_SMOKE=1 "$CARGO_TARGET_DIR/tether-bridge-test"
```
