//! The process allocator (ST-3). macOS malloc keeps every freed large block
//! resident for reuse: a closed session's decode planes and tiles stayed in
//! the footprint (`MALLOC_LARGE (empty)`, ~650 MB on the 8-RAW sample) after
//! their `free`, `malloc_zone_pressure_relief` returned nothing, and only the
//! process-start switches `MallocLargeCache=0` / `MallocSpaceEfficient=1`
//! gave them back. This allocator maps blocks of [`LARGE`] bytes and more
//! with `mmap` and unmaps them on free, so pixel buffers leave the footprint
//! the moment they are dropped; everything smaller stays with the system
//! allocator. It applies to all Rust code linked into the app, the CLI and
//! the tests (one `#[global_allocator]` per binary).
use std::alloc::{GlobalAlloc, Layout, System};

/// Blocks at least this large go to the OS directly: every tile (a 256×256
/// f16 RGB tile is 384 KB), plane and decode buffer; not strings or rows.
pub(crate) const LARGE: usize = 128 << 10;
/// Largest alignment `mmap` satisfies without over-allocating (the page).
const PAGE: usize = 16 << 10;

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

/// System malloc for small blocks, `mmap`/`munmap` for large ones.
pub(crate) struct LargeToOs;

impl LargeToOs {
    #[inline]
    fn large(layout: &Layout, size: usize) -> bool {
        cfg!(target_os = "macos") && size >= LARGE && layout.align() <= PAGE
    }
}

unsafe impl GlobalAlloc for LargeToOs {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if Self::large(&layout, layout.size()) {
            #[cfg(target_os = "macos")]
            // SAFETY: a fresh mapping of the requested size.
            return unsafe { os::map(layout.size()) };
        }
        // SAFETY: forwarded unchanged.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if Self::large(&layout, layout.size()) {
            // Anonymous pages are zero-filled.
            // SAFETY: as `alloc`.
            return unsafe { self.alloc(layout) };
        }
        // SAFETY: forwarded unchanged.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if Self::large(&layout, layout.size()) {
            #[cfg(target_os = "macos")]
            // SAFETY: `ptr` was mapped by `alloc` with this layout.
            unsafe {
                os::unmap(ptr, layout.size())
            };
            #[cfg(target_os = "macos")]
            return;
        }
        // SAFETY: `ptr` came from `System.alloc` with this layout.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if !Self::large(&layout, layout.size()) && !Self::large(&layout, new_size) {
            // SAFETY: both sizes are the system allocator's.
            return unsafe { System.realloc(ptr, layout, new_size) };
        }
        // A block changing class (or a mapped block growing: there is no
        // `mremap`) moves: allocate, copy what fits, free the old block.
        // SAFETY: `new_size` is non-zero for a large class; the alignment is the caller's.
        let new_layout = unsafe { Layout::from_size_align_unchecked(new_size, layout.align()) };
        // SAFETY: as `alloc`.
        let new_ptr = unsafe { self.alloc(new_layout) };
        if !new_ptr.is_null() {
            // SAFETY: both blocks are live and at least `min` bytes long.
            unsafe {
                std::ptr::copy_nonoverlapping(ptr, new_ptr, layout.size().min(new_size));
                self.dealloc(ptr, layout);
            }
        }
        new_ptr
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
        let planes: Vec<Vec<f32>> = (1..=4).map(|i| vec![i as f32; 256 << 18]).collect();
        let held = crate::mem_trace::phys_footprint().unwrap();
        assert!(held >= before + (900 << 20), "{before} -> {held}");
        drop(planes);
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
