//! Resident premultiplied RGBA presentation without pixel readback.

use crate::geom::Rect;
use engine_api::{EngineError, EngineResult, tile::Extent};
use gpu_core::{
    Lut3d,
    color_mgmt::{Builtin, Profile, Registry, Transform, TransformOptions},
};
use std::sync::Arc;
use std::{collections::HashMap, sync::Mutex};
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

/// Source contract for profiled presentation. This is explicit because a bounded
/// ICC LUT cannot preserve arbitrary scene-linear HDR input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceDomain {
    /// Straight RGB is document-encoded in [0,1]. Not an HDR source path.
    EncodedUnit,
    /// Samples are already linear in the document profile's primaries/white;
    /// its TRCs are intentionally ignored. Matrix RGB profiles only, relative
    /// colorimetric intent, float destination. No LUT input clamping.
    LinearExtended,
}
#[derive(Clone)]
/// Destination encoding and storage format.
pub enum DisplayDestination {
    /// ICC-encoded RGBA8; host must tag the surface with this profile.
    Encoded(Arc<Profile>),
    /// Linear extended sRGB RGBA16F (no OETF).
    LinearSrgb,
    /// Linear extended Display P3 RGBA16F (no OETF).
    LinearDisplayP3,
}
/// Output range, not an exposure gain or a scene tone mapper.
#[derive(Debug, Clone, Copy, Default)]
pub struct Headroom {
    /// Enable extended output range.
    pub hdr: bool,
    /// Requested headroom stops, sanitized to 0..16.
    pub stops: f32,
    /// Display headroom as a linear multiple of SDR white.
    pub display: f32,
}
impl Headroom {
    /// min(2^stops, display) for HDR; 1 for SDR. Limited to finite f16 range.
    pub fn effective(self) -> f32 {
        let stops = if self.stops.is_finite() {
            self.stops.clamp(0.0, 16.0)
        } else {
            0.0
        };
        let display = if self.display.is_finite() {
            self.display.max(1.0)
        } else {
            1.0
        };
        if self.hdr {
            stops.exp2().min(display).min(65504.0)
        } else {
            1.0
        }
    }
}
/// Immutable device-resident transform. Retaining this avoids even CPU LUT
/// hashing on redraws. It must be used on the presenter's device.
#[derive(Clone)]
pub struct PreparedOutput {
    nodes: wgpu::Buffer,
    float: bool,
    headroom: f32,
    extended: bool,
}
#[derive(PartialEq, Eq, Hash)]
struct ProfileKey {
    source: [u8; 32],
    destination: [u8; 32],
    domain: SourceDomain,
    intent: u32,
    bpc: bool,
    paper: bool,
    threshold: u32,
}
#[derive(Default)]
struct Profiles {
    registry: Registry,
    buffers: HashMap<ProfileKey, wgpu::Buffer>,
}
/// Counters are shared by renderers using the same pipeline instance.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OutputCacheStats {
    /// GPU LUT buffer allocations/uploads.
    pub uploads: u64,
    /// Distinct ICC/profile/options preparations.
    pub preparations: u64,
    /// Raw content-cache hits (profile-cache hits do not hash content).
    pub hits: u64,
    /// GPU LUT bytes retained for the pipeline lifetime.
    pub resident_bytes: u64,
}
#[derive(Default)]
struct LutCache {
    buffers: HashMap<blake3::Hash, wgpu::Buffer>,
    stats: OutputCacheStats,
}

pub struct OutputPresenter {
    profiles: Mutex<Profiles>,
    cache: Mutex<LutCache>,
    dummy: wgpu::Buffer,
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
            profiles: Mutex::new(Profiles::default()),
            cache: Mutex::new(LutCache::default()),
            dummy: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("output dummy LUT"),
                contents: &[0; 12],
                usage: wgpu::BufferUsages::STORAGE,
            }),
            sdr: build("rgba8unorm"),
            edr: build("rgba16float"),
        };
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(EngineError::internal(e.to_string()));
        }
        Ok(result)
    }

    /// None is untagged sRGB. Handle-only profiles are deliberately rejected:
    /// no guessed interpretation of an unresolved document profile.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &self,
        device: &wgpu::Device,
        profile: Option<&crate::document::ColorProfile>,
        domain: SourceDomain,
        destination: DisplayDestination,
        options: TransformOptions,
        headroom: Headroom,
    ) -> EngineResult<PreparedOutput> {
        let error = |e: gpu_core::color_mgmt::Error| EngineError::invalid("profile", e.to_string());
        let mut profiles = self.profiles.lock().unwrap_or_else(|e| e.into_inner());
        let registry = &mut profiles.registry;
        let source = match profile {
            None => registry.builtin(Builtin::Srgb).map_err(error)?,
            Some(p) => registry
                .load_bytes(p.icc.as_deref().ok_or_else(|| {
                    EngineError::invalid("profile", "embedded ICC bytes required")
                })?)
                .map_err(error)?,
        };
        let extended = domain == SourceDomain::LinearExtended;
        if extended
            && (matches!(destination, DisplayDestination::Encoded(_))
                || options.intent != gpu_core::color_mgmt::Intent::RelativeColorimetric)
        {
            return Err(EngineError::invalid(
                "source domain",
                "linear extended requires linear destination and relative colorimetric intent",
            ));
        }
        let source = if extended {
            registry
                .linearized_rgb(&source)
                .map_err(error)?
                .ok_or_else(|| {
                    EngineError::invalid(
                        "source domain",
                        "linear extended requires matrix RGB profile",
                    )
                })?
        } else {
            source
        };
        let float = !matches!(destination, DisplayDestination::Encoded(_));
        let destination = match destination {
            DisplayDestination::Encoded(p) => p,
            destination => {
                let p = registry
                    .builtin(if matches!(destination, DisplayDestination::LinearSrgb) {
                        Builtin::Srgb
                    } else {
                        Builtin::DisplayP3
                    })
                    .map_err(error)?;
                registry
                    .linearized_rgb(&p)
                    .map_err(error)?
                    .ok_or_else(|| EngineError::invalid("display", "matrix RGB profile required"))?
            }
        };
        if !options.gamut_threshold.is_finite() || options.gamut_threshold < 0.0 {
            return Err(EngineError::invalid(
                "options",
                "finite nonnegative gamut threshold required",
            ));
        }
        let key = ProfileKey {
            source: source.digest(),
            destination: destination.digest(),
            domain,
            intent: options.intent as u32,
            bpc: options.black_point_compensation,
            paper: options.simulate_paper,
            threshold: options.gamut_threshold.to_bits(),
        };
        let nodes = if let Some(nodes) = profiles.buffers.get(&key) {
            nodes.clone()
        } else {
            let lut = Transform::new(&source, &destination, options)
                .map_err(error)?
                .lut33();
            let nodes = self.nodes(device, Some(&lut));
            self.cache
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .stats
                .preparations += 1;
            profiles.buffers.insert(key, nodes.clone());
            nodes
        };
        Ok(PreparedOutput {
            nodes,
            float,
            headroom: headroom.effective(),
            extended,
        })
    }

    pub fn cache_stats(&self) -> OutputCacheStats {
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).stats
    }

    fn nodes(&self, device: &wgpu::Device, lut: Option<&Lut3d>) -> wgpu::Buffer {
        let Some(lut) = lut else {
            return self.dummy.clone();
        };
        // Content identity, not address: the public Lut3d is mutable.
        let bytes = bytemuck::cast_slice(lut.values.as_slice());
        let key = blake3::hash(bytes);
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(buffer) = cache.buffers.get(&key).cloned() {
            cache.stats.hits += 1;
            return buffer;
        }
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("resident output LUT"),
            contents: bytes,
            usage: wgpu::BufferUsages::STORAGE,
        });
        cache.stats.uploads += 1;
        cache.stats.resident_bytes += bytes.len() as u64;
        cache.buffers.insert(key, buffer.clone());
        buffer
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
        self.dispatch(
            device, queue, source, extent, src, target, dst, background, policy, lut, None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn present_prepared(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: &wgpu::Buffer,
        extent: Extent,
        src: Rect,
        target: &wgpu::Texture,
        dst: (u32, u32),
        background: Option<[f32; 3]>,
        prepared: &PreparedOutput,
    ) -> EngineResult<()> {
        self.dispatch(
            device,
            queue,
            source,
            extent,
            src,
            target,
            dst,
            background,
            SourceColorPolicy::LutInput,
            None,
            Some(prepared),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn dispatch(
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
        prepared: Option<&PreparedOutput>,
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
        if prepared.is_some_and(|p| p.float != float) {
            return Err(bad(
                "target",
                "prepared transform does not match texture format",
            ));
        }
        match (policy, lut.is_some() || prepared.is_some(), float) {
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
            u32::from(lut.is_some() || prepared.is_some()),
            prepared
                .filter(|p| p.float)
                .map_or(0.0, |p| p.headroom)
                .to_bits(),
            u32::from(prepared.is_some_and(|p| p.extended)),
            0,
            0,
        ];
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let ub = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("output parameters"),
            contents: bytemuck::cast_slice(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let nodes = prepared.map_or_else(|| self.nodes(device, lut), |p| p.nodes.clone());
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
    fn profiled_output_matches_cpu_and_caches_preparation() {
        use gpu_core::color_mgmt::{Builtin, Registry, Transform, TransformOptions};
        let g = gpu();
        let out = OutputPresenter::new(&g.device).unwrap();
        let mut registry = Registry::new();
        let source_profile = registry.builtin(Builtin::DisplayP3).unwrap();
        let dest = registry.builtin(Builtin::Srgb).unwrap();
        let options = TransformOptions::default();
        let cpu = Transform::new(&source_profile, &dest, options).unwrap();
        let profile =
            crate::document::ColorProfile::from_icc("P3", source_profile.icc_bytes().to_vec());
        let rgb = [0.47, 0.61, 0.33];
        let src = source(
            &g.device,
            &[[rgb[0] * 0.5, rgb[1] * 0.5, rgb[2] * 0.5, 0.5]],
        );
        let target = texture(&g.device, wgpu::TextureFormat::Rgba8Unorm);
        for _ in 0..3 {
            let prepared = out
                .prepare(
                    &g.device,
                    Some(&profile),
                    SourceDomain::EncodedUnit,
                    DisplayDestination::Encoded(dest.clone()),
                    options,
                    Headroom::default(),
                )
                .unwrap();
            out.present_prepared(
                &g.device,
                &g.queue,
                &src,
                Extent::new(1, 1),
                Rect::new(0, 0, 1, 1),
                &target,
                (1, 1),
                None,
                &prepared,
            )
            .unwrap();
            let actual = read(&g, &target)[4];
            for (c, value) in actual.iter().take(3).enumerate() {
                assert!((*value - cpu.apply(rgb)[c] * 0.5).abs() < 0.005);
            }
        }
        assert_eq!(out.cache_stats().uploads, 1);
        assert_eq!(out.cache_stats().preparations, 1);
    }

    #[test]
    fn linear_profiled_hdr_preserves_highlights_and_caps_headroom() {
        let g = gpu();
        let out = OutputPresenter::new(&g.device).unwrap();
        let mut registry = Registry::new();
        let sp = registry.builtin(Builtin::Rec2020).unwrap();
        let profile = crate::document::ColorProfile::from_icc("2020", sp.icc_bytes().to_vec());
        let linear = registry.linearized_rgb(&sp).unwrap().unwrap();
        let rgb = [3.2, 1.8, 0.7];
        let src = source(
            &g.device,
            &[
                [rgb[0] * 0.5, rgb[1] * 0.5, rgb[2] * 0.5, 0.5],
                [4.0, 3.0, 2.0, 0.0],
            ],
        );
        let t = texture(&g.device, wgpu::TextureFormat::Rgba16Float);
        for (destination, builtin) in [
            (DisplayDestination::LinearSrgb, Builtin::Srgb),
            (DisplayDestination::LinearDisplayP3, Builtin::DisplayP3),
        ] {
            let dp = registry.builtin(builtin).unwrap();
            let dp = registry.linearized_rgb(&dp).unwrap().unwrap();
            let cpu = Transform::new(&linear, &dp, TransformOptions::default()).unwrap();
            for hdr in [false, true] {
                let h = Headroom {
                    hdr,
                    stops: 2.0,
                    display: 2.5,
                };
                let prepared = out
                    .prepare(
                        &g.device,
                        Some(&profile),
                        SourceDomain::LinearExtended,
                        destination.clone(),
                        TransformOptions::default(),
                        h,
                    )
                    .unwrap();
                for bg in [None, Some([0.25, 0.5, 0.75])] {
                    out.present_prepared(
                        &g.device,
                        &g.queue,
                        &src,
                        Extent::new(2, 1),
                        Rect::new(0, 0, 2, 1),
                        &t,
                        (1, 1),
                        bg,
                        &prepared,
                    )
                    .unwrap();
                    let px = read(&g, &t);
                    for c in 0..3 {
                        let expected = cpu.apply(rgb)[c].clamp(0.0, h.effective()) * 0.5
                            + bg.map_or(0.0, |b| b[c] * 0.5);
                        assert!(
                            (px[4][c] - expected).abs() < 0.003,
                            "{builtin:?} {hdr} {c}: {} != {expected}",
                            px[4][c]
                        );
                        assert!((px[5][c] - bg.map_or(0.0, |b| b[c])).abs() < 0.001);
                    }
                }
            }
        }
        assert_eq!(out.cache_stats().preparations, 2);
        assert_eq!(
            Headroom {
                hdr: true,
                stops: f32::NAN,
                display: f32::INFINITY
            }
            .effective(),
            1.0
        );
    }

    #[test]
    fn profiles_reject_unresolved_invalid_and_unsupported_contracts() {
        let g = gpu();
        let out = OutputPresenter::new(&g.device).unwrap();
        let mut r = Registry::new();
        let srgb = r.builtin(Builtin::Srgb).unwrap();
        let mut profile =
            crate::document::ColorProfile::from_icc("sRGB", srgb.icc_bytes().to_vec());
        profile.icc = None;
        assert!(
            out.prepare(
                &g.device,
                Some(&profile),
                SourceDomain::EncodedUnit,
                DisplayDestination::LinearSrgb,
                TransformOptions::default(),
                Headroom::default()
            )
            .is_err()
        );
        profile.icc = Some(Arc::new(vec![1, 2, 3]));
        assert!(
            out.prepare(
                &g.device,
                Some(&profile),
                SourceDomain::EncodedUnit,
                DisplayDestination::LinearSrgb,
                TransformOptions::default(),
                Headroom::default()
            )
            .is_err()
        );
        assert!(
            out.prepare(
                &g.device,
                None,
                SourceDomain::LinearExtended,
                DisplayDestination::Encoded(srgb),
                TransformOptions::default(),
                Headroom::default()
            )
            .is_err()
        );
        let options = TransformOptions {
            intent: gpu_core::color_mgmt::Intent::Perceptual,
            ..Default::default()
        };
        assert!(
            out.prepare(
                &g.device,
                None,
                SourceDomain::LinearExtended,
                DisplayDestination::LinearSrgb,
                options,
                Headroom::default()
            )
            .is_err()
        );
        let prepared = out
            .prepare(
                &g.device,
                None,
                SourceDomain::EncodedUnit,
                DisplayDestination::LinearSrgb,
                TransformOptions::default(),
                Headroom::default(),
            )
            .unwrap();
        let src = source(&g.device, &[[0.5, 0.5, 0.5, 1.0]]);
        let target = texture(&g.device, wgpu::TextureFormat::Rgba8Unorm);
        assert!(
            out.present_prepared(
                &g.device,
                &g.queue,
                &src,
                Extent::new(1, 1),
                Rect::new(0, 0, 1, 1),
                &target,
                (0, 0),
                None,
                &prepared
            )
            .is_err()
        );
    }

    #[test]
    fn encoded_profiles_to_linear_displays_match_lcms_grid() {
        let g = gpu();
        let out = OutputPresenter::new(&g.device).unwrap();
        let mut registry = Registry::new();
        let target = texture(&g.device, wgpu::TextureFormat::Rgba16Float);
        for builtin in [
            Builtin::Srgb,
            Builtin::DisplayP3,
            Builtin::AdobeRgb,
            Builtin::ProPhoto,
        ] {
            let source_profile = registry.builtin(builtin).unwrap();
            let profile = crate::document::ColorProfile::from_icc(
                "test",
                source_profile.icc_bytes().to_vec(),
            );
            for (destination, db) in [
                (DisplayDestination::LinearSrgb, Builtin::Srgb),
                (DisplayDestination::LinearDisplayP3, Builtin::DisplayP3),
            ] {
                let dest = registry.builtin(db).unwrap();
                let dest = registry.linearized_rgb(&dest).unwrap().unwrap();
                let cpu =
                    Transform::new(&source_profile, &dest, TransformOptions::default()).unwrap();
                let prepared = out
                    .prepare(
                        &g.device,
                        Some(&profile),
                        SourceDomain::EncodedUnit,
                        destination,
                        TransformOptions::default(),
                        Headroom {
                            hdr: true,
                            stops: 2.0,
                            display: 3.0,
                        },
                    )
                    .unwrap();
                for rgb in [
                    [0.0, 0.0, 0.0],
                    [1.0, 1.0, 1.0],
                    [1.0, 0.0, 0.0],
                    [0.13, 0.47, 0.79],
                    [0.031, 0.042, 0.052],
                ] {
                    let src = source(&g.device, &[[rgb[0], rgb[1], rgb[2], 1.0]]);
                    out.present_prepared(
                        &g.device,
                        &g.queue,
                        &src,
                        Extent::new(1, 1),
                        Rect::new(0, 0, 1, 1),
                        &target,
                        (0, 0),
                        None,
                        &prepared,
                    )
                    .unwrap();
                    let actual = read(&g, &target)[0];
                    for c in 0..3 {
                        assert!(
                            (actual[c] - cpu.apply(rgb)[c].clamp(0.0, 3.0)).abs() < 0.004,
                            "{builtin:?}->{db:?} {rgb:?}: {actual:?} != {:?}",
                            cpu.apply(rgb)
                        );
                    }
                }
            }
        }
        let count = out.cache_stats().preparations;
        let opts = TransformOptions {
            intent: gpu_core::color_mgmt::Intent::Perceptual,
            ..Default::default()
        };
        out.prepare(
            &g.device,
            None,
            SourceDomain::EncodedUnit,
            DisplayDestination::LinearSrgb,
            opts,
            Headroom::default(),
        )
        .unwrap();
        assert_eq!(out.cache_stats().preparations, count + 1);
    }

    #[test]
    #[ignore = "printing before/after resident presentation benchmark; run explicitly on Metal"]
    fn benchmark_present_before_after_resident_lut() {
        let g = gpu();
        let out = OutputPresenter::new(&g.device).unwrap();
        let mut registry = Registry::new();
        let source_profile = registry.builtin(Builtin::Srgb).unwrap();
        let destination = registry.builtin(Builtin::DisplayP3).unwrap();
        let lut = Transform::new(&source_profile, &destination, TransformOptions::default())
            .unwrap()
            .lut33();
        let extent = Extent::new(1920, 1080);
        let rect = Rect::of_extent(extent);
        let src = source(&g.device, &vec![[0.25, 0.3, 0.4, 0.5]; 1920 * 1080]);
        let t = g.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 1920,
                height: 1080,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING,
            view_formats: &[],
        });
        let n = 60;
        // Same pipeline/dispatch: force the old upload-every-call behavior,
        // excluding shader compilation, LUT generation and pixel uploads.
        out.present(
            &g.device,
            &g.queue,
            &src,
            extent,
            rect,
            &t,
            (0, 0),
            None,
            SourceColorPolicy::LutInput,
            Some(&lut),
        )
        .unwrap();
        g.wait().unwrap();
        let before = std::time::Instant::now();
        for _ in 0..n {
            let mut cache = out.cache.lock().unwrap();
            cache.buffers.clear();
            cache.stats.resident_bytes = 0;
            drop(cache);
            out.present(
                &g.device,
                &g.queue,
                &src,
                extent,
                rect,
                &t,
                (0, 0),
                None,
                SourceColorPolicy::LutInput,
                Some(&lut),
            )
            .unwrap();
            g.wait().unwrap();
        }
        let before = before.elapsed();
        let prepared = out
            .prepare(
                &g.device,
                None,
                SourceDomain::EncodedUnit,
                DisplayDestination::Encoded(destination),
                TransformOptions::default(),
                Headroom::default(),
            )
            .unwrap();
        let uploads = out.cache_stats().uploads;
        let after = std::time::Instant::now();
        for _ in 0..n {
            out.present_prepared(
                &g.device,
                &g.queue,
                &src,
                extent,
                rect,
                &t,
                (0, 0),
                None,
                &prepared,
            )
            .unwrap();
            g.wait().unwrap();
        }
        let after = after.elapsed();
        assert_eq!(out.cache_stats().uploads, uploads);
        println!(
            "M5-16 {:?}: 1920x1080, {n} presents, wait/frame, before upload-every-frame {:.3} ms/frame; after resident {:.3} ms/frame; redraw LUT uploads=0, cache={:?}",
            g.adapter_info.name,
            before.as_secs_f64() * 1000.0 / n as f64,
            after.as_secs_f64() * 1000.0 / n as f64,
            out.cache_stats()
        );
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
        assert_eq!(out.cache_stats().uploads, 1);
        for _ in 0..2 {
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
        }
        assert_eq!(out.cache_stats().uploads, 1);
        for node in &mut lut.values {
            node[0] = 2.0;
        }
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
        assert_eq!(out.cache_stats().uploads, 2);
        assert_eq!(read(&g, &t)[0], [2.0, -0.5, 0.5, 1.0]);
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
