//! Point-stage fusion: one invocation keeps tone, curves, color and effects in registers.

use engine_api::{
    EngineError, EngineResult,
    tile::{TILE_SIZE, TileCoord, TileLayout},
};
use image_core::Op;

/// One last-used immutable parameter set per render worker. Bounded regardless
/// of slider history, and no cross-device GPU resources or image pixels.
pub(crate) struct ConstantsCache<K> {
    entry: Option<(K, Vec<f32>)>,
}
impl<K> Default for ConstantsCache<K> {
    fn default() -> Self {
        Self { entry: None }
    }
}
impl<K: PartialEq + Clone> ConstantsCache<K> {
    pub(crate) fn get_or_try_insert(
        &mut self,
        key: &K,
        build: impl FnOnce() -> EngineResult<Vec<f32>>,
    ) -> EngineResult<Vec<f32>> {
        if let Some((old, value)) = &self.entry
            && old == key
        {
            return Ok(value.clone());
        }
        let value = build()?;
        self.entry = Some((key.clone(), value.clone()));
        Ok(value)
    }
}

fn slot(op: &Op<'_>) -> Option<usize> {
    match op {
        Op::Tone(_) => Some(0),
        Op::ToneExtra(s) if s.texture == 0. && s.clarity == 0. && s.dehaze == 0. => Some(1),
        Op::Color(_) => Some(2),
        Op::Effects(..) | Op::EffectsInCrop(..) => Some(3),
        Op::Display { .. } => Some(4),
        _ => None,
    }
}
pub(crate) fn supports(ops: &[Op<'_>]) -> bool {
    !ops.is_empty()
        && ops.iter().all(|op| slot(op).is_some())
        && ops.windows(2).all(|w| slot(&w[0]) < slot(&w[1]))
}
pub(crate) fn parameters(
    ops: &[Op<'_>],
    input: TileLayout,
    coord: TileCoord,
) -> EngineResult<(Vec<f32>, TileLayout)> {
    if !supports(ops) || input.channels != 3 {
        return Err(EngineError::invalid(
            "fused chain",
            "ordered RGB point stages required",
        ));
    }
    let mut out = input;
    if matches!(ops.last(), Some(Op::Display { .. })) {
        out.halo = 0;
    }
    // Standard operator layout header followed by five optional block offsets.
    let mut p = vec![0.; 38];
    p[1] = input.extent.width as f32;
    p[2] = input.extent.height as f32;
    p[3] = input.halo as f32;
    p[4] = out.halo as f32;
    p[5] = input.plane_len() as f32;
    p[6] = out.plane_len() as f32;
    for op in ops {
        let block = match op {
            Op::Effects(s, e) => {
                crate::effects::parameters(input, coord, s, *e, &Default::default())?
            }
            Op::EffectsInCrop(s, e, crop) => crate::effects::parameters(input, coord, s, *e, crop)?,
            _ => crate::operator::parameters(op, input, coord.pixel_origin(TILE_SIZE))?.0,
        };
        p[33 + slot(op).unwrap()] = p.len() as f32;
        p.extend(block);
    }
    Ok((p, out))
}
pub(crate) fn pipeline(ctx: &crate::GpuContext) -> wgpu::ComputePipeline {
    // Compose the same tested scalar functions, not a second copy of their math.
    // Each block retains its own relative curve-table offsets.
    let operators = include_str!("operators.wgsl")
        .split("@compute")
        .next()
        .unwrap()
        .replace("p[", "p[base + ");
    let effects = include_str!("effects.wgsl")
        .split("const MAX")
        .nth(1)
        .unwrap()
        .split("@compute")
        .next()
        .unwrap()
        .replace("p[", "p[base + ");
    let source = format!(
        "var<private> base: u32;\n{operators}\nconst MAX{effects}\n{}",
        include_str!("fused.wgsl")
    );
    let module = ctx
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fused point stages"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
    ctx.device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("fused tone color effects"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        })
}

#[cfg(test)]
mod tests {
    #[test]
    fn grading_directions_are_prepared_once_per_parameter_set() {
        let mut settings = engine_api::recipe::settings::ColorSettings::default();
        settings.grading.shadows.hue = 90.;
        let mut p = vec![0.; 33];
        crate::color::parameters(&settings, &mut p).unwrap();
        assert_eq!(p.len(), 81);
        assert!(p[73].abs() < 1e-6);
        assert!((p[74] - 1.).abs() < 1e-6);
        settings.grading.shadows.hue = 180.;
        let mut edited = vec![0.; 33];
        crate::color::parameters(&settings, &mut edited).unwrap();
        assert!((edited[73] + 1.).abs() < 1e-6);
    }

    #[test]
    fn constants_cache_reuses_equal_keys_and_invalidates_edits() {
        let mut cache = super::ConstantsCache::<u32>::default();
        let calls = std::cell::Cell::new(0);
        for key in [1, 1, 2, 2, 1] {
            let value = cache
                .get_or_try_insert(&key, || {
                    calls.set(calls.get() + 1);
                    Ok(vec![key as f32])
                })
                .unwrap();
            assert_eq!(value, vec![key as f32]);
        }
        assert_eq!(calls.get(), 3);
    }
}
