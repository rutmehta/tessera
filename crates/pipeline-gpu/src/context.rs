use engine_api::{EngineError, EngineResult};

/// Optional adapter capabilities. In-flight samples always remain f32.
#[derive(Debug, Clone, Copy)]
pub struct GpuCapabilities {
    pub timestamp_query: bool,
    /// WGSL f16, including storage-buffer elements.
    pub shader_f16: bool,
    pub rgba16float_storage: bool,
}

/// Shared Metal device and queue. Initialization fails explicitly without Metal.
pub struct GpuContext {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub adapter_info: wgpu::AdapterInfo,
    pub capabilities: GpuCapabilities,
    pub(crate) pipeline: wgpu::ComputePipeline,
    pub(crate) local_tone_pipelines:
        std::sync::Mutex<Option<(wgpu::ComputePipeline, wgpu::ComputePipeline)>>,
    /// Export lens kernels (lateral CA, vignetting, remap), compiled on use.
    pub(crate) lens_pipelines: std::sync::OnceLock<[wgpu::ComputePipeline; 3]>,
    device_loss: std::sync::Arc<std::sync::Mutex<Option<String>>>,
}

/// Defaults plus what whole-level resident filters need: nine storage
/// bindings, 32 KiB workgroup tiles and single buffers up to 1 GiB (a 61 MP
/// level as packed f32 RGBA), each capped by the adapter.
fn limits(adapter: &wgpu::Limits) -> wgpu::Limits {
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

impl GpuContext {
    pub(crate) fn device_failure(&self) -> Option<String> {
        self.device_loss
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
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
            label: Some("tessera M1"),
            required_features: features
                & (wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::SHADER_F16),
            required_limits: limits(&adapter.limits()),
            ..Default::default()
        }))
        .map_err(|e| EngineError::internal(format!("Metal device: {e}")))?;
        let device_loss = std::sync::Arc::new(std::sync::Mutex::new(None));
        let loss = device_loss.clone();
        device.set_device_lost_callback(move |reason, message| {
            let detail = format!("Metal device lost: {reason:?}: {message}");
            eprintln!("{detail}");
            *loss.lock().unwrap_or_else(|e| e.into_inner()) = Some(detail);
        });
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("M1 operators"),
            source: wgpu::ShaderSource::Wgsl(include_str!("operators.wgsl").into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("M1 operators"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(EngineError::internal(format!("M1 shader: {e}")));
        }
        Ok(Self {
            device,
            queue,
            adapter_info: adapter.get_info(),
            capabilities,
            pipeline,
            local_tone_pipelines: std::sync::Mutex::new(None),
            lens_pipelines: std::sync::OnceLock::new(),
            device_loss,
        })
    }
}
