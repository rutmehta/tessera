//! Bounded, collision-safe cache of structurally specialized document
//! kernels, compiled in the background.
//!
//! A kernel is keyed by the program's *structure* (per step: kind, blend
//! mode, flags, source kind, adjustment kind; plus depth and live slab
//! count). Parameters stay in the step buffer, so painting, opacity drags
//! and mask edits never compile. A new structure starts compiling on a
//! worker thread and the interpreter renders until the kernel is ready.
//! Both are compiled with IEEE maths from the same WGSL formulas
//! (`gpu_core::precise_compute_pipeline`), so they produce bit-identical
//! pixels and switching needs no re-render.
use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use super::program::Program;
use crate::gpu::shader;

/// Structures cached (LRU).
const CAPACITY: usize = 8;
/// Longer programs use the interpreter (bounds compile time and code size).
pub(super) const MAX_STEPS: usize = 256;
/// Background compilations in flight at once.
const MAX_COMPILING: usize = 2;

type Structure = Vec<[u32; 5]>;

#[derive(Default)]
pub(super) struct Cache {
    entries: VecDeque<Entry>,
}

struct Entry {
    hash: [u8; 32],
    structure: Structure,
    state: State,
}

enum State {
    Compiling(Receiver<Option<wgpu::ComputePipeline>>),
    Ready(wgpu::ComputePipeline),
    Failed,
}

impl Entry {
    /// Collects a finished compilation; `block` waits for it.
    fn poll(&mut self, block: bool) {
        if let State::Compiling(rx) = &self.state {
            let got = if block {
                rx.recv().map_err(|_| TryRecvError::Disconnected)
            } else {
                rx.try_recv()
            };
            self.state = match got {
                Ok(Some(p)) => State::Ready(p),
                Ok(None) | Err(TryRecvError::Disconnected) => State::Failed,
                Err(TryRecvError::Empty) => return,
            };
        }
    }
}

impl Cache {
    /// Kernels compiled and ready.
    pub fn len(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| matches!(e.state, State::Ready(_)))
            .count()
    }

    /// Blocks until every started compilation has finished.
    pub fn wait(&mut self) {
        for e in &mut self.entries {
            e.poll(true);
        }
    }

    /// The specialized kernel for `program`'s structure if it is compiled;
    /// otherwise starts compiling it (at most [`MAX_COMPILING`] at a time)
    /// and returns `None`, so the caller uses the interpreter.
    pub fn pipeline(
        &mut self,
        device: &wgpu::Device,
        depth: u32,
        slabs: usize,
        program: &Program,
    ) -> Option<wgpu::ComputePipeline> {
        if program.steps.is_empty() || program.steps.len() > MAX_STEPS {
            return None;
        }
        let mut structure: Structure = program
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
            let mut entry = self.entries.remove(i).unwrap();
            entry.poll(false);
            let result = match &entry.state {
                State::Ready(p) => Some(p.clone()),
                _ => None,
            };
            self.entries.push_back(entry);
            return result;
        }
        let compiling = self
            .entries
            .iter_mut()
            .filter_map(|e| {
                e.poll(false);
                matches!(e.state, State::Compiling(_)).then_some(())
            })
            .count();
        if compiling >= MAX_COMPILING {
            return None;
        }
        let (tx, rx) = channel();
        let (device, key) = (device.clone(), structure.clone());
        let spawned = std::thread::Builder::new()
            .name("resident specialization".into())
            .spawn(move || {
                let _ = tx.send(compile(&device, &key, depth, slabs));
            });
        if spawned.is_err() {
            return None;
        }
        if self.entries.len() == CAPACITY {
            // A dropped compilation finishes on its thread and is discarded.
            self.entries.pop_front();
        }
        self.entries.push_back(Entry {
            hash,
            structure,
            state: State::Compiling(rx),
        });
        None
    }
}

/// Compiles one structure (on a worker thread). `None` on any failure:
/// that structure then stays on the interpreter.
fn compile(
    device: &wgpu::Device,
    structure: &Structure,
    depth: u32,
    slabs: usize,
) -> Option<wgpu::ComputePipeline> {
    let steps = &structure[..structure.len() - 1];
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let pipe = gpu_core::precise_compute_pipeline(
        device,
        "specialized resident structure",
        &source(steps, depth, slabs),
        "main",
        (16, 16, 1),
        &super::doc_layout_entries(),
    );
    match (pipe, pollster::block_on(scope.pop())) {
        (Ok(p), None) => Some(p.pipeline),
        (Err(error), _) => {
            eprintln!("resident specialization fallback: {error}");
            None
        }
        (_, Some(error)) => {
            eprintln!("resident specialization fallback: {error}");
            None
        }
    }
}

fn source(steps: &[[u32; 5]], depth: u32, slabs: usize) -> String {
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
    for s in steps {
        modes.insert(s[1]);
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
    for (chunk, steps) in steps.chunks(64).enumerate() {
        out.push_str(&format!(
            "{{ let c0 = {}u; let n = {}u;\n",
            chunk * 64,
            steps.len()
        ));
        out.push_str(stage);
        for (j, &[kind, mode, flags, src, _]) in steps.iter().enumerate() {
            out.push_str(&format!("loop {{ let j = {j}u;\n"));
            let body = body
                .replace(
                    "let h = sh_h[j];",
                    &format!("let h = vec4<u32>({kind}u, {mode}u, {flags}u, {src}u);"),
                )
                .replace("fast.z", &format!("{flags}u"))
                .replace("fast.y", &format!("{mode}u"))
                .replace(&format!("blend_px({mode}u,"), &format!("blend_{mode}("))
                .replace("blend_px(h.y,", &format!("blend_{mode}("))
                .replace("composite_step(", &format!("step_{mode}("))
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
