//! The process's shared Metal device: one `wgpu::Device`/`wgpu::Queue` pair
//! with the limits every GPU consumer needs (the develop pipeline in
//! `pipeline-gpu` and the layer compositor), device-loss tracking and
//! same-device IOSurface import for presentation.
//!
//! This crate has no image-pipeline dependencies (no image-core, raw-decode
//! or LibRaw), so the compositor can share the app's device without pulling
//! in the raw stack. Cloning a [`GpuDevice`] clones handles, not the device.
#![deny(unsafe_code)]

mod iosurface;
pub use iosurface::{SurfaceFormat, write_to_iosurface};

use std::sync::{Arc, Mutex};

use engine_api::{EngineError, EngineResult};

/// Optional adapter capabilities. In-flight samples always remain f32.
#[derive(Debug, Clone, Copy)]
pub struct GpuCapabilities {
    /// Timestamp queries are available.
    pub timestamp_query: bool,
    /// WGSL f16, including storage-buffer elements.
    pub shader_f16: bool,
    /// `Rgba16Float` storage textures.
    pub rgba16float_storage: bool,
}

/// Defaults plus what whole-level resident work needs: sixteen storage
/// bindings, 32 KiB workgroup storage and single buffers up to 1 GiB (a
/// 61 MP level as packed f32 RGBA), each capped by the adapter.
pub fn limits(adapter: &wgpu::Limits) -> wgpu::Limits {
    let base = wgpu::Limits::default();
    wgpu::Limits {
        max_storage_buffers_per_shader_stage: adapter.max_storage_buffers_per_shader_stage.min(16),
        max_compute_workgroup_storage_size: adapter
            .max_compute_workgroup_storage_size
            .min(32 << 10),
        max_storage_buffer_binding_size: adapter.max_storage_buffer_binding_size.min(1 << 30),
        max_buffer_size: adapter.max_buffer_size.min(1 << 30),
        ..base
    }
}

/// A shared Metal device and queue. Initialization fails explicitly without
/// Metal.
#[derive(Clone)]
pub struct GpuDevice {
    /// The device.
    pub device: wgpu::Device,
    /// Its queue (one per device; every consumer submits here).
    pub queue: wgpu::Queue,
    /// Adapter description.
    pub adapter_info: wgpu::AdapterInfo,
    /// Optional features that were enabled.
    pub capabilities: GpuCapabilities,
    device_loss: Arc<Mutex<Option<String>>>,
}

impl std::fmt::Debug for GpuDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuDevice")
            .field("adapter", &self.adapter_info.name)
            .field("capabilities", &self.capabilities)
            .finish()
    }
}

impl GpuDevice {
    /// Opens the high-performance Metal adapter with [`limits`] and the
    /// timestamp/f16 features when available.
    pub fn new() -> EngineResult<Self> {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::METAL;
        let instance = wgpu::Instance::new(desc);
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .map_err(|e| EngineError::internal(format!("Metal adapter: {e}")))?;
        let features = adapter.features();
        let capabilities = GpuCapabilities {
            timestamp_query: features.contains(wgpu::Features::TIMESTAMP_QUERY),
            shader_f16: features.contains(wgpu::Features::SHADER_F16),
            rgba16float_storage: adapter
                .get_texture_format_features(wgpu::TextureFormat::Rgba16Float)
                .allowed_usages
                .contains(wgpu::TextureUsages::STORAGE_BINDING),
        };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("tessera"),
            required_features: features
                & (wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::SHADER_F16),
            required_limits: limits(&adapter.limits()),
            ..Default::default()
        }))
        .map_err(|e| EngineError::internal(format!("Metal device: {e}")))?;
        let device_loss = Arc::new(Mutex::new(None));
        let loss = device_loss.clone();
        device.set_device_lost_callback(move |reason, message| {
            let detail = format!("Metal device lost: {reason:?}: {message}");
            eprintln!("{detail}");
            *loss.lock().unwrap_or_else(|e| e.into_inner()) = Some(detail);
        });
        Ok(Self {
            device,
            queue,
            adapter_info: adapter.get_info(),
            capabilities,
            device_loss,
        })
    }

    /// The device-loss message, once the device has been lost.
    pub fn device_failure(&self) -> Option<String> {
        self.device_loss
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Blocks until all submitted work has completed.
    pub fn wait(&self) -> EngineResult<()> {
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| EngineError::Gpu {
                message: e.to_string(),
            })?;
        match self.device_failure() {
            Some(message) => Err(EngineError::Gpu { message }),
            None => Ok(()),
        }
    }
}

/// Copies `size` bytes at `offset` of a `COPY_SRC` buffer to the CPU
/// (blocking). Used for explicit export/readback paths only.
pub fn read_buffer(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    src: &wgpu::Buffer,
    offset: u64,
    size: u64,
) -> EngineResult<Vec<u8>> {
    let gpu = |e: &dyn std::fmt::Display| EngineError::Gpu {
        message: e.to_string(),
    };
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&Default::default());
    enc.copy_buffer_to_buffer(src, offset, &staging, 0, size);
    queue.submit([enc.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    staging.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| gpu(&e))?;
    rx.recv().map_err(|e| gpu(&e))?.map_err(|e| gpu(&e))?;
    let data = staging
        .slice(..)
        .get_mapped_range()
        .map_err(|e| gpu(&e))?
        .to_vec();
    staging.unmap();
    Ok(data)
}
