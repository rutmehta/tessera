use engine_api::{EngineError, EngineResult};

pub use gpu_core::GpuCapabilities;

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
    /// Export row-band gather and interleave (compiled on first use).
    pub(crate) cfa_pipelines: std::sync::OnceLock<[wgpu::ComputePipeline; 2]>,
    pub(crate) band_pipelines: std::sync::OnceLock<[wgpu::ComputePipeline; 2]>,
    /// Export Lanczos-3 resize (compiled on first use).
    pub(crate) resize_pipeline: std::sync::OnceLock<wgpu::ComputePipeline>,
    pub(crate) metrics_pipeline: std::sync::OnceLock<wgpu::ComputePipeline>,
    shared: gpu_core::GpuDevice,
}

impl GpuContext {
    pub(crate) fn device_failure(&self) -> Option<String> {
        self.shared.device_failure()
    }

    /// The shared device handle, for other GPU consumers (the layer
    /// compositor) so the app keeps one Metal context.
    pub fn shared(&self) -> &gpu_core::GpuDevice {
        &self.shared
    }

    /// Opens the shared device ([`gpu_core::GpuDevice::new`]) and compiles
    /// the operator pipeline on it.
    pub fn new() -> EngineResult<Self> {
        Self::from_shared(gpu_core::GpuDevice::new()?)
    }

    /// Compiles the operator pipeline on an existing shared device.
    pub fn from_shared(shared: gpu_core::GpuDevice) -> EngineResult<Self> {
        let device = shared.device.clone();
        let queue = shared.queue.clone();
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
            adapter_info: shared.adapter_info.clone(),
            capabilities: shared.capabilities,
            pipeline,
            local_tone_pipelines: std::sync::Mutex::new(None),
            lens_pipelines: std::sync::OnceLock::new(),
            cfa_pipelines: std::sync::OnceLock::new(),
            band_pipelines: std::sync::OnceLock::new(),
            resize_pipeline: std::sync::OnceLock::new(),
            metrics_pipeline: std::sync::OnceLock::new(),
            shared,
        })
    }
}
