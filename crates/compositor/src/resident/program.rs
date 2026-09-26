//! The layer tree flattened once per document state into GPU steps (the
//! tile-independent twin of `render::exec::TileJob::compile`).

use std::sync::Arc;

use engine_api::{EngineError, EngineResult};

use crate::adjust::{Adjustment, Compiled};
use crate::document::{Fill, GradientKind, GroupMode, Knockout, Layer, LayerKind};
use crate::render::pixel::Params;

/// Maximum nesting of isolated/clip/pass-through frames the shader stack
/// holds (the root frame excluded).
pub const MAX_NESTING: usize = 7;

pub(super) const K_BLEND: u32 = 0;
pub(super) const K_ADJUST: u32 = 1;
pub(super) const K_PUSH: u32 = 2;
pub(super) const K_PUSH_PASS: u32 = 3;
pub(super) const K_POP: u32 = 4;
pub(super) const K_POP_PASS: u32 = 5;
pub(super) const K_SNAPSHOT: u32 = 6;

const F_ATOP: u32 = 1;
const F_BLEND_IF: u32 = 8;
const F_MASK: u32 = 16;
const F_SKIP_ABSENT: u32 = 32;
const F_PLAIN: u32 = 64;

pub(super) const S_RASTER: u32 = 0;
const S_SOLID: u32 = 1;
const S_LINEAR: u32 = 2;
const S_RADIAL: u32 = 3;
const S_PATTERN: u32 = 4;
const S_CONSTANT: u32 = 5;
pub(super) const S_SMART: u32 = 6;

/// One GPU step (`Step` in doc.wgsl).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct Step {
    pub kind: u32,
    pub mode: u32,
    pub flags: u32,
    pub src: u32,
    pub table: u32,
    pub mask_table: u32,
    pub adj: u32,
    pub aux: u32,
    pub aux_n: u32,
    pub seed: u32,
    pub _u: [u32; 2],
    pub opacity: f32,
    pub fill: f32,
    pub src_default: f32,
    pub density: f32,
    pub mask_default: f32,
    pub _g: [f32; 3],
    pub p: [[f32; 4]; 3],
    pub bi: [[f32; 4]; 8],
}

impl Step {
    fn new(kind: u32) -> Self {
        Self {
            kind,
            mode: 0,
            flags: 0,
            src: 0,
            table: 0,
            mask_table: 0,
            adj: 0,
            aux: 0,
            aux_n: 0,
            seed: 0,
            _u: [0; 2],
            opacity: 1.0,
            fill: 1.0,
            src_default: 0.0,
            density: 1.0,
            mask_default: 1.0,
            _g: [0.0; 3],
            p: [[0.0; 4]; 3],
            bi: [[0.0, 0.0, 1.0, 1.0]; 8],
        }
    }

    fn set(&mut self, p: &Params) {
        self.mode = p.mode.index();
        self.opacity = p.opacity;
        self.fill = p.fill;
        self.seed = p.seed;
        self.flags |= u32::from(p.atop)
            | match p.knockout {
                Knockout::None => 0,
                Knockout::Shallow => 2,
                Knockout::Deep => 4,
            };
        if let Some(bi) = &p.blend_if {
            self.flags |= F_BLEND_IF;
            self.bi = bi.packed();
        }
        if p.atop {
            self.flags |= F_ATOP;
        }
    }
}

/// Which raster of a layer a page table resolves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Part {
    Content,
    Mask,
    Smart,
}

/// A page table the steps reference: table `i` starts at `i · grid_len`.
pub(super) struct TableRef {
    pub layer: Arc<Layer>,
    pub part: Part,
}

/// A compiled document: steps, auxiliary data (8-bit LUT, adjustment
/// LUTs, gradient stops, patterns) and the tables to resolve.
pub(super) struct Program {
    pub steps: Vec<Step>,
    pub aux: Vec<f32>,
    pub tables: Vec<TableRef>,
}

impl Program {
    pub fn compile(root: &[Arc<Layer>], grid_len: usize) -> EngineResult<Self> {
        // aux[0..256]: the CPU's 8-bit normalization table (`i / 255`).
        let mut p = Program {
            steps: Vec::new(),
            aux: (0..256).map(|i| i as f32 / 255.0).collect(),
            tables: Vec::new(),
        };
        let mut c = Compiler {
            p: &mut p,
            grid_len,
            depth: 0,
        };
        c.list(root, true)?;
        Ok(p)
    }

    /// The bytes that, with the page tables, determine every output pixel.
    pub fn bytes(&self) -> Vec<u8> {
        let mut v = bytemuck::cast_slice::<Step, u8>(&self.steps).to_vec();
        v.extend_from_slice(bytemuck::cast_slice(&self.aux));
        v
    }
}

struct Compiler<'p> {
    p: &'p mut Program,
    grid_len: usize,
    depth: usize,
}

impl Compiler<'_> {
    fn table(&mut self, layer: &Arc<Layer>, part: Part) -> EngineResult<u32> {
        let offset = self.p.tables.len() * self.grid_len;
        self.p.tables.push(TableRef {
            layer: layer.clone(),
            part,
        });
        u32::try_from(offset).map_err(|_| EngineError::ResourceExhausted {
            resource: "resident page tables".into(),
        })
    }

    fn push(&mut self, kind: u32) -> EngineResult<()> {
        self.depth += 1;
        if self.depth > MAX_NESTING {
            return Err(EngineError::Unsupported {
                what: format!("group nesting deeper than {MAX_NESTING} on the GPU"),
            });
        }
        self.p.steps.push(Step::new(kind));
        Ok(())
    }

    fn pop(&mut self, mut s: Step) {
        self.depth -= 1;
        s.kind = if s.kind == K_POP_PASS {
            K_POP_PASS
        } else {
            K_POP
        };
        self.p.steps.push(s);
    }

    fn mask(&mut self, s: &mut Step, layer: &Arc<Layer>) -> EngineResult<()> {
        if let Some(m) = layer.mask.as_ref().filter(|m| m.enabled) {
            let d = m.density.clamp(0.0, 1.0);
            s.flags |= F_MASK;
            s.density = d;
            s.mask_default = 1.0 - d * (1.0 - m.raster.default_value());
            s.mask_table = self.table(layer, Part::Mask)?;
        }
        Ok(())
    }

    // Mirrors TileJob::compile_list.
    fn list(&mut self, children: &[Arc<Layer>], root: bool) -> EngineResult<()> {
        let mut i = 0;
        while i < children.len() {
            let base = &children[i];
            i += 1;
            let start = i;
            while i < children.len() && children[i].props.clipped {
                i += 1;
            }
            if !base.props.visible {
                continue;
            }
            let clipped: Vec<&Arc<Layer>> = children[start..i]
                .iter()
                .filter(|c| c.props.visible)
                .collect();
            let base_is_adjustment = matches!(base.kind, LayerKind::Adjustment(_));
            if clipped.is_empty() || base_is_adjustment {
                self.emit(base, false)?;
                for c in clipped {
                    self.emit(c, false)?;
                }
            } else {
                self.push(K_PUSH)?;
                self.emit_with(base, Params::clip_base(base), true)?;
                for c in clipped {
                    self.emit(c, true)?;
                }
                let mut s = Step::new(K_POP);
                s.set(&Params::clip_pop(base));
                self.pop(s);
            }
            if root && start == 1 && base.props.background {
                self.p.steps.push(Step::new(K_SNAPSHOT));
            }
        }
        Ok(())
    }

    fn emit(&mut self, layer: &Arc<Layer>, atop: bool) -> EngineResult<()> {
        if let LayerKind::Group {
            mode: GroupMode::PassThrough,
            children,
        } = &layer.kind
            && !atop
        {
            self.push(K_PUSH_PASS)?;
            self.list(children, false)?;
            let mut s = Step::new(K_POP_PASS);
            s.set(&Params::of(layer, false));
            self.mask(&mut s, layer)?;
            self.pop(s);
            return Ok(());
        }
        if let LayerKind::Adjustment(adj) = &layer.kind {
            let mut s = Step::new(K_ADJUST);
            s.set(&Params::of(layer, atop));
            self.adjustment(&mut s, adj);
            self.mask(&mut s, layer)?;
            self.p.steps.push(s);
            return Ok(());
        }
        self.emit_with(layer, Params::of(layer, atop), true)
    }

    fn emit_with(&mut self, layer: &Arc<Layer>, params: Params, mask: bool) -> EngineResult<()> {
        let mut s = Step::new(K_BLEND);
        match &layer.kind {
            LayerKind::Pixel(_) | LayerKind::Text(_) => {
                let raster = layer
                    .raster()
                    .ok_or_else(|| EngineError::internal("no raster"))?;
                s.src = S_RASTER;
                s.src_default = raster.default_value();
                if s.src_default == 0.0 {
                    s.flags |= F_SKIP_ABSENT;
                }
                s.table = self.table(layer, Part::Content)?;
            }
            LayerKind::Fill(f) => self.fill(&mut s, f),
            LayerKind::SmartObject(_) => {
                s.src = S_SMART;
                s.flags |= F_SKIP_ABSENT;
                s.table = self.table(layer, Part::Smart)?;
            }
            LayerKind::Adjustment(_) => return Err(EngineError::internal("adjustment as source")),
            LayerKind::Group { children, .. } => {
                self.push(K_PUSH)?;
                self.list(children, false)?;
                let mut s = Step::new(K_POP);
                s.set(&params);
                if mask {
                    self.mask(&mut s, layer)?;
                }
                self.pop(s);
                return Ok(());
            }
        }
        s.set(&params);
        if mask {
            self.mask(&mut s, layer)?;
        }
        // The shader's fast path: a raster over the running backdrop with
        // nothing but mode, opacity and fill.
        if s.src == S_RASTER
            && s.flags == F_SKIP_ABSENT
            && s.mode != crate::BlendMode::Dissolve.index()
        {
            s.flags |= F_PLAIN;
        }
        self.p.steps.push(s);
        Ok(())
    }

    fn aux_offset(&self) -> u32 {
        self.p.aux.len() as u32
    }

    fn fill(&mut self, s: &mut Step, f: &Fill) {
        match f {
            Fill::Solid { color } => {
                s.src = S_SOLID;
                s.p[0] = [color[0], color[1], color[2], 1.0];
            }
            Fill::Gradient {
                gradient,
                start,
                end,
                stops,
            } => {
                s.src = match gradient {
                    GradientKind::Linear => S_LINEAR,
                    GradientKind::Radial => S_RADIAL,
                };
                s.p[0] = [start[0], start[1], end[0], end[1]];
                s.aux = self.aux_offset();
                s.aux_n = stops.len() as u32;
                for st in stops {
                    self.p.aux.push(st.position);
                    self.p.aux.extend_from_slice(&st.color);
                }
            }
            Fill::Pattern {
                width,
                height,
                rgba,
                origin,
            } => {
                if *width == 0 || *height == 0 || rgba.len() < (*width * *height * 4) as usize {
                    s.src = S_CONSTANT;
                    s.p[0] = [0.0; 4];
                    return;
                }
                s.src = S_PATTERN;
                s.p[0] = [*width as f32, *height as f32, origin[0], origin[1]];
                s.aux = self.aux_offset();
                self.p
                    .aux
                    .extend_from_slice(&rgba[..(*width * *height * 4) as usize]);
            }
        }
    }

    fn adjustment(&mut self, s: &mut Step, adj: &Adjustment) {
        match adj {
            Adjustment::Invert => s.adj = 0,
            Adjustment::Exposure {
                exposure,
                offset,
                gamma,
            } => {
                s.adj = 1;
                let g = if *gamma > 0.0 { 1.0 / gamma } else { 1.0 };
                s.p[0] = [exposure.exp2(), *offset, g, 0.0];
            }
            Adjustment::Threshold { level } => {
                s.adj = 2;
                s.p[0][0] = *level;
            }
            Adjustment::Posterize { levels } => {
                s.adj = 3;
                s.p[0][0] = (*levels).clamp(2, 255) as f32;
            }
            Adjustment::Levels { .. } | Adjustment::Curves { .. } => {
                s.adj = 4;
                s.aux = self.aux_offset();
                if let Compiled::Luts(ch, master) = adj.compile() {
                    for l in ch.iter().chain([&master]) {
                        self.p.aux.extend_from_slice(l);
                    }
                }
            }
            Adjustment::ChannelMixer {
                matrix,
                constant,
                monochrome,
            } => {
                s.adj = 5;
                for i in 0..3 {
                    let r = if *monochrome { 0 } else { i };
                    s.p[i] = [matrix[r][0], matrix[r][1], matrix[r][2], constant[r]];
                }
            }
            Adjustment::HueSaturation {
                hue,
                saturation,
                lightness,
                colorize,
            } => {
                s.adj = 6;
                s.p[0] = [
                    hue / 360.0,
                    (saturation / 100.0).clamp(-1.0, 1.0),
                    (lightness / 100.0).clamp(-1.0, 1.0),
                    if *colorize { 1.0 } else { 0.0 },
                ];
            }
        }
    }
}
