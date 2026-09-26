//! Bounded, collision-safe cache of structurally specialized document kernels.
//! Parameters stay in the step buffer; pixel/opacity edits never compile shaders.
use super::program::Program;
use crate::gpu::shader;
use std::collections::VecDeque;

#[derive(Default)]
pub(super) struct Cache {
    entries: VecDeque<Entry>,
}

struct Entry {
    hash: [u8; 32],
    structure: Vec<[u32; 5]>,
    pipeline: Option<wgpu::ComputePipeline>,
}

impl Cache {
    pub fn len(&self) -> usize {
        self.entries.iter().filter(|e| e.pipeline.is_some()).count()
    }

    pub fn pipeline(
        &mut self,
        device: &wgpu::Device,
        depth: u32,
        slabs: usize,
        program: &Program,
    ) -> Option<wgpu::ComputePipeline> {
        // Cap compiler work and code size. Unsupported/failed programs stay on
        // the interpreter, including repeated requests for a failed structure.
        // Constant folding changes floating-point association. Discontinuous
        // operators can turn a one-ulp input drift into a large output jump
        // (the 100-layer benchmark demonstrated 0.082 vs the 0.002 gate).
        // Preserve the reference interpreter for these entire structures.
        let discontinuous = program.steps.iter().any(|s| {
            matches!(s.mode, 1 | 6 | 11 | 18)
                || (s.kind == super::program::K_ADJUST && matches!(s.adj, 2 | 3))
        });
        if program.steps.is_empty() || program.steps.len() > 128 || discontinuous {
            return None;
        }
        let mut structure: Vec<_> = program
            .steps
            .iter()
            .map(|s| [s.kind, s.mode, s.flags, s.src, s.adj])
            .collect();
        structure.push([depth, slabs as u32, 0, 0, 0]);
        let hash = *blake3::hash(bytemuck::cast_slice(&structure)).as_bytes();
        if let Some(i) = self
            .entries
            .iter()
            .position(|e| e.hash == hash && e.structure == structure)
        {
            let entry = self.entries.remove(i).unwrap();
            let result = entry.pipeline.clone();
            self.entries.push_back(entry);
            return result;
        }
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("specialized resident structure"),
            source: wgpu::ShaderSource::Wgsl(source(program, depth, slabs).into()),
        });
        // Explicit layout retains bindings pruned by constant folding.
        let entries: Vec<_> = (0..16)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: if binding == 9 || binding == 10 {
                        wgpu::BufferBindingType::Uniform
                    } else {
                        wgpu::BufferBindingType::Storage {
                            read_only: binding != 15,
                        }
                    },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .collect();
        let bindings = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("resident explicit bindings"),
            entries: &entries,
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("resident shared layout"),
            bind_group_layouts: &[Some(&bindings)],
            immediate_size: 0,
        });
        let pipe = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("specialized resident structure"),
            layout: Some(&layout),
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let result = if let Some(error) = pollster::block_on(scope.pop()) {
            eprintln!("resident specialization fallback: {error}");
            None
        } else {
            Some(pipe)
        };
        if self.entries.len() == 8 {
            self.entries.pop_front();
        }
        self.entries.push_back(Entry {
            hash,
            structure,
            pipeline: result.clone(),
        });
        result
    }
}

fn source(program: &Program, depth: u32, slabs: usize) -> String {
    // Use the interpreter as the single source of semantics. Emit its step
    // body once per structural step with constant kind/mode/flags/source and
    // literal indices. Metal folds the branches and per-mode blend switch.
    let template = include_str!("doc.wgsl");
    let (prefix, rest) = template.split_once("    for (var c0 = 0u;").unwrap();
    let (_, rest) = rest
        .split_once("        let n = min(CHUNK, frame.nsteps - c0);\n")
        .unwrap();
    let (stage, rest) = rest
        .split_once("    for (var j = 0u; j < n; j++) {\n")
        .unwrap();
    let (body, suffix) = rest.rsplit_once("    }\n    }\n").unwrap();
    let mut out = prefix.to_string();
    // Explicitly emit switch-free blend functions rather than relying on the
    // driver's inlining heuristics for the large general blend function.
    let blend = include_str!("../blend.wgsl");
    let core = blend.split_once("fn composite_core(").unwrap().1;
    let step = prefix
        .split_once("fn composite_step(")
        .unwrap()
        .1
        .split_once("// Steps are staged")
        .unwrap()
        .0;
    let mut modes = std::collections::BTreeSet::new();
    for s in &program.steps {
        modes.insert(s.mode);
    }
    let mut functions = String::new();
    for mode in modes {
        let expr = if mode <= 1 {
            "return s;"
        } else {
            let marker = format!("case {mode}u: {{");
            let tail = blend.split_once(&marker).unwrap().1;
            let mut nesting = 1i32;
            let end = tail
                .char_indices()
                .find_map(|(i, c)| {
                    if c == '{' {
                        nesting += 1;
                    }
                    if c == '}' {
                        nesting -= 1;
                    }
                    (nesting == 0).then_some(i)
                })
                .unwrap();
            &tail[..end]
        };
        functions.push_str(&format!("fn blend_{mode}(b: vec3<f32>, s: vec3<f32>) -> vec3<f32> {{ let one = vec3<f32>(1.0); let zero = vec3<f32>(0.0); let half = vec3<f32>(0.5); {expr} }}\n"));
        functions.push_str(&format!(
            "fn core_{mode}({}",
            core.replace("(mode ==", &format!("({mode}u =="))
                .replace("blend_px(mode,", &format!("blend_{mode}("))
        ));
        functions.push_str(&format!(
            "fn step_{mode}({}",
            step.replace("composite_core(", &format!("core_{mode}("))
        ));
    }
    for (chunk, steps) in program.steps.chunks(64).enumerate() {
        out.push_str(&format!(
            "{{ let c0 = {}u; let n = {}u;\n",
            chunk * 64,
            steps.len()
        ));
        out.push_str(stage);
        for (j, s) in steps.iter().enumerate() {
            out.push_str(&format!("loop {{ let j = {j}u;\n"));
            let body = body
                .replace(
                    "let h = sh_h[j];",
                    &format!(
                        "let h = vec4<u32>({}u, {}u, {}u, {}u);",
                        s.kind, s.mode, s.flags, s.src
                    ),
                )
                .replace("fast.z", &format!("{}u", s.flags))
                .replace("fast.y", &format!("{}u", s.mode))
                .replace(
                    &format!("blend_px({}u,", s.mode),
                    &format!("blend_{}(", s.mode),
                )
                .replace("blend_px(h.y,", &format!("blend_{}(", s.mode))
                .replace("composite_step(", &format!("step_{}(", s.mode))
                .replace("continue;", "break;");
            out.push_str(&body);
            out.push_str("break; }\n");
        }
        out.push_str("}\n");
    }
    out.push_str(suffix);
    out.push_str(&functions);
    let pages = include_str!("pages.wgsl").replace("ACCESS", "read");
    let (head, tail) = pages.split_once("    switch page >> 29u {").unwrap();
    let (_, tail) = tail.split_once("\n    }\n").unwrap();
    let mut load = String::new();
    if slabs <= 1 {
        load.push_str("    return slab0[w];\n");
    } else {
        load.push_str("    switch page >> 29u {\n");
        for i in 0..slabs - 1 {
            load.push_str(&format!("case {i}u: {{ return slab{i}[w]; }}\n"));
        }
        load.push_str(&format!("default: {{ return slab{}[w]; }}\n}}", slabs - 1));
    }
    shader(&format!("{head}{load}{tail}\n{out}"))
        .replace("pool.depth", &format!("{depth}u"))
        .replace("pool.page_words", &format!("{}u", 65536u32 << depth))
}
