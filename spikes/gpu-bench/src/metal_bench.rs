//! Native Metal path via objc2-metal. The MSL source is compiled at runtime
//! with `newLibraryWithSource:options:error:` (default compile options, i.e.
//! Metal's default fast-math mode). GPU time is
//! `MTLCommandBuffer.GPUEndTime - GPUStartTime` of the one command buffer that
//! holds all dispatches of a kernel.

use crate::data::{Px, H, W};
use crate::Timing;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::NSString;
use objc2_metal::{
    MTLBuffer, MTLCommandBuffer, MTLCommandEncoder, MTLCommandQueue, MTLCompileOptions,
    MTLComputeCommandEncoder, MTLComputePipelineState, MTLCreateSystemDefaultDevice, MTLDevice,
    MTLLibrary, MTLResourceOptions, MTLSize,
};
use std::ptr::NonNull;
use std::time::Instant;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {}

type Buf = Retained<ProtocolObject<dyn MTLBuffer>>;
type Pso = Retained<ProtocolObject<dyn MTLComputePipelineState>>;

pub struct Ctx {
    device: Retained<ProtocolObject<dyn MTLDevice>>,
    queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    library: Retained<ProtocolObject<dyn MTLLibrary>>,
    pub device_name: String,
}

pub struct Job {
    /// (pipeline, [(buffer index, buffer)])
    passes: Vec<(Pso, Vec<(usize, Buf)>)>,
    output: Buf,
}

impl Ctx {
    pub fn new() -> Result<Ctx, String> {
        let device = MTLCreateSystemDefaultDevice().ok_or("no Metal device")?;
        let queue = device.newCommandQueue().ok_or("newCommandQueue failed")?;
        let src = include_str!("../shaders/kernels.metal")
            .replace("__W__", &W.to_string())
            .replace("__H__", &H.to_string());
        let opts = MTLCompileOptions::new();
        let library = device
            .newLibraryWithSource_options_error(&NSString::from_str(&src), Some(&opts))
            .map_err(|e| format!("MSL compile failed: {}", e.localizedDescription()))?;
        let device_name = device.name().to_string();
        Ok(Ctx {
            device,
            queue,
            library,
            device_name,
        })
    }

    fn pso(&self, name: &str) -> Result<Pso, String> {
        let f = self
            .library
            .newFunctionWithName(&NSString::from_str(name))
            .ok_or_else(|| format!("no MSL function {name}"))?;
        self.device
            .newComputePipelineStateWithFunction_error(&f)
            .map_err(|e| format!("pipeline {name}: {}", e.localizedDescription()))
    }

    fn buf_init(&self, bytes: &[u8]) -> Result<Buf, String> {
        // SAFETY: pointer/length describe a live slice; Metal copies it.
        unsafe {
            self.device.newBufferWithBytes_length_options(
                NonNull::new(bytes.as_ptr() as *mut _).unwrap(),
                bytes.len(),
                MTLResourceOptions::StorageModeShared,
            )
        }
        .ok_or_else(|| "newBufferWithBytes failed".to_string())
    }

    fn buf(&self) -> Result<Buf, String> {
        self.device
            .newBufferWithLength_options(W * H * 16, MTLResourceOptions::StorageModeShared)
            .ok_or_else(|| "newBufferWithLength failed".to_string())
    }

    pub fn demosaic_job(&self, cfa: &[u16]) -> Result<Job, String> {
        let input = self.buf_init(bytemuck::cast_slice(cfa))?;
        let out = self.buf()?;
        Ok(Job {
            passes: vec![(self.pso("demosaic")?, vec![(0, input), (1, out.clone())])],
            output: out,
        })
    }

    pub fn guided_job(&self, img: &[Px]) -> Result<Job, String> {
        let i = self.buf_init(bytemuck::cast_slice(img))?;
        let (t1, t2, a, b) = (self.buf()?, self.buf()?, self.buf()?, self.buf()?);
        let passes = vec![
            (
                self.pso("hbox_in")?,
                vec![(0, i.clone()), (3, t1.clone()), (4, t2.clone())],
            ),
            (
                self.pso("vbox_coef")?,
                vec![
                    (0, t1.clone()),
                    (1, t2.clone()),
                    (3, a.clone()),
                    (4, b.clone()),
                ],
            ),
            (
                self.pso("hbox_ab")?,
                vec![
                    (0, a.clone()),
                    (1, b.clone()),
                    (3, t1.clone()),
                    (4, t2.clone()),
                ],
            ),
            (
                self.pso("vbox_out")?,
                vec![(0, t1), (1, t2), (2, i), (3, b.clone())],
            ),
        ];
        Ok(Job { passes, output: b })
    }

    pub fn lut_job(&self, img: &[Px], lut: &[Px]) -> Result<Job, String> {
        let src = self.buf_init(bytemuck::cast_slice(img))?;
        let l = self.buf_init(bytemuck::cast_slice(lut))?;
        let out = self.buf()?;
        Ok(Job {
            passes: vec![(self.pso("lut3d")?, vec![(0, src), (1, l), (2, out.clone())])],
            output: out,
        })
    }

    pub fn run(&self, job: &Job, warmup: usize, runs: usize) -> Result<(Timing, Vec<f32>), String> {
        let mut out = vec![0f32; W * H * 4];
        let (mut gpu, mut wall) = (Vec::new(), Vec::new());
        for it in 0..warmup + runs {
            let t0 = Instant::now();
            let cb = self.queue.commandBuffer().ok_or("commandBuffer failed")?;
            let enc = cb
                .computeCommandEncoder()
                .ok_or("computeCommandEncoder failed")?;
            for (pso, bufs) in &job.passes {
                enc.setComputePipelineState(pso);
                for (idx, b) in bufs {
                    // SAFETY: buffers are live and sized for the kernel's indexing.
                    unsafe { enc.setBuffer_offset_atIndex(Some(b), 0, *idx) };
                }
                // Serial dispatch type: dispatches run in order with hazard tracking
                // on these (tracked) buffers, matching wgpu's per-dispatch barriers.
                enc.dispatchThreadgroups_threadsPerThreadgroup(
                    MTLSize {
                        width: W / 16,
                        height: H / 16,
                        depth: 1,
                    },
                    MTLSize {
                        width: 16,
                        height: 16,
                        depth: 1,
                    },
                );
            }
            enc.endEncoding();
            cb.commit();
            cb.waitUntilCompleted();
            if let Some(e) = cb.error() {
                return Err(format!(
                    "command buffer error: {}",
                    e.localizedDescription()
                ));
            }
            let g = (cb.GPUEndTime() - cb.GPUStartTime()) * 1e3;
            // Readback: shared storage is CPU-visible; copy it out like the wgpu path does.
            // SAFETY: the buffer holds W*H float4 and the GPU work has completed.
            let src = unsafe {
                std::slice::from_raw_parts(job.output.contents().as_ptr() as *const f32, W * H * 4)
            };
            out.copy_from_slice(src);
            let w = t0.elapsed().as_secs_f64() * 1e3;
            if it >= warmup {
                gpu.push(g);
                wall.push(w);
            }
        }
        Ok((Timing::from_samples(gpu, wall), out))
    }
}
