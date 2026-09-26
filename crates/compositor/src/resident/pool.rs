//! The GPU page pool: fixed-size pages in up to eight storage-buffer slabs.
//!
//! The first slab grows by reallocation (a GPU copy into a larger buffer,
//! page numbers unchanged) up to the storage-binding limit; only then are
//! further slabs added. Kernels therefore almost always see one slab and
//! load pages without a per-texel slab switch, and pool growth does not
//! change the specialized kernels' structure key.

use engine_api::{EngineError, EngineResult};

/// Slabs bound per dispatch (pages.wgsl).
pub(super) const SLABS: usize = 8;
/// Smallest slab, in pages.
const MIN_SLAB_PAGES: u32 = 32;

pub(super) struct Pool {
    pub page_bytes: u64,
    max_slab_pages: u32,
    /// (buffer, first page, page count)
    pub slabs: Vec<(wgpu::Buffer, u32, u32)>,
    free: Vec<u32>,
    next: u32,
    total: u32,
    max_slabs: usize,
    label: &'static str,
}

impl Pool {
    pub fn new(
        device: &wgpu::Device,
        page_bytes: u64,
        label: &'static str,
        max_slabs: usize,
    ) -> EngineResult<Self> {
        let l = device.limits();
        let max = l.max_storage_buffer_binding_size.min(l.max_buffer_size);
        let max_slab_pages = u32::try_from(max / page_bytes).unwrap_or(u32::MAX);
        if max_slab_pages == 0 {
            return Err(EngineError::Unsupported {
                what: "device storage binding limit is smaller than one page".into(),
            });
        }
        Ok(Self {
            page_bytes,
            max_slab_pages,
            slabs: Vec::new(),
            free: Vec::new(),
            next: 0,
            total: 0,
            max_slabs: max_slabs.min(SLABS),
            label,
        })
    }

    /// Pages that can be handed out without allocating a slab.
    pub fn available(&self) -> u32 {
        self.free.len() as u32 + (self.total - self.next)
    }

    /// Allocated slab bytes.
    pub fn bytes(&self) -> u64 {
        u64::from(self.total) * self.page_bytes
    }

    /// Whether the pool can take more pages.
    pub fn can_grow(&self) -> bool {
        self.slabs.len() < self.max_slabs
            || self
                .slabs
                .last()
                .is_some_and(|&(_, _, pages)| pages < self.max_slab_pages)
    }

    fn buffer(&self, device: &wgpu::Device, pages: u32) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(self.label),
            size: u64::from(pages) * self.page_bytes,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        })
    }

    /// Makes room for more pages, at least `need` if the binding limit
    /// allows: the last slab is reallocated at least twice as large (and to
    /// fit `need` plus a quarter) while under the binding limit, with its
    /// pages copied on the GPU (queued before any later upload); otherwise a
    /// slab is added. Growth past `cap` total pages is limited to `need`.
    pub fn grow(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        need: u32,
        cap: u32,
    ) -> EngineResult<()> {
        if !self.can_grow() {
            return Err(EngineError::ResourceExhausted {
                resource: format!("{} (all {} slabs allocated)", self.label, self.max_slabs),
            });
        }
        let want = need.saturating_add(need / 4);
        if let Some(&(ref old, first, pages)) = self.slabs.last()
            && pages < self.max_slab_pages
        {
            let room = cap
                .saturating_sub(self.total - pages)
                .max(pages.saturating_add(need));
            let grown = pages
                .saturating_mul(2)
                .max(pages.saturating_add(want))
                .min(room)
                .min(self.max_slab_pages);
            let buffer = self.buffer(device, grown);
            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("resident pool growth"),
            });
            enc.copy_buffer_to_buffer(old, 0, &buffer, 0, u64::from(pages) * self.page_bytes);
            queue.submit([enc.finish()]);
            let last = self.slabs.len() - 1;
            self.slabs[last] = (buffer, first, grown);
            self.total += grown - pages;
            return Ok(());
        }
        let pages = want
            .max(self.total)
            .max(MIN_SLAB_PAGES)
            .min(cap.saturating_sub(self.total).max(need))
            .min(self.max_slab_pages);
        let buffer = self.buffer(device, pages);
        self.slabs.push((buffer, self.total, pages));
        self.total += pages;
        Ok(())
    }

    pub fn alloc(&mut self) -> Option<u32> {
        if let Some(p) = self.free.pop() {
            return Some(p);
        }
        (self.next < self.total).then(|| {
            self.next += 1;
            self.next - 1
        })
    }

    pub fn release(&mut self, page: u32) {
        self.free.push(page);
    }

    /// (slab buffer, byte offset) of a page.
    pub fn locate(&self, page: u32) -> (&wgpu::Buffer, u64) {
        let (b, first, _) = self
            .slabs
            .iter()
            .rev()
            .find(|(_, first, _)| *first <= page)
            .expect("page in an allocated slab");
        (b, u64::from(page - first) * self.page_bytes)
    }

    /// The shader address of a page: `(slab << 29) | page-in-slab`.
    pub fn packed(&self, page: u32) -> u32 {
        let (i, (_, first, _)) = self
            .slabs
            .iter()
            .enumerate()
            .rev()
            .find(|(_, (_, first, _))| *first <= page)
            .expect("page in an allocated slab");
        ((i as u32) << 29) | (page - first)
    }
}
