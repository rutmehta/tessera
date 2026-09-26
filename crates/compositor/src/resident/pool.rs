//! The GPU page pool: fixed-size pages in up to eight storage-buffer slabs.

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

    /// Whether another slab can be added.
    pub fn can_grow(&self) -> bool {
        self.slabs.len() < self.max_slabs
    }

    /// Adds a slab for at least `need` more pages (sized to the larger of
    /// `need` plus a quarter and the pages so far, capped by the binding
    /// limit).
    pub fn grow(&mut self, device: &wgpu::Device, need: u32) -> EngineResult<()> {
        if !self.can_grow() {
            return Err(EngineError::ResourceExhausted {
                resource: format!("{} (all {} slabs allocated)", self.label, self.max_slabs),
            });
        }
        let pages = (need + need / 4)
            .max(self.total)
            .max(MIN_SLAB_PAGES)
            .min(self.max_slab_pages);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(self.label),
            size: u64::from(pages) * self.page_bytes,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
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
