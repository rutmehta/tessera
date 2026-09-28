# Independent review: camera-linear CPU Smart Preview component

Reviewed 2026-09-28, final commit `a3ab5034fbda4b486bd7bec493943bd104dfd772`, base `7f98ab79`. Reviewed all four changed pipeline-cpu files, the task-1 plan, shipping specification, and source audit. Also inspected the existing lens, embedded-opcode, image-reduction and settings-validation dependencies. No source edits or builds were performed by this reviewer.

## Decision

Accept this bounded in-memory CPU component. No actionable correctness finding was identified in the reviewed diff. This is not acceptance of persistent Smart Previews, image-core routing, offline editing, reconnect, export, or GUI behavior.

## Source findings

- `smart_preview.rs:53–74` admits only Native revision 2, raw denoise Off and the existing CPU settings validator. Applied stage-2 embedded operations explicitly return Unsupported/original-required; original metadata remains separate from reduced addressing. This does not silently downgrade Auto to None.
- `render.rs:228–265` uses the original camera matrix and as-shot multipliers, in the same profile-then-WB order as CFA rendering. Working-space RGB has its own branch and is not reused. The calibration validator remains unchanged; this does not add support for formerly unsupported camera-profile recipe controls.
- The extracted original-sensor prefix retains reconstruction, raw-denoise, demosaic, original-camera lens resolution, database/Bayer CA re-demosaic or RGB CA, stage 1, and manual CA order. Post-demosaic denoise remains in its prior full-RAW location after this prefix. The only subsequent amendment moves the existing test module below the extracted function.
- Captured correction is owned and private. Proxy rendering does not repeat CA or stage 0/1, and reuses the late vignette and common geometry tail. Attempts to provide replacement resolved/profile/database/capture/manual-CA dependencies are refused. Whole decode/linearize/demosaic/denoise/lens equality conservatively invalidates the baked prefix without rewriting settings.
- Scale is chosen from the active crop, and existing area averaging includes partial edge bins. The final Image constructor checks finite samples while retaining signed and HDR values. Original crop and EXIF orientation are retained; CPU planes remain sensor-oriented, consistent with the existing CFA API.
- Tests discriminate nonidentity calibration, custom/as-shot WB, editable exposure/geometry, default nonidentity Auto lens/CA, stage 0/1 capture, late-stage refusal, upstream mismatch, negative/HDR values and independently assembled odd-crop area bins.

## Evidence checked

Evidence directory: `/Volumes/betterSSD/tessera-validation/smart-previews/engine`.

- `frozen2-commit.txt` identifies final `a3ab5034`.
- `frozen2-full.log` and `.exit`: complete Release pipeline-cpu suite, 147 passed, zero failed, one existing ignored test; exit 0.
- `frozen2-clippy.log` and `.exit`: Release all-targets clippy with `-D warnings`; exit 0.
- `frozen2-fmt.exit`: scoped rustfmt check; exit 0.
- `frozen2-hash-compare.exit`: before/after source manifest comparison; exit 0.
- Earlier compile-red, first HDR-fixture failure, and first frozen clippy placement failure remain recorded in the evidence. They are not counted as successful validation.

## Limits for later integration

The component deliberately has no persistent representation or deserializer, payload digest, image-core cache identity, RAW recipe-owner integration, or original-only export enforcement. Those remain later plan tasks. The supplied original digest is caller-verified; coherent capture and binding must be enforced by the integration layer.

The current tests retain orientation metadata and exercise odd reduction and geometry separately; they do not establish image-core EXIF application, reduced-resolution mask placement, or real-camera/offline behavior. Qualification of those integration paths still needs their planned tests. Pixel-space detail and reduced nonlinear rendering remain approximations, as explicitly allowed by the specification.
