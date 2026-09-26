//! Zero-copy interop with caller-owned buffers on the context's device.
use super::Storage;
use crate::GpuContext;
use engine_api::{
    EngineError, EngineResult,
    tile::{TileCoord, TileLayout},
};
use image_core::resident::ResidentTile;
use std::sync::{Arc, Weak};

/// An owning view of resident storage. Retains the tile, not just a wgpu
/// handle, so batch scratch recycling cannot overwrite the exported buffer.
/// Keep this guard alive until external consumers have completed. Do not
/// destroy, map, or mutate its buffer while any resident/cache users exist.
#[derive(Clone)]
pub struct ResidentBuffer {
    tile: ResidentTile,
}
impl ResidentBuffer {
    /// The complete allocation, starting at byte zero. A cloned buffer handle
    /// alone does not prevent scratch recycling; retain this guard as well.
    pub fn buffer(&self) -> &wgpu::Buffer {
        &self.tile.storage.downcast_ref::<Storage>().unwrap().buffer
    }
    /// Planar geometry including halo. There is no row/plane padding.
    pub fn layout(&self) -> TileLayout {
        self.tile.layout
    }
    /// False means f32 samples. True means pairs of IEEE f16 samples packed
    /// into u32 words (first sample in low 16 bits), with a padded final word
    /// for an odd sample count. This flag describes storage, not color space.
    pub fn packed(&self) -> bool {
        self.tile.storage.downcast_ref::<Storage>().unwrap().packed
    }
}

impl GpuContext {
    /// Imports a whole, unmapped same-device buffer without copying pixels.
    /// Samples must be tightly packed **planar f32**, beginning at byte zero,
    /// including the halo described by `layout`. Interleaved RGBA, offsets,
    /// padded strides and packed f16 are not accepted by this entrypoint.
    /// The allocation may be larger than the payload, but must fit a storage
    /// binding and have STORAGE | COPY_SRC usage. Geometry must be nonempty.
    /// The caller must ensure buffer/device/queue come from the same device
    /// and wgpu Instance. Scoped binding validation catches ordinary misuse,
    /// but wgpu exposes no buffer-owner query and cannot reliably validate
    /// provenance across unrelated Instances. Contents/packing are not inspected.
    ///
    /// Clones the handle; never destroys it or returns it to the scratch pool.
    /// The caller must not destroy/map/mutate it while resident users exist.
    /// Submit producer commands on this context's queue before finishing the
    /// consuming batch. Import itself neither submits nor waits. Pending
    /// queue writes execute before that batch's compute commands.
    pub fn import_resident_buffer(
        &self,
        coord: TileCoord,
        layout: TileLayout,
        buffer: &wgpu::Buffer,
    ) -> EngineResult<ResidentTile> {
        let invalid = |reason| EngineError::invalid("resident buffer", reason);
        let bytes = (layout.stride() as u64)
            .checked_mul(layout.rows() as u64)
            .and_then(|n| n.checked_mul(layout.channels as u64))
            .and_then(|n| n.checked_mul(4))
            .ok_or_else(|| invalid("layout overflow"))?;
        if layout.extent.width == 0 || layout.extent.height == 0 || layout.channels == 0 {
            return Err(invalid("empty layout"));
        }
        if bytes > buffer.size()
            || buffer.size() > self.device.limits().max_storage_buffer_binding_size
        {
            return Err(invalid("layout exceeds buffer or storage binding limit"));
        }
        if !buffer
            .usage()
            .contains(wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC)
        {
            return Err(invalid("requires STORAGE | COPY_SRC usage"));
        }
        // wgpu exposes no Buffer::device(). A scoped binding validation checks
        // provenance without dispatching, submitting, or reading any pixels.
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let binding_layout =
            self.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("resident import validation"),
                    entries: &[wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    }],
                });
        let _binding = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("resident import validation"),
            layout: &binding_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        if let Some(error) = pollster::block_on(scope.pop()) {
            return Err(EngineError::invalid("resident buffer", error.to_string()));
        }
        Ok(ResidentTile {
            coord,
            layout,
            storage: Arc::new(Storage {
                buffer: buffer.clone(),
                packed: false,
                device: self.device.clone(),
                pool: Weak::new(),
            }),
        })
    }

    /// Exports an owning buffer/layout/packing view, rejecting foreign backend
    /// or device tiles. This does not submit, wait, copy or read back pixels.
    ///
    /// For GPU-only completion, retain the returned guard and call
    /// `batch.finish(vec![], false, None, &cancel)` on a normal resident batch.
    /// That submits and waits without pixel readback; only after success may
    /// an external consumer use the output. Dropping a batch discards its
    /// unsubmitted work; `checkpoint` is not an unconditional submission.
    /// The empty finish path does not apply configured export resize.
    pub fn resident_buffer(&self, tile: &ResidentTile) -> EngineResult<ResidentBuffer> {
        let storage = tile
            .storage
            .downcast_ref::<Storage>()
            .ok_or_else(|| EngineError::invalid("resident tile", "foreign backend"))?;
        if storage.device != self.device {
            return Err(EngineError::invalid("resident tile", "foreign device"));
        }
        Ok(ResidentBuffer { tile: tile.clone() })
    }
}
