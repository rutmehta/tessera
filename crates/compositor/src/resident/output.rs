//! Resident premultiplied RGBA presentation without pixel readback.

use crate::geom::Rect;
use engine_api::{EngineError, EngineResult, tile::Extent};
use gpu_core::Lut3d;
use wgpu::util::DeviceExt;

/// Interpretation of straight RGB obtained by unpremultiplying the composite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceColorPolicy {
    /// Already destination-encoded SDR. RGBA8 only, no LUT or transfer conversion.
    DocumentEncoded,
    /// Already display-linear extended RGB. RGBA16F only, preserves headroom.
    DisplayLinear,
    /// RGB is in the supplied LUT's input space (not implicitly Rec.2020).
    /// Requires a LUT; its output must be encoded for RGBA8, display-linear for
    /// RGBA16F. The caller resolves document/profile identity before using it.
    LutInput,
}

/// Reusable pipelines for presentation. Source and target must belong to the
/// supplied device, and the queue must be that device's queue.
pub struct OutputPresenter {
    sdr: wgpu::ComputePipeline,
    edr: wgpu::ComputePipeline,
}
impl OutputPresenter {
    pub fn new(device: &wgpu::Device) -> EngineResult<Self> {
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let build = |format| {
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("resident output"),
                source: wgpu::ShaderSource::Wgsl(
                    include_str!("output.wgsl")
                        .replace("OUTPUT_FORMAT", format)
                        .into(),
                ),
            });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("resident output"),
                layout: None,
                module: &shader,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let result = Self {
            sdr: build("rgba8unorm"),
            edr: build("rgba16float"),
        };
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(EngineError::internal(e.to_string()));
        }
        Ok(result)
    }

    /// Presents tightly packed interleaved premultiplied f32 RGBA, with no
    /// pixel upload/readback. Caller guarantees finite RGB and alpha in [0,1].
    /// Background is straight RGB in the *destination* encoding; flattening
    /// happens after the color transform. None preserves premultiplied alpha.
    /// A raw 33³ LUT matches pipeline-gpu::GpuOutputLut: input clamps to [0,1],
    /// red-fastest trilinear interpolation, no output clamp/extra transfer.
    /// This is not scene tone mapping, proof selection, or gamut mapping.
    #[allow(clippy::too_many_arguments)]
    pub fn present(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: &wgpu::Buffer,
        extent: Extent,
        src: Rect,
        target: &wgpu::Texture,
        dst: (u32, u32),
        background: Option<[f32; 3]>,
        policy: SourceColorPolicy,
        lut: Option<&Lut3d>,
    ) -> EngineResult<()> {
        let bad = |field, message| EngineError::invalid(field, message);
        if src.is_empty()
            || src.x0 < 0
            || src.y0 < 0
            || src.x1 > i64::from(extent.width)
            || src.y1 > i64::from(extent.height)
        {
            return Err(bad("src", "outside the level"));
        }
        let bytes = u64::from(extent.width)
            .checked_mul(u64::from(extent.height))
            .and_then(|n| n.checked_mul(16));
        if bytes != Some(source.size())
            || source.size() > device.limits().max_storage_buffer_binding_size
            || !source.usage().contains(wgpu::BufferUsages::STORAGE)
        {
            return Err(bad(
                "source",
                "tightly packed f32 RGBA storage buffer required",
            ));
        }
        let float = target.format() == wgpu::TextureFormat::Rgba16Float;
        if (!float && target.format() != wgpu::TextureFormat::Rgba8Unorm)
            || !target
                .usage()
                .contains(wgpu::TextureUsages::STORAGE_BINDING)
            || target.dimension() != wgpu::TextureDimension::D2
            || target.depth_or_array_layers() != 1
            || target.sample_count() != 1
            || target.mip_level_count() != 1
        {
            return Err(bad(
                "target",
                "single-layer, single-mip, single-sample RGBA8/RGBA16F 2D storage texture required",
            ));
        }
        let (w, h) = (src.width() as u32, src.height() as u32);
        if dst.0.checked_add(w).is_none_or(|v| v > target.width())
            || dst.1.checked_add(h).is_none_or(|v| v > target.height())
        {
            return Err(bad("dst", "outside the target"));
        }
        if w.div_ceil(16) > device.limits().max_compute_workgroups_per_dimension
            || h.div_ceil(16) > device.limits().max_compute_workgroups_per_dimension
        {
            return Err(bad("src", "dispatch exceeds device limits"));
        }
        if background.is_some_and(|bg| bg.iter().any(|v| !v.is_finite())) {
            return Err(bad("background", "finite destination RGB required"));
        }
        match (policy, lut.is_some(), float) {
            (SourceColorPolicy::DocumentEncoded, false, false)
            | (SourceColorPolicy::DisplayLinear, false, true)
            | (SourceColorPolicy::LutInput, true, _) => {}
            _ => return Err(bad("source color", "policy does not match target or LUT")),
        }
        if let Some(lut) = lut
            && (lut.size != 33
                || lut.values.len() != 33 * 33 * 33
                || lut.values.iter().flatten().any(|v| !v.is_finite()))
        {
            return Err(bad("output LUT", "33^3 finite RGB nodes required"));
        }
        let bg = background.unwrap_or([0.0; 3]);
        let params = [
            extent.width,
            src.x0 as u32,
            src.y0 as u32,
            dst.0,
            dst.1,
            w,
            h,
            u32::from(background.is_some()),
            bg[0].to_bits(),
            bg[1].to_bits(),
            bg[2].to_bits(),
            u32::from(lut.is_some()),
        ];
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let ub = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("output parameters"),
            contents: bytemuck::cast_slice(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let nodes = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("output LUT"),
            contents: bytemuck::cast_slice(lut.map_or(&[[0.0; 3]][..], |l| l.values.as_slice())),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let view = target.create_view(&Default::default());
        let pipeline = if float { &self.edr } else { &self.sdr };
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("resident output"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: source.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: ub.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: nodes.as_entire_binding(),
                },
            ],
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(w.div_ceil(16), h.div_ceil(16), 1);
        }
        queue.submit([encoder.finish()]);
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(EngineError::internal(e.to_string()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::util::DeviceExt;

    fn gpu() -> gpu_core::GpuDevice {
        gpu_core::GpuDevice::new().expect("Metal device required for output tests")
    }
    fn texture(d: &wgpu::Device, format: wgpu::TextureFormat) -> wgpu::Texture {
        d.create_texture(&wgpu::TextureDescriptor {
            label: Some("output test"),
            size: wgpu::Extent3d {
                width: 3,
                height: 2,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
    }
    fn source(d: &wgpu::Device, pixels: &[[f32; 4]]) -> wgpu::Buffer {
        d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(pixels),
            usage: wgpu::BufferUsages::STORAGE,
        })
    }
    fn read(g: &gpu_core::GpuDevice, t: &wgpu::Texture) -> Vec<[f32; 4]> {
        let b = g.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 512,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let mut e = g.device.create_command_encoder(&Default::default());
        e.copy_texture_to_buffer(
            t.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &b,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: Some(2),
                },
            },
            t.size(),
        );
        g.queue.submit([e.finish()]);
        let bytes = gpu_core::read_buffer(&g.device, &g.queue, &b, 0, 512).unwrap();
        let float = t.format() == wgpu::TextureFormat::Rgba16Float;
        (0..6)
            .map(|i| {
                let offset = i / 3 * 256 + i % 3 * if float { 8 } else { 4 };
                std::array::from_fn(|c| {
                    if float {
                        half::f16::from_bits(u16::from_le_bytes([
                            bytes[offset + c * 2],
                            bytes[offset + c * 2 + 1],
                        ]))
                        .to_f32()
                    } else {
                        f32::from(bytes[offset + c]) / 255.0
                    }
                })
            })
            .collect()
    }
    #[test]
    fn lut_transforms_straight_rgb_before_premultiplication_and_flattening() {
        let g = gpu();
        let out = OutputPresenter::new(&g.device).unwrap();
        let mut lut = Lut3d {
            size: 33,
            values: Vec::new(),
        };
        for b in 0..33 {
            for green in 0..33 {
                for r in 0..33 {
                    let (r, green, b) = (r as f32 / 32.0, green as f32 / 32.0, b as f32 / 32.0);
                    lut.values.push([b * b * 3.0, r * r, green * 0.5]);
                }
            }
        }
        let rgb = [0.37, 0.43, 0.71];
        let expected = lut.sample(rgb);
        let src = source(
            &g.device,
            &[
                [rgb[0] * 0.5, rgb[1] * 0.5, rgb[2] * 0.5, 0.5],
                [8.0, 4.0, 2.0, 0.0],
            ],
        );
        for format in [
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureFormat::Rgba16Float,
        ] {
            let t = texture(&g.device, format);
            for bg in [None, Some([0.1, 0.2, 0.3])] {
                out.present(
                    &g.device,
                    &g.queue,
                    &src,
                    Extent::new(2, 1),
                    Rect::new(0, 0, 2, 1),
                    &t,
                    (1, 1),
                    bg,
                    SourceColorPolicy::LutInput,
                    Some(&lut),
                )
                .unwrap();
                let pixels = read(&g, &t);
                for c in 0..3 {
                    let want = expected[c] * 0.5 + bg.map_or(0.0, |v| v[c] * 0.5);
                    assert!(
                        (pixels[4][c] - want).abs() < 0.005,
                        "{format:?} channel {c}: {:?} expected {want}",
                        pixels[4]
                    );
                    assert!((pixels[5][c] - bg.map_or(0.0, |v| v[c])).abs() < 0.005);
                }
                assert!((pixels[4][3] - if bg.is_some() { 1.0 } else { 0.5 }).abs() < 0.005);
            }
        }
    }

    #[test]
    fn validates_target_bounds_source_policy_and_lut() {
        let g = gpu();
        let out = OutputPresenter::new(&g.device).unwrap();
        let src = source(&g.device, &[[0.25, 0.125, 0.0, 0.5]]);
        let sdr = texture(&g.device, wgpu::TextureFormat::Rgba8Unorm);
        let call = |buffer: &wgpu::Buffer,
                    target: &wgpu::Texture,
                    rect,
                    dst,
                    bg,
                    policy,
                    lut: Option<&Lut3d>| {
            out.present(
                &g.device,
                &g.queue,
                buffer,
                Extent::new(1, 1),
                rect,
                target,
                dst,
                bg,
                policy,
                lut,
            )
        };
        let rect = Rect::new(0, 0, 1, 1);
        let plain = SourceColorPolicy::DocumentEncoded;
        call(&src, &sdr, rect, (0, 0), None, plain, None).unwrap();
        let px = read(&g, &sdr)[0];
        assert!((px[0] - 0.25).abs() < 0.005);
        assert!((px[3] - 0.5).abs() < 0.005);
        for r in [
            Rect::new(-1, 0, 1, 1),
            Rect::new(0, 0, 2, 1),
            Rect::new(0, 0, 0, 1),
            Rect::new(i64::MIN, 0, i64::MAX, 1),
        ] {
            assert!(call(&src, &sdr, r, (0, 0), None, plain, None).is_err());
        }
        for dst in [(3, 0), (0, 2), (u32::MAX, 0)] {
            assert!(call(&src, &sdr, rect, dst, None, plain, None).is_err());
        }
        assert!(
            call(
                &src,
                &sdr,
                rect,
                (0, 0),
                Some([f32::NAN, 0.0, 0.0]),
                plain,
                None
            )
            .is_err()
        );
        assert!(
            call(
                &src,
                &sdr,
                rect,
                (0, 0),
                None,
                SourceColorPolicy::DisplayLinear,
                None
            )
            .is_err()
        );
        assert!(
            call(
                &src,
                &sdr,
                rect,
                (0, 0),
                None,
                SourceColorPolicy::LutInput,
                None
            )
            .is_err()
        );
        let edr = texture(&g.device, wgpu::TextureFormat::Rgba16Float);
        assert!(call(&src, &edr, rect, (0, 0), None, plain, None).is_err());
        let wrong_size = source(&g.device, &[[0.0; 4]; 2]);
        assert!(call(&wrong_size, &sdr, rect, (0, 0), None, plain, None).is_err());
        let wrong_usage = g.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 16,
            usage: wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        assert!(call(&wrong_usage, &sdr, rect, (0, 0), None, plain, None).is_err());
        for lut in [
            Lut3d {
                size: 2,
                values: vec![[0.0; 3]; 8],
            },
            Lut3d {
                size: 33,
                values: vec![],
            },
            Lut3d {
                size: 33,
                values: vec![[f32::NAN; 3]; 33 * 33 * 33],
            },
        ] {
            assert!(
                call(
                    &src,
                    &sdr,
                    rect,
                    (0, 0),
                    None,
                    SourceColorPolicy::LutInput,
                    Some(&lut)
                )
                .is_err()
            );
            assert!(call(&src, &sdr, rect, (0, 0), None, plain, Some(&lut)).is_err());
        }
        for kind in 0..5 {
            let t = g.device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: 4,
                    height: 4,
                    depth_or_array_layers: if kind == 1 || kind == 2 { 2 } else { 1 },
                },
                dimension: if kind == 2 {
                    wgpu::TextureDimension::D3
                } else {
                    wgpu::TextureDimension::D2
                },
                mip_level_count: if kind == 3 { 2 } else { 1 },
                sample_count: 1,
                format: if kind == 4 {
                    wgpu::TextureFormat::Rgba8Uint
                } else {
                    wgpu::TextureFormat::Rgba8Unorm
                },
                usage: if kind == 0 {
                    wgpu::TextureUsages::COPY_SRC
                } else {
                    wgpu::TextureUsages::STORAGE_BINDING
                },
                view_formats: &[],
            });
            assert!(
                call(&src, &t, rect, (0, 0), None, plain, None).is_err(),
                "target kind {kind}"
            );
        }
    }

    #[test]
    fn lut_clamps_input_but_preserves_extended_output() {
        let g = gpu();
        let out = OutputPresenter::new(&g.device).unwrap();
        let mut lut = Lut3d {
            size: 33,
            values: Vec::new(),
        };
        for b in 0..33 {
            for green in 0..33 {
                for r in 0..33 {
                    lut.values
                        .push([r as f32 / 8.0, green as f32 / 32.0 - 0.5, b as f32 / 32.0]);
                }
            }
        }
        let src = source(&g.device, &[[2.0, -1.0, 0.5, 1.0]]);
        let t = texture(&g.device, wgpu::TextureFormat::Rgba16Float);
        out.present(
            &g.device,
            &g.queue,
            &src,
            Extent::new(1, 1),
            Rect::new(0, 0, 1, 1),
            &t,
            (0, 0),
            None,
            SourceColorPolicy::LutInput,
            Some(&lut),
        )
        .unwrap();
        assert_eq!(read(&g, &t)[0], [4.0, -0.5, 0.5, 1.0]);
    }

    #[test]
    fn edr_preserves_extended_values_alpha_and_offsets() {
        let g = gpu();
        let out = OutputPresenter::new(&g.device).unwrap();
        let src = source(&g.device, &[[0.0; 4], [1.5, -0.25, 0.125, 0.5]]);
        let t = texture(&g.device, wgpu::TextureFormat::Rgba16Float);
        out.present(
            &g.device,
            &g.queue,
            &src,
            Extent::new(2, 1),
            Rect::new(1, 0, 2, 1),
            &t,
            (2, 1),
            None,
            SourceColorPolicy::DisplayLinear,
            None,
        )
        .unwrap();
        let pixels = read(&g, &t);
        assert_eq!(pixels[5], [1.5, -0.25, 0.125, 0.5]);
        assert_eq!(pixels[0], [0.0; 4]);
    }
}
