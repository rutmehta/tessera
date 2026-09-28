# Develop open admission controls — passed

Exact tested checkpoint: `8cc6bc18068f54843d4445e2d0549909623e51aa` on `codex/develop-admission-error-tests`, product base `f627c4a071b7e4a35c2a114917cb726ab793d458`. Only two Swift test methods/helper and a plan were added; product source was unchanged. Root reviewed the checkpoint before granting one serial compiler/native lane. No GUI was run and normal preview PID 57591 was untouched.

- `focused/`: 2 XCTest methods passed, direct exit 0. The superseded-error method independently covers replacement loading and already-ready state. Current rejection is visibly unavailable, its ownership settles, navigation leaves, and a fresh real editor opens and closes.
- `adjacent/`: 34 XCTest methods passed, direct exit 0: DevelopRecoveryCoordinatorTests, DevelopRecoveryAdmissionTests, DevelopRecoveryAdmissionBehaviorTests, and DevelopRecoveryStateTests. This count includes the two focused methods; it is not 36 unique cases.
- Both filtered invocations report 0 selected Swift Testing cases. No full-suite rerun is claimed.
- Before/after JSON freezes contain all 312 tracked apps/mac Swift, C header, modulemap, Package.resolved and ignored FFI archive inputs. They match exactly within each run; HEAD/status remained the same clean checkpoint. Commands, raw logs, PIDs, elapsed seconds, and direct exits are saved. The timeout watchdog did not fire.

Coordinator verification: root independently read both raw results and direct exits, checked the identical before/after freezes, and verified all 311 tracked frozen Git blobs against exact `8cc6bc18` plus the unchanged archive (312 total inputs). This is an independent saved-evidence check, not another runtime invocation.

Archive SHA-256 remained `4a45f8235a8c6382d0dad9be735d5cd336a86c20eb50c15051b1c55654444596`. Its prior accepted provenance is copied as `accepted-archive-provenance.json`; generated Swift/C/modulemap and archive were verified against all four entries before execution. No Rust build, archive replacement, or binding regeneration occurred. This is an accepted compatible baseline archive, not a newly built Stage C lease archive.

Both commands use `--jobs 2` and the explicit BetterSSD scratch/cache location recorded in their command files. `environment.json` records the external Swift/Clang module caches. The wrapper is saved as `run-controls.py`; its absolute paths record the actual local run and are not a portable replay guarantee. This evidence directory preserves relative payload paths and SHA256SUMS for portable verification.

Raw warnings are retained: existing Swift concurrency/capture/mutability diagnostics and the archive object built for macOS 26.5 while linking for deployment target 15.0. These results do not establish macOS 15 compatibility.

These are passing controls for existing error-handling behavior, not manufactured RED. They demonstrate the UI path for an injected opener rejection and stale completion; successful opens use real generated tiny JPEG sessions. They do not prove native exclusive admission, actual conflict wording, failed-open lease release, native close-drain/release, all-writer durability, global Quit, or B Document/Save As behavior. Native Stage C still requires its own tests. The reviewer released the sole compiler/native lane immediately after both commands completed.
