use crate::Tensor;
use anyhow::{Result, ensure};
use ort::{ep, session::builder::GraphOptimizationLevel, value::Tensor as OrtTensor};
use std::{collections::BTreeSet, path::Path};

pub use ort::ep::coreml::{ComputeUnits, ModelFormat};

#[derive(Clone, Debug)]
pub struct SessionOptions {
    pub coreml: bool,
    pub compute_units: ComputeUnits,
    pub model_format: ModelFormat,
}
impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            coreml: cfg!(target_os = "macos"),
            compute_units: ComputeUnits::All,
            model_format: ModelFormat::MLProgram,
        }
    }
}
impl SessionOptions {
    pub fn cpu() -> Self {
        Self {
            coreml: false,
            ..Self::default()
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct NodeAssignment {
    pub node: String,
    pub provider: String,
}
/// Executed optimized nodes, including fused CoreML subgraphs, from ORT profiling.
#[derive(Debug, Clone)]
pub struct PartitionReport {
    pub nodes: Vec<NodeAssignment>,
}
impl PartitionReport {
    pub fn require_coreml(&self) -> Result<()> {
        ensure!(
            !self.nodes.is_empty(),
            "partition report contains no executed nodes"
        );
        ensure!(
            self.nodes
                .iter()
                .all(|n| n.provider == "CoreMLExecutionProvider"),
            "model has non-CoreML nodes: {:?}",
            self.nodes
        );
        Ok(())
    }
}

pub struct Session {
    inner: ort::session::Session,
    _profile_dir: tempfile::TempDir,
    report: Option<PartitionReport>,
    runs: usize,
    /// If CoreML initialization failed, the reason for rebuilding on CPU.
    pub fallback_reason: Option<String>,
}
impl Session {
    pub fn load(path: impl AsRef<Path>, options: SessionOptions) -> Result<Self> {
        let dir = tempfile::tempdir()?;
        let build = |coreml: bool| -> Result<ort::session::Session> {
            let mut builder = ort::session::Session::builder()?
                .with_optimization_level(GraphOptimizationLevel::Level1)
                .map_err(ort::Error::<()>::from)?
                .with_intra_threads(1)
                .map_err(ort::Error::<()>::from)?
                .with_profiling(dir.path().join(if coreml { "coreml" } else { "cpu" }))
                .map_err(ort::Error::<()>::from)?;
            if coreml {
                builder = builder
                    .with_execution_providers([
                        ep::CoreML::default()
                            .with_compute_units(options.compute_units)
                            .with_model_format(options.model_format)
                            .build()
                            .error_on_failure(),
                        ep::CPU::default().build(),
                    ])
                    .map_err(ort::Error::<()>::from)?;
            } else {
                builder = builder
                    .with_execution_providers([ep::CPU::default().build()])
                    .map_err(ort::Error::<()>::from)?;
            }
            Ok(builder.commit_from_file(path.as_ref())?)
        };
        let (inner, fallback_reason) = match build(options.coreml) {
            Ok(s) => (s, None),
            Err(e) if options.coreml => (build(false)?, Some(e.to_string())),
            Err(e) => return Err(e),
        };
        Ok(Self {
            inner,
            _profile_dir: dir,
            report: None,
            runs: 0,
            fallback_reason,
        })
    }
    /// Executes manifest-specified zero inputs for partition auditing. Models
    /// with data-dependent branches require additional representative runs.
    pub fn probe(&mut self, specs: &[crate::TensorSpec]) -> Result<()> {
        let mut inputs = Vec::new();
        for spec in specs {
            let len = spec
                .shape
                .iter()
                .try_fold(1usize, |a, b| a.checked_mul(*b))
                .ok_or_else(|| anyhow::anyhow!("shape overflow"))?;
            ensure!(len > 0, "empty probe shape");
            let value = match spec.dtype {
                crate::Dtype::Fp32 => {
                    OrtTensor::from_array((spec.shape.clone(), vec![0f32; len]))?.into_dyn()
                }
                crate::Dtype::Fp16 => {
                    OrtTensor::from_array((spec.shape.clone(), vec![half::f16::ZERO; len]))?
                        .into_dyn()
                }
                crate::Dtype::Int8 => {
                    OrtTensor::from_array((spec.shape.clone(), vec![0i8; len]))?.into_dyn()
                }
            };
            inputs.push((spec.name.as_str(), value));
        }
        self.inner.run(inputs)?;
        self.runs += 1;
        Ok(())
    }
    pub fn run(&mut self, input: &Tensor) -> Result<Tensor> {
        ensure!(
            self.inner.inputs().len() == 1 && self.inner.outputs().len() == 1,
            "image run requires one input and one output"
        );
        let value = match self.inner.inputs()[0].dtype().tensor_type() {
            Some(ort::value::TensorElementType::Float32) => {
                OrtTensor::from_array((input.shape(), input.data().to_vec()))?.into_dyn()
            }
            Some(ort::value::TensorElementType::Float16) => {
                OrtTensor::from_array((input.shape(), input.to_f16()))?.into_dyn()
            }
            _ => anyhow::bail!("image run requires fp32/fp16 input"),
        };
        let outputs = self.inner.run(ort::inputs![value])?;
        let (shape, data) =
            if outputs[0].dtype().tensor_type() == Some(ort::value::TensorElementType::Float16) {
                let (s, d) = outputs[0].try_extract_tensor::<half::f16>()?;
                (s, d.iter().map(|v| v.to_f32()).collect::<Vec<_>>())
            } else {
                let (s, d) = outputs[0].try_extract_tensor::<f32>()?;
                (s, d.to_vec())
            };
        ensure!(
            shape.len() == 4 && shape[0] == 1 && shape[1..].iter().all(|n| *n > 0),
            "expected NCHW output"
        );
        self.runs += 1;
        Tensor::new(
            shape[1] as usize,
            shape[2] as usize,
            shape[3] as usize,
            data,
        )
    }
    /// Runs a spatially local, shape-preserving model on overlapping patches.
    /// `halo` must cover the model's receptive radius. Global attention/pooling
    /// models cannot use this path. Boundary patches are clipped to the image so
    /// the model's own padding, rather than artificial pixels, defines edges.
    pub fn run_tiled(&mut self, input: &Tensor, tile_size: usize, halo: usize) -> Result<Tensor> {
        ensure!(tile_size > 0, "tile size must be positive");
        let [_, channels, height, width] = input.shape();
        let mut result = vec![0.; input.data().len()];
        for y in (0..height).step_by(tile_size) {
            for x in (0..width).step_by(tile_size) {
                let end_y = y.saturating_add(tile_size).min(height);
                let end_x = x.saturating_add(tile_size).min(width);
                let y0 = y.saturating_sub(halo);
                let x0 = x.saturating_sub(halo);
                let y1 = end_y.saturating_add(halo).min(height);
                let x1 = end_x.saturating_add(halo).min(width);
                let ph = y1 - y0;
                let pw = x1 - x0;
                let mut patch = Vec::with_capacity(channels * ph * pw);
                for c in 0..channels {
                    for row in y0..y1 {
                        let start = c * height * width + row * width + x0;
                        patch.extend_from_slice(&input.data()[start..start + pw]);
                    }
                }
                let output = self.run(&Tensor::new(channels, ph, pw, patch)?)?;
                ensure!(
                    output.shape() == [1, channels, ph, pw],
                    "tiled model must preserve shape"
                );
                for c in 0..channels {
                    for row in y..end_y {
                        let src = c * ph * pw + (row - y0) * pw + x - x0;
                        let dst = c * height * width + row * width + x;
                        result[dst..dst + end_x - x]
                            .copy_from_slice(&output.data()[src..src + end_x - x]);
                    }
                }
            }
        }
        Tensor::new(channels, height, width, result)
    }
    /// Finalizes profiling. Call after representative inference; subsequent calls
    /// return the same snapshot. Conditional branches not executed are not covered.
    pub fn partition_report(&mut self) -> Result<PartitionReport> {
        if let Some(report) = &self.report {
            return Ok(report.clone());
        }
        ensure!(
            self.runs > 0,
            "run inference before requesting a partition report"
        );
        let path = self.inner.end_profiling()?;
        let events: Vec<serde_json::Value> = serde_json::from_slice(&std::fs::read(path)?)?;
        let mut nodes = BTreeSet::new();
        for event in events {
            if event["cat"] != "Node" {
                continue;
            }
            if let (Some(node), Some(provider)) =
                (event["name"].as_str(), event["args"]["provider"].as_str())
            {
                nodes.insert(NodeAssignment {
                    node: node.trim_end_matches("_kernel_time").to_owned(),
                    provider: provider.to_owned(),
                });
            }
        }
        ensure!(
            !nodes.is_empty(),
            "ORT profile contains no node/provider assignments"
        );
        let report = PartitionReport {
            nodes: nodes.into_iter().collect(),
        };
        self.report = Some(report.clone());
        Ok(report)
    }
}
