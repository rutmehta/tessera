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
    shared: Option<gpu_core::GpuDevice>,
}

impl GpuContext {
    pub(crate) fn device_failure(&self) -> Option<String> {
        self.shared.as_ref().and_then(|s| s.device_failure())
    }

    /// The shared device handle for contexts opened by `new` or `from_shared`.
    ///
    /// # Panics
    /// Panics for [`Self::from_device`] contexts. Use [`Self::shared_device`]
    /// when the context's origin is not known, or use `device`/`queue` directly.
    pub fn shared(&self) -> &gpu_core::GpuDevice {
        self.shared_device()
            .expect("from_device contexts have no GpuDevice wrapper")
    }

    /// Returns the original wrapper, if constructed with `new`/`from_shared`.
    pub fn shared_device(&self) -> Option<&gpu_core::GpuDevice> {
        self.shared.as_ref()
    }

    /// Opens the shared device ([`gpu_core::GpuDevice::new`]) and compiles
    /// the operator pipeline on it.
    pub fn new() -> EngineResult<Self> {
        Self::from_shared(gpu_core::GpuDevice::new()?)
    }

    /// Compiles the operator pipeline on an existing shared device.
    pub fn from_shared(shared: gpu_core::GpuDevice) -> EngineResult<Self> {
        let mut context = Self::from_device(&shared.device, &shared.queue)?;
        context.adapter_info = shared.adapter_info.clone();
        context.capabilities = shared.capabilities;
        context.shared = Some(shared);
        Ok(context)
    }

    /// Compiles on the caller's existing Metal device; clones handles only.
    /// `queue` must belong to `device`. No new device, submissions or pixel
    /// transfers are made. The caller must provide sufficient device limits
    /// for the operators it uses (see [`gpu_core::limits`]).
    ///
    /// Enabled features are queried from the device. This does not replace
    /// the caller's device-loss callback: the caller remains responsible for
    /// loss notification/recovery; polling/validation errors still propagate.
    pub fn from_device(device: &wgpu::Device, queue: &wgpu::Queue) -> EngineResult<Self> {
        let adapter_info = device.adapter_info();
        if adapter_info.backend != wgpu::Backend::Metal {
            return Err(EngineError::invalid("GPU context", "requires Metal"));
        }
        let features = device.features();
        let capabilities = GpuCapabilities {
            timestamp_query: features.contains(wgpu::Features::TIMESTAMP_QUERY),
            shader_f16: features.contains(wgpu::Features::SHADER_F16),
            // Rgba16Float storage is part of wgpu's guaranteed format support.
            rgba16float_storage: true,
            passthrough_shaders: features.contains(wgpu::Features::PASSTHROUGH_SHADERS),
        };
        let device = device.clone();
        let queue = queue.clone();
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
            adapter_info,
            capabilities,
            pipeline,
            local_tone_pipelines: std::sync::Mutex::new(None),
            lens_pipelines: std::sync::OnceLock::new(),
            cfa_pipelines: std::sync::OnceLock::new(),
            band_pipelines: std::sync::OnceLock::new(),
            resize_pipeline: std::sync::OnceLock::new(),
            metrics_pipeline: std::sync::OnceLock::new(),
            shared: None,
        })
    }
}
