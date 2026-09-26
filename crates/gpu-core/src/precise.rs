//! Compute pipelines with IEEE-754 conformant f32 maths.
//!
//! Metal compiles shaders with fast math by default: division and
//! reciprocal may be approximate, and the compiler may reassociate and
//! contract (`a·b + c` → `fma`). Compositing chains with threshold modes
//! (Hard Mix, Divide, Darker/Lighter Colour) turn a one-ulp difference into
//! a 0 ↔ 1 jump, so the compositor's reference comparisons need the GPU to
//! round like the CPU. [`precise_compute_pipeline`] translates WGSL to MSL
//! with naga (no runtime bounds checks, no loop-bounding counters, no
//! workgroup zero-initialisation), prepends
//! `#pragma METAL fp math_mode(safe)` and `#pragma METAL fp contract(off)`,
//! and creates the module through wgpu's MSL passthrough. Without
//! [`wgpu::Features::PASSTHROUGH_SHADERS`] it falls back to the ordinary
//! WGSL path with runtime checks disabled ([`Precision::Relaxed`]).
#![allow(unsafe_code)]

use std::borrow::Cow;

use engine_api::{EngineError, EngineResult};
use wgpu::naga;

/// How a pipeline from [`precise_compute_pipeline`] was compiled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Precision {
    /// MSL passthrough with `math_mode(safe)` and `contract(off)`.
    Ieee,
    /// The backend's default (fast-math) WGSL compilation.
    Relaxed,
}

/// A compute pipeline and the explicit layout it was created with.
#[derive(Debug, Clone)]
pub struct PrecisePipeline {
    /// The pipeline.
    pub pipeline: wgpu::ComputePipeline,
    /// Its group-0 layout (bind groups must use this layout).
    pub layout: wgpu::BindGroupLayout,
    /// How the maths was compiled.
    pub precision: Precision,
}

fn gpu(message: impl std::fmt::Display) -> EngineError {
    EngineError::Gpu {
        message: message.to_string(),
    }
}

/// Compiles `wgsl`'s compute entry point `entry` against one bind group
/// (group 0) described by `entries`. `workgroup` must equal the shader's
/// `@workgroup_size`. Every WGSL binding must appear in `entries`; entries
/// may name bindings the shader does not use.
///
/// The caller is responsible for in-bounds indexing: the translated shader
/// has no runtime bounds checks.
pub fn precise_compute_pipeline(
    device: &wgpu::Device,
    label: &str,
    wgsl: &str,
    entry: &str,
    workgroup: (u32, u32, u32),
    entries: &[wgpu::BindGroupLayoutEntry],
) -> EngineResult<PrecisePipeline> {
    let mut entries = entries.to_vec();
    entries.sort_by_key(|e| e.binding);
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
        entries: &entries,
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let passthrough = device
        .features()
        .contains(wgpu::Features::PASSTHROUGH_SHADERS)
        && cfg!(target_os = "macos");
    let (module, entry_name, precision) = if passthrough {
        let (msl, name) = translate(wgsl, entry, &entries)?;
        let desc = wgpu::ShaderModuleDescriptorPassthrough {
            label: Some(label),
            entry_points: Cow::Owned(vec![wgpu::PassthroughShaderEntryPoint {
                name: Cow::Owned(name.clone()),
                workgroup_size: workgroup,
            }]),
            msl: Some(Cow::Owned(msl)),
            ..Default::default()
        };
        // SAFETY: the MSL is naga's translation of WGSL that naga
        // validated, bound to the buffer indices wgpu-hal's Metal backend
        // assigns for this exact layout (see `translate`). Threadgroup
        // memory is declared inside the kernel, so no lengths are needed.
        let module = unsafe { device.create_shader_module_passthrough(desc) };
        (module, name, Precision::Ieee)
    } else {
        // SAFETY: callers guarantee in-bounds indexing and bounded loops
        // (documented on this function).
        let module = unsafe {
            device.create_shader_module_trusted(
                wgpu::ShaderModuleDescriptor {
                    label: Some(label),
                    source: wgpu::ShaderSource::Wgsl(wgsl.into()),
                },
                wgpu::ShaderRuntimeChecks::unchecked(),
            )
        };
        (module, entry.to_string(), Precision::Relaxed)
    };
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(label),
        layout: Some(&pipeline_layout),
        module: &module,
        entry_point: Some(&entry_name),
        compilation_options: Default::default(),
        cache: None,
    });
    Ok(PrecisePipeline {
        pipeline,
        layout,
        precision,
    })
}

/// WGSL → MSL for one compute entry point, bound the way wgpu-hal's Metal
/// backend binds `entries` (sorted by binding; buffers, textures and
/// samplers each numbered in order). Returns the source and the MSL entry
/// point name.
pub fn translate(
    wgsl: &str,
    entry: &str,
    entries: &[wgpu::BindGroupLayoutEntry],
) -> EngineResult<(String, String)> {
    use naga::back::msl;
    let module = naga::front::wgsl::parse_str(wgsl).map_err(|e| gpu(e.emit_to_string(wgsl)))?;
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        msl::supported_capabilities(),
    )
    .validate(&module)
    .map_err(|e| gpu(e.emit_to_string(wgsl)))?;
    let mut resources = msl::BindingMap::default();
    let (mut buffers, mut textures, mut samplers) = (0u8, 0u8, 0u8);
    let mut sorted = entries.to_vec();
    sorted.sort_by_key(|e| e.binding);
    for e in &sorted {
        let mut target = msl::BindTarget::default();
        match e.ty {
            wgpu::BindingType::Buffer { ty, .. } => {
                target.buffer = Some(buffers);
                buffers += 1;
                if let wgpu::BufferBindingType::Storage { read_only } = ty {
                    target.mutable = !read_only;
                }
            }
            wgpu::BindingType::StorageTexture { access, .. } => {
                target.texture = Some(textures);
                textures += 1;
                target.mutable = access != wgpu::StorageTextureAccess::ReadOnly;
            }
            wgpu::BindingType::Texture { .. } => {
                target.texture = Some(textures);
                textures += 1;
            }
            wgpu::BindingType::Sampler(_) => {
                target.sampler = Some(msl::BindSamplerTarget::Resource(samplers));
                samplers += 1;
            }
            _ => return Err(gpu("precise pipeline: unsupported binding type")),
        }
        resources.insert(
            naga::ResourceBinding {
                group: 0,
                binding: e.binding,
            },
            target,
        );
    }
    let mut per_entry_point_map = msl::EntryPointResourceMap::default();
    per_entry_point_map.insert(
        entry.to_string(),
        msl::EntryPointResources {
            resources,
            immediates_buffer: None,
            // Required by naga for runtime-sized arrays; never read without
            // bounds checks (asserted below).
            sizes_buffer: Some(buffers),
        },
    );
    let options = msl::Options {
        lang_version: (3, 0),
        per_entry_point_map,
        fake_missing_bindings: false,
        bounds_check_policies: naga::proc::BoundsCheckPolicies {
            index: naga::proc::BoundsCheckPolicy::Unchecked,
            buffer: naga::proc::BoundsCheckPolicy::Unchecked,
            image_load: naga::proc::BoundsCheckPolicy::Unchecked,
            binding_array: naga::proc::BoundsCheckPolicy::Unchecked,
        },
        zero_initialize_workgroup_memory: false,
        force_loop_bounding: false,
        ..Default::default()
    };
    let pipeline = msl::PipelineOptions {
        entry_point: Some((naga::ShaderStage::Compute, entry.to_string())),
        allow_and_force_point_size: false,
        vertex_pulling_transform: false,
        vertex_buffer_mappings: Vec::new(),
        binding_array_length_map: Default::default(),
    };
    let (source, tr) = msl::write_string(&module, &info, &options, &pipeline).map_err(gpu)?;
    let name = tr
        .entry_point_names
        .into_iter()
        .next()
        .ok_or_else(|| gpu("no entry point"))?
        .map_err(gpu)?;
    if source.contains("_buffer_sizes.") {
        return Err(gpu("precise pipeline: shader reads buffer sizes"));
    }
    // Metal's default sqrt is about one ulp off; the precise variant is
    // correctly rounded like Rust's `f32::sqrt`.
    let source = precise_divisions(&local_threadgroup(&source, &name)?)?
        .replace("metal::sqrt(", "metal::precise::sqrt(")
        .replace("metal::pow(", "metal::precise::pow(");
    Ok((format!("{PRAGMAS}\n{source}"), name))
}

/// IEEE semantics (no reassociation, no approximate library functions) and
/// no contraction: every product and sum rounds separately, as in Rust.
const PRAGMAS: &str = "#pragma METAL fp math_mode(safe)\n#pragma METAL fp contract(off)";

/// The WGSL helpers `fn pdiv(a: f32, b: f32) -> f32 { return a / b; }` and
/// `fn pdiv3(a: vec3<f32>, b: vec3<f32>) -> vec3<f32>` become a correctly
/// rounded division built from the fast reciprocal: one Newton step on the
/// reciprocal, then two residual corrections of the quotient with exact
/// FMA residuals (the CUDA `div.rn` sequence). `metal::precise::divide` is
/// also correctly rounded but costs about 60% of a 100-layer composite;
/// this costs about 5%. Measured bit-exact against the CPU for every pair
/// of 8-bit values and millions of random pairs (tests/precise.rs). Division
/// by zero yields NaN rather than ±∞; shaders must guard it (the compositor
/// selects the result away wherever the CPU special-cases zero). Operands
/// must keep the quotient and reciprocal out of the subnormal range.
fn precise_divisions(source: &str) -> EngineResult<String> {
    let mut out = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(i) = ["float pdiv(", "metal::float3 pdiv3_("]
        .iter()
        .filter_map(|h| rest.find(h))
        .min()
    {
        let t = if rest[i..].starts_with("float ") {
            "float"
        } else {
            "metal::float3"
        };
        let body = i + rest[i..]
            .find(") {\n")
            .ok_or_else(|| gpu("precise pipeline: pdiv signature"))?;
        let end = body
            + rest[body..]
                .find("\n}")
                .ok_or_else(|| gpu("precise pipeline: pdiv body"))?;
        let not_div = || gpu("precise pipeline: pdiv is not `return a / b;`");
        let expr = rest[body..end]
            .trim_start_matches(") {")
            .trim()
            .strip_prefix("return ")
            .and_then(|r| r.strip_suffix(';'))
            .ok_or_else(not_div)?;
        let (a, b) = expr.split_once(" / ").ok_or_else(not_div)?;
        out.push_str(&rest[..body]);
        out.push_str(&format!(
            ") {{\n    {t} y = metal::fast::divide({t}(1.0), {b});\n    \
             y = metal::fma(metal::fma(-{b}, y, {t}(1.0)), y, y);\n    \
             {t} q = {a} * y;\n    \
             q = metal::fma(metal::fma(-{b}, q, {a}), y, q);\n    \
             return metal::fma(metal::fma(-{b}, q, {a}), y, q);"
        ));
        rest = &rest[end..];
    }
    out.push_str(rest);
    Ok(out)
}

/// naga passes workgroup variables to the kernel as `threadgroup T& x
/// [[threadgroup(i)]]` arguments whose lengths the host sets; passthrough
/// pipelines carry no lengths. Declare them inside the kernel instead.
fn local_threadgroup(source: &str, name: &str) -> EngineResult<String> {
    let head = format!("kernel void {name}(");
    let start = source
        .find(&head)
        .ok_or_else(|| gpu("precise pipeline: kernel not found"))?;
    let body = start
        + source[start..]
            .find(") {")
            .ok_or_else(|| gpu("precise pipeline: kernel signature"))?;
    let params = &source[start + head.len()..body];
    let mut kept = Vec::new();
    let mut locals = String::new();
    for p in params.split('\n') {
        let t = p.trim().trim_start_matches(',').trim();
        if let Some(decl) = t.strip_prefix("threadgroup ") {
            let decl = decl.split(" [[").next().unwrap_or(decl);
            let (ty, var) = decl
                .rsplit_once("& ")
                .ok_or_else(|| gpu("precise pipeline: threadgroup argument"))?;
            locals.push_str(&format!("    threadgroup {ty} {var};\n"));
        } else if !t.is_empty() {
            kept.push(t.to_string());
        }
    }
    Ok(format!(
        "{}{}\n  {}\n) {{\n{}{}",
        &source[..start],
        head,
        kept.join("\n, "),
        locals,
        &source[body + 3..]
    ))
}
