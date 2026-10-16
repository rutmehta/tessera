//! Memory tracing (ST-3). With `TESSERA_MEM_TRACE=1` the engine prints, after
//! a folder scan and at every develop session open and close, the process
//! footprint and the size of each cache it holds: the shared tile memo and
//! the backend's device memo, each open session's decoded source, memos and
//! mask rasters, SQLite's heap and page-cache budget, the preview request
//! tables and which ML models are loaded. `memory_report` returns the same
//! table for a host log. The sample-shoot test at the end prints the table
//! across three open/close cycles so the biggest holders are named.
use crate::{Engine, develop::Shared};
use std::sync::{Arc, OnceLock};

fn tracing() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("TESSERA_MEM_TRACE").is_some_and(|v| v == "1"))
}

/// The process's physical footprint in bytes (`phys_footprint` of
/// `TASK_VM_INFO`, the number Activity Monitor and `footprint(1)` show:
/// anonymous, compressed and IOSurface/Metal memory, not file-backed pages).
#[cfg(target_os = "macos")]
pub fn phys_footprint() -> Option<u64> {
    // `task_vm_info` up to `phys_footprint` (TASK_VM_INFO_REV0): the kernel
    // fills as many words as asked for and rejects fewer than this.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct TaskVmInfo {
        virtual_size: u64,
        region_count: i32,
        page_size: i32,
        resident_size: u64,
        resident_size_peak: u64,
        device: u64,
        device_peak: u64,
        internal: u64,
        internal_peak: u64,
        external: u64,
        external_peak: u64,
        reusable: u64,
        reusable_peak: u64,
        purgeable_volatile_pmap: u64,
        purgeable_volatile_resident: u64,
        purgeable_volatile_virtual: u64,
        compressed: u64,
        compressed_peak: u64,
        compressed_lifetime: u64,
        phys_footprint: u64,
    }
    unsafe extern "C" {
        static mach_task_self_: u32;
        fn task_info(target: u32, flavor: u32, info: *mut i32, count: *mut u32) -> i32;
    }
    const TASK_VM_INFO: u32 = 22;
    let words = (std::mem::size_of::<TaskVmInfo>() / 4) as u32;
    let mut info = std::mem::MaybeUninit::<TaskVmInfo>::zeroed();
    let mut count = words;
    // SAFETY: `info` has room for `count` words and the kernel writes at most that many.
    let kr = unsafe {
        task_info(
            mach_task_self_,
            TASK_VM_INFO,
            info.as_mut_ptr().cast(),
            &mut count,
        )
    };
    (kr == 0 && count >= words).then(|| unsafe { info.assume_init() }.phys_footprint)
}

#[cfg(not(target_os = "macos"))]
pub fn phys_footprint() -> Option<u64> {
    None
}

/// Bytes of physical RAM (`hw.memsize`), or `None` where it cannot be read.
#[cfg(target_os = "macos")]
pub fn physical_memory() -> Option<u64> {
    unsafe extern "C" {
        fn sysctlbyname(
            name: *const std::ffi::c_char,
            oldp: *mut std::ffi::c_void,
            oldlenp: *mut usize,
            newp: *mut std::ffi::c_void,
            newlen: usize,
        ) -> i32;
    }
    let mut size: u64 = 0;
    let mut len = std::mem::size_of::<u64>();
    // SAFETY: `hw.memsize` is a u64 and `len` tells the kernel the buffer size.
    let rc = unsafe {
        sysctlbyname(
            c"hw.memsize".as_ptr(),
            (&mut size as *mut u64).cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    (rc == 0 && size > 0).then_some(size)
}

#[cfg(not(target_os = "macos"))]
pub fn physical_memory() -> Option<u64> {
    None
}

/// Share of physical RAM the shared memo caches may hold between them.
const CACHE_RAM_FRACTION: f64 = 0.15;
/// Budget when RAM cannot be read (the pre-ST-3 fixed 512 MB per memo).
const CACHE_FALLBACK: u64 = 1 << 30;

/// The develop memo budget in bytes, shared by the CPU tile memo and the
/// GPU resident memo (each gets half): `TESSERA_CACHE_BUDGET_MB` when set,
/// else [`CACHE_RAM_FRACTION`] of physical RAM. Images with no open session
/// leave the memos on close (`Shared::release_resources`), so the budget
/// bounds the open sessions' working set, not everything ever rendered.
pub(crate) fn cache_budget() -> u64 {
    static BUDGET: OnceLock<u64> = OnceLock::new();
    *BUDGET.get_or_init(|| {
        let configured = std::env::var("TESSERA_CACHE_BUDGET_MB")
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .filter(|mb| *mb > 0)
            .map(|mb| mb << 20);
        let budget = configured.unwrap_or_else(|| {
            physical_memory().map_or(CACHE_FALLBACK, |ram| {
                (ram as f64 * CACHE_RAM_FRACTION) as u64
            })
        });
        // Below this a single 61 MP render thrashes; the memo is still a cache.
        budget.max(256 << 20)
    })
}

fn mb(bytes: u64) -> String {
    format!("{:.0} MB", bytes as f64 / 1_048_576.)
}

impl Engine {
    pub(crate) fn register_session(&self, shared: &Arc<Shared>) {
        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        sessions.retain(|s| s.strong_count() > 0);
        sessions.push(Arc::downgrade(shared));
    }

    /// Every develop session still alive (the host dropped the others).
    pub(crate) fn open_sessions(&self) -> Vec<Arc<Shared>> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        sessions.retain(|s| s.strong_count() > 0);
        sessions
            .iter()
            .filter_map(std::sync::Weak::upgrade)
            .collect()
    }

    /// Prints the memory table on stderr when `TESSERA_MEM_TRACE=1`.
    pub(crate) fn trace_memory(&self, label: &str) {
        if tracing() {
            eprintln!("{}", self.memory_report_labelled(label));
        }
    }

    /// The table as one string: a footprint line, then one line per cache
    /// (`name: bytes`), sessions indented under their image.
    pub(crate) fn memory_report_labelled(&self, label: &str) -> String {
        let mut out = format!(
            "memory [{label}]: footprint {}",
            phys_footprint().map_or_else(|| "?".into(), mb)
        );
        for (name, bytes) in self.memory_lines() {
            out.push_str(&format!("\n  {name}: {}", mb(bytes)));
        }
        out
    }

    /// Named sizes of every cache the engine holds, in bytes. Shared caches
    /// first, then each open session's own, then SQLite, previews and models
    /// (reported as loaded or not: their weights are not accounted).
    pub(crate) fn memory_lines(&self) -> Vec<(String, u64)> {
        let mut lines: Vec<(String, u64)> = Vec::new();
        if let Some(ram) = physical_memory() {
            lines.push(("physical RAM".into(), ram));
        }
        match self.renderer.get() {
            Some(backend) => {
                for (name, bytes) in backend.memory_report() {
                    lines.push((name, bytes as u64));
                }
            }
            None => lines.push(("shared renderer (not created)".into(), 0)),
        }
        for session in self.open_sessions() {
            let name = session.trace_name();
            for (what, bytes) in session.memory_report() {
                // The shared memo and the backend are reported once above.
                if what.contains("(shared)") || what.starts_with("gpu ") {
                    continue;
                }
                lines.push((format!("session {name}: {what}"), bytes as u64));
            }
        }
        // SQLite's heap is process-wide (three connections); the page-cache
        // budget is per connection (`PRAGMA cache_size`, negative = KiB).
        // SAFETY: a plain query of the allocator's counter.
        let sqlite_heap = unsafe { rusqlite::ffi::sqlite3_memory_used() };
        lines.push(("sqlite heap".into(), sqlite_heap.max(0) as u64));
        if let Ok(heads) = self.heads.lock() {
            let cache: i64 = heads
                .query_row("PRAGMA cache_size", [], |r| r.get(0))
                .unwrap_or(0);
            let bytes = if cache < 0 {
                cache.unsigned_abs() * 1024
            } else {
                cache as u64 * 4096
            };
            lines.push(("sqlite page cache budget (per connection)".into(), bytes));
        }
        let previews = self.preview_states.lock().map_or(0, |s| s.len() as u64);
        lines.push(("preview request states (count)".into(), previews));
        lines.push((
            "segmenter loaded".into(),
            u64::from(self.segmenter.is_loaded()),
        ));
        let faces = self.faces.lock().is_ok_and(|f| f.is_some());
        lines.push(("face models loaded".into(), u64::from(faces)));
        lines
    }
}

#[uniffi::export]
impl Engine {
    /// The engine's memory table (see `mem_trace`): the process footprint and
    /// every cache it holds with its resident bytes, one per line.
    pub fn memory_report(&self) -> String {
        self.memory_report_labelled("report")
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    //! `TESSERA_SAMPLE_SHOOT=<dir of RAWs> cargo test -p tessera-ffi
    //! sample_shoot_memory_table -- --nocapture` scans a copy of the folder,
    //! opens, renders and closes the first three photos twice, and prints the
    //! footprint and cache table at every step (the ST-3 evidence).
    use super::*;
    use crate::{ImageQuery, surface::Surface};
    use std::sync::mpsc;

    struct FinalFrame(mpsc::Sender<bool>);
    impl crate::develop::DevelopListener for FinalFrame {
        fn frame_ready(&self, frame: crate::develop::FrameInfo) {
            if frame.is_final {
                let _ = self.0.send(true);
            }
        }
        fn render_failed(&self, _: String) {
            let _ = self.0.send(false);
        }
        fn saved(&self, _: String) {}
    }

    fn step(engine: &Engine, label: &str, lines: &mut Vec<String>) -> u64 {
        // Let the save worker and GPU retire what a close released.
        std::thread::sleep(std::time::Duration::from_millis(300));
        let report = engine.memory_report_labelled(label);
        eprintln!("{report}");
        lines.push(report);
        if std::env::var_os("TESSERA_MEM_HEAP").is_some() && label.ends_with("all closed") {
            // Where the unattributed bytes live: the process's own regions
            // and the allocator's largest classes (developer tools).
            let pid = std::process::id().to_string();
            for (tool, args) in [
                ("vmmap", vec!["-summary", pid.as_str()]),
                ("heap", vec!["-s", pid.as_str()]),
            ] {
                if let Ok(out) = std::process::Command::new(tool).args(args).output() {
                    let text = String::from_utf8_lossy(&out.stdout);
                    let shown: Vec<_> = text.lines().take(60).collect();
                    eprintln!("{tool}:\n{}", shown.join("\n"));
                    lines.push(format!("{tool}:\n{}", shown.join("\n")));
                }
            }
        }
        phys_footprint().unwrap_or(0)
    }

    #[test]
    fn sample_shoot_memory_table() {
        let Some(shoot) = std::env::var_os("TESSERA_SAMPLE_SHOOT") else {
            eprintln!("TESSERA_SAMPLE_SHOOT not set: skipping the sample-shoot memory table");
            return;
        };
        let scratch = tempfile::tempdir().unwrap();
        let folder = scratch.path().join("shoot");
        std::fs::create_dir_all(&folder).unwrap();
        let folder = folder.canonicalize().unwrap();
        let mut names = Vec::new();
        for entry in std::fs::read_dir(&shoot).unwrap() {
            let path = entry.unwrap().path();
            let raw = path.extension().is_some_and(|e| {
                ["arw", "dng", "nef", "cr3"].contains(&e.to_string_lossy().to_lowercase().as_str())
            });
            if raw {
                let name = path.file_name().unwrap().to_owned();
                std::fs::copy(&path, folder.join(&name)).unwrap();
                names.push(name);
            }
        }
        assert!(
            names.len() >= 3,
            "need at least three RAWs in {}",
            shoot.to_string_lossy()
        );
        let mut lines = Vec::new();
        let engine = Engine::open(scratch.path().join("support").to_string_lossy().into()).unwrap();
        let start = step(&engine, "start", &mut lines);
        engine
            .index_folder(folder.to_string_lossy().into_owned())
            .unwrap();
        let after_scan = step(&engine, "after scan", &mut lines);
        let mut images = engine
            .list_images(ImageQuery {
                folder: Some(folder.to_string_lossy().into_owned()),
                text: None,
                decision: None,
                limit: 0,
                offset: 0,
            })
            .unwrap();
        images.sort_by(|a, b| a.path.cmp(&b.path));
        let ids: Vec<_> = images.iter().take(3).map(|i| i.id.clone()).collect();
        assert_eq!(ids.len(), 3, "the scan indexed the copied RAWs");
        let mut peak = after_scan;
        let mut closed_after_round = Vec::new();
        for round in 0..2 {
            for (n, id) in ids.iter().enumerate() {
                let session = engine.clone().open_develop_session(id.clone()).unwrap();
                let plan = session.plan_surface(1440, 900);
                let surface = Surface::create_rgba8(plan.width, plan.height).unwrap();
                let (send, receive) = mpsc::channel();
                session.set_listener(Some(Arc::new(FinalFrame(send))));
                session
                    .attach_surface(surface.id(), plan.width, plan.height)
                    .unwrap();
                assert!(
                    receive
                        .recv_timeout(std::time::Duration::from_secs(120))
                        .unwrap(),
                    "render failed"
                );
                peak = peak.max(step(
                    &engine,
                    &format!("round {} photo {} open (rendered)", round + 1, n + 1),
                    &mut lines,
                ));
                session.close().unwrap();
                session.set_listener(None);
                drop(session);
                drop(surface);
                step(
                    &engine,
                    &format!("round {} photo {} closed", round + 1, n + 1),
                    &mut lines,
                );
            }
            closed_after_round.push(step(
                &engine,
                &format!("round {} all closed", round + 1),
                &mut lines,
            ));
        }
        let summary = format!(
            "ST-3 memory summary: start {}, after scan {}, peak with one session open {}, \
             all closed after round 1 {}, after round 2 {} (growth {:+.0} MB)",
            mb(start),
            mb(after_scan),
            mb(peak),
            mb(closed_after_round[0]),
            mb(closed_after_round[1]),
            (closed_after_round[1] as f64 - closed_after_round[0] as f64) / 1_048_576.
        );
        eprintln!("{summary}");
        if let Some(out) = std::env::var_os("TESSERA_MEM_REPORT") {
            lines.push(summary);
            std::fs::write(out, lines.join("\n\n")).unwrap();
        }
    }
}
