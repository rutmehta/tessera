//! The process allocator (ST-3). macOS malloc keeps every freed large block
//! resident for reuse: a closed session's decode planes and tiles stayed in
//! the footprint (`MALLOC_LARGE (empty)`, ~650 MB on the 8-RAW sample) after
//! their `free`, `malloc_zone_pressure_relief` returned nothing, and only the
//! process-start switches `MallocLargeCache=0` / `MallocSpaceEfficient=1`
//! gave them back. This allocator maps blocks of [`LARGE`] bytes and more
//! with `mmap` and unmaps them on free, so pixel buffers leave the footprint
//! the moment they are dropped; everything smaller stays with the system
//! allocator. It applies to all Rust code linked into the app, the CLI and
//! the tests (one `#[global_allocator]` per binary), and keeps the live and
//! peak heap counters the memory tests measure with ([`live_bytes`],
//! [`peak_during`]).
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

/// Blocks at least this large go to the OS directly: every tile (a 256×256
/// f16 RGB tile is 384 KB), plane and decode buffer; not strings or rows.
pub const LARGE: usize = 128 << 10;
/// Largest alignment `mmap` satisfies without over-allocating (the page).
const PAGE: usize = 16 << 10;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

/// Bytes currently allocated through this allocator (requested sizes).
pub fn live_bytes() -> usize {
    LIVE.load(Relaxed)
}

/// Peak heap growth in bytes above the level at entry while `f` runs. The
/// counters are process-wide: measure one thing at a time.
pub fn peak_during<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let base = LIVE.load(Relaxed);
    PEAK.store(base, Relaxed);
    let out = f();
    (out, PEAK.load(Relaxed).saturating_sub(base))
}

fn count_alloc(bytes: usize) {
    let now = LIVE.fetch_add(bytes, Relaxed) + bytes;
    PEAK.fetch_max(now, Relaxed);
}

#[cfg(target_os = "macos")]
mod os {
    use std::ffi::c_void;
    unsafe extern "C" {
        fn mmap(
            addr: *mut c_void,
            len: usize,
            prot: i32,
            flags: i32,
            fd: i32,
            offset: i64,
        ) -> *mut c_void;
        fn munmap(addr: *mut c_void, len: usize) -> i32;
    }
    const PROT_READ_WRITE: i32 = 0x1 | 0x2;
    const MAP_PRIVATE_ANON: i32 = 0x0002 | 0x1000;

    /// Zeroed, page-aligned, private anonymous pages; null when exhausted.
    pub(super) unsafe fn map(len: usize) -> *mut u8 {
        // SAFETY: an anonymous private mapping of `len` bytes; no file.
        let p = unsafe {
            mmap(
                std::ptr::null_mut(),
                len,
                PROT_READ_WRITE,
                MAP_PRIVATE_ANON,
                -1,
                0,
            )
        };
        if p as isize == -1 {
            std::ptr::null_mut()
        } else {
            p.cast()
        }
    }
    pub(super) unsafe fn unmap(p: *mut u8, len: usize) {
        // SAFETY: `p` and `len` came from `map`.
        unsafe { munmap(p.cast::<c_void>(), len) };
    }
}

/// System malloc for small blocks, `mmap`/`munmap` for large ones, with
/// live/peak accounting of requested bytes.
pub(crate) struct LargeToOs;

impl LargeToOs {
    #[inline]
    fn large(layout: &Layout, size: usize) -> bool {
        cfg!(target_os = "macos") && size >= LARGE && layout.align() <= PAGE
    }

    unsafe fn raw_alloc(layout: Layout, zeroed: bool) -> *mut u8 {
        if Self::large(&layout, layout.size()) {
            // Anonymous pages are zero-filled.
            #[cfg(target_os = "macos")]
            // SAFETY: a fresh mapping of the requested size.
            return unsafe { os::map(layout.size()) };
        }
        // SAFETY: forwarded unchanged.
        unsafe {
            if zeroed {
                System.alloc_zeroed(layout)
            } else {
                System.alloc(layout)
            }
        }
    }

    unsafe fn raw_dealloc(ptr: *mut u8, layout: Layout) {
        if Self::large(&layout, layout.size()) {
            #[cfg(target_os = "macos")]
            // SAFETY: `ptr` was mapped by `raw_alloc` with this layout.
            unsafe {
                os::unmap(ptr, layout.size())
            };
            #[cfg(target_os = "macos")]
            return;
        }
        // SAFETY: `ptr` came from `System.alloc` with this layout.
        unsafe { System.dealloc(ptr, layout) }
    }
}

unsafe impl GlobalAlloc for LargeToOs {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller's contract.
        let p = unsafe { Self::raw_alloc(layout, false) };
        if !p.is_null() {
            count_alloc(layout.size());
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller's contract.
        let p = unsafe { Self::raw_alloc(layout, true) };
        if !p.is_null() {
            count_alloc(layout.size());
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller's contract.
        unsafe { Self::raw_dealloc(ptr, layout) };
        LIVE.fetch_sub(layout.size(), Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = if !Self::large(&layout, layout.size()) && !Self::large(&layout, new_size) {
            // SAFETY: both sizes are the system allocator's.
            unsafe { System.realloc(ptr, layout, new_size) }
        } else {
            // A block changing class (or a mapped block growing: there is no
            // `mremap`) moves: allocate, copy what fits, free the old block.
            // SAFETY: `new_size` is non-zero for a large class; the alignment is the caller's.
            let new_layout = unsafe { Layout::from_size_align_unchecked(new_size, layout.align()) };
            // SAFETY: as `alloc`.
            let new_ptr = unsafe { Self::raw_alloc(new_layout, false) };
            if !new_ptr.is_null() {
                // SAFETY: both blocks are live and at least `min` bytes long.
                unsafe {
                    std::ptr::copy_nonoverlapping(ptr, new_ptr, layout.size().min(new_size));
                    Self::raw_dealloc(ptr, layout);
                }
            }
            new_ptr
        };
        if !p.is_null() {
            if new_size >= layout.size() {
                count_alloc(new_size - layout.size());
            } else {
                LIVE.fetch_sub(layout.size() - new_size, Relaxed);
            }
        }
        p
    }
}

#[global_allocator]
static ALLOCATOR: LargeToOs = LargeToOs;

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn large_blocks_leave_the_footprint_when_dropped() {
        // Non-zero fills: a zeroed plane is untouched pages, not resident.
        // 1 GB so concurrent tests in this process cannot mask the result.
        let before = crate::mem_trace::phys_footprint().unwrap();
        let (planes, peak) = peak_during(|| {
            (1..=4)
                .map(|i| vec![i as f32; 256 << 18])
                .collect::<Vec<Vec<f32>>>()
        });
        assert!(peak >= 1 << 30, "counted peak {peak}");
        let held = crate::mem_trace::phys_footprint().unwrap();
        assert!(held >= before + (900 << 20), "{before} -> {held}");
        let live = live_bytes();
        drop(planes);
        assert!(live - live_bytes() >= 1 << 30);
        let after = crate::mem_trace::phys_footprint().unwrap();
        assert!(
            held - after >= 768 << 20,
            "freed planes stay resident: {before} -> {held} -> {after}"
        );
    }

    #[test]
    fn realloc_crosses_the_class_boundary_both_ways() {
        let mut v: Vec<u8> = Vec::with_capacity(16);
        for i in 0..(LARGE * 3) {
            v.push(i as u8);
        }
        assert!(v.iter().enumerate().all(|(i, &b)| b == i as u8));
        v.truncate(100);
        v.shrink_to_fit();
        assert!(v.iter().enumerate().all(|(i, &b)| b == i as u8));
        let zeroed = vec![0u64; LARGE];
        assert!(zeroed.iter().all(|&z| z == 0));
    }
}
