// Copied temporarily to crates/pipeline-gpu/tests by sdr_audit.py. No production hooks.
#[path = "../../image-core/tests/common/mod.rs"]
mod common;
use engine_api::{
    EngineError, EngineResult,
    recipe::{DevelopSettings, settings::ToneSettings},
    stage::StageId,
    tile::{TILE_SIZE, Tile},
};
use image_core::{
    CpuStageOp, Op, PixelRect, RenderOutput, Renderer, RendererConfig, StageOp, TileCache,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

// The runner inserts the complete tone function, unmodified apart from its name
// and crate-qualified imports, from the pinned pre-ENG-4 base.
/* BASELINE_TONE */

fn luminance(rgb: [f32; 3]) -> f32 {
    0.2627 * rgb[0] + 0.6780 * rgb[1] + 0.0593 * rgb[2]
}
use pipeline_cpu::{legacy_pv2010, map_rgb};
type Pixels = BTreeMap<(u32, u32), [f32; 3]>;
fn collect(tile: &Tile) -> Pixels {
    let l = tile.layout();
    let n = l.plane_len();
    let v = tile.samples::<f32>().unwrap();
    let (ox, oy) = tile.coord().pixel_origin(TILE_SIZE);
    let mut result = Pixels::new();
    for y in 0..l.extent.height {
        for x in 0..l.extent.width {
            let i = (y as usize + l.halo as usize) * l.stride() + x as usize + l.halo as usize;
            result.insert((ox + x, oy + y), std::array::from_fn(|c| v[c * n + i]));
        }
    }
    result
}
#[derive(Default)]
struct Audit {
    baseline: bool,
    tone_in: Mutex<Pixels>,
    tone_out: Mutex<Pixels>,
    encoded: Mutex<Pixels>,
}
impl StageOp for Audit {
    fn run(&self, stage: StageId, op: &Op<'_>, input: Tile) -> EngineResult<Tile> {
        if let Op::Tone(settings) = op {
            self.tone_in.lock().unwrap().extend(collect(&input));
            let mut output = input;
            if self.baseline {
                baseline_tone(&mut output, settings)?;
            } else {
                pipeline_cpu::tone(&mut output, settings)?;
            }
            self.tone_out.lock().unwrap().extend(collect(&output));
            return Ok(output);
        }
        if let Op::Display {
            gamut,
            headroom: None,
        } = op
        {
            let encoded = pipeline_cpu::display_float(&input, Default::default(), *gamut)?;
            self.encoded.lock().unwrap().extend(collect(&encoded));
        }
        CpuStageOp.run(stage, op, input)
    }
}
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3)
    })
}
fn output(tiles: &[Tile]) -> BTreeMap<(u32, u32), [u8; 3]> {
    let mut result = BTreeMap::new();
    for tile in tiles {
        let l = tile.layout();
        assert_eq!(l.halo, 0);
        let n = l.plane_len();
        let v = tile.samples::<u8>().unwrap();
        let (ox, oy) = tile.coord().pixel_origin(TILE_SIZE);
        for y in 0..l.extent.height {
            for x in 0..l.extent.width {
                let i = (y * l.extent.width + x) as usize;
                result.insert((ox + x, oy + y), std::array::from_fn(|c| v[c * n + i]));
            }
        }
    }
    result
}
fn settings() -> Vec<DevelopSettings> {
    let mut a = DevelopSettings::default();
    a.tone.exposure = 1.7;
    a.tone.highlights = -30.;
    let mut b = DevelopSettings::default();
    b.color.vibrance = 40.;
    b.tone.shadows = 25.;
    vec![DevelopSettings::default(), a, b]
}
#[test]
fn sdr_golden_attribution() {
    let dir = std::path::PathBuf::from(std::env::var_os("ENG4_AUDIT_DIR").unwrap());
    let mut report = Vec::new();
    let expected = [0x44fae39b8cad1e, 0xcc470d33eeb4d28a, 0x39f4bd02fec83fd1];
    let image = common::synthetic(2201, 300, 211, common::RGGB, [0, 0, 300, 211]);
    // Match hdr_surface exactly: one renderer/cache reused for all three cases.
    let renderers = [true, false].map(|baseline| {
        let audit = Arc::new(Audit {
            baseline,
            ..Default::default()
        });
        let config = RendererConfig::default();
        let renderer = Renderer::with_ops(
            audit.clone(),
            Arc::new(TileCache::new(config.cache_budget_bytes)),
            config,
        );
        (audit, renderer)
    });
    for (case, s) in settings().iter().enumerate() {
        let mut captures = Vec::new();
        for (audit, renderer) in &renderers {
            let baseline = audit.baseline;
            let tiles = renderer
                .render_region_as(
                    &image,
                    s,
                    0,
                    PixelRect::full(image.level_extent(0)),
                    RenderOutput::Display,
                )
                .unwrap();
            let bytes: Vec<u8> = tiles
                .iter()
                .flat_map(|t| t.samples::<u8>().unwrap().to_vec())
                .collect();
            std::fs::write(
                dir.join(format!(
                    "sdr-{case}-{}.planar-tiles.u8",
                    if baseline { "before" } else { "after" }
                )),
                &bytes,
            )
            .unwrap();
            if baseline {
                assert_eq!(
                    fnv(&bytes),
                    expected[case],
                    "baseline replay must reproduce stored fingerprint"
                );
            }
            captures.push((audit.clone(), output(&tiles), fnv(&bytes)));
        }
        let (before, old, old_hash) = &captures[0];
        let (after, new, new_hash) = &captures[1];
        let before_in = before.tone_in.lock().unwrap();
        let after_in = after.tone_in.lock().unwrap();
        assert_eq!(before_in.len(), after_in.len());
        for (xy, rgb) in before_in.iter() {
            assert_eq!(
                rgb.map(f32::to_bits),
                after_in[xy].map(f32::to_bits),
                "upstream tone input bits changed"
            );
        }
        let before_tone = before.tone_out.lock().unwrap();
        let after_tone = after.tone_out.lock().unwrap();
        let before_encoded = before.encoded.lock().unwrap();
        let after_encoded = after.encoded.lock().unwrap();
        let mut changed = Vec::new();
        let mut max_delta = 0;
        let mut seeds = 0;
        let bayer = [
            [0u8, 8, 2, 10],
            [12, 4, 14, 6],
            [3, 11, 1, 9],
            [15, 7, 13, 5],
        ];
        for (&xy, p) in old {
            let q = new[&xy];
            let noise =
                (f32::from(bayer[(xy.1 % 4) as usize][(xy.0 % 4) as usize]) + 0.5) / 16. - 0.5;
            let old_pre = before_encoded[&xy].map(|v| v * 255. + noise);
            let new_pre = after_encoded[&xy].map(|v| v * 255. + noise);
            let quant = |v: f32| v.round().clamp(0., 255.) as u8;
            assert_eq!(old_pre.map(quant), *p, "old quantizer replay");
            assert_eq!(new_pre.map(quant), q, "new quantizer replay");
            let active =
                luminance(before_in[&xy].map(|v| v * s.tone.exposure.clamp(-10., 10.).exp2())) > 0.
                    && [
                        s.tone.contrast,
                        s.tone.highlights,
                        s.tone.shadows,
                        s.tone.whites,
                        s.tone.blacks,
                    ]
                    .iter()
                    .any(|v| *v != 0.);
            let tone_moved =
                before_tone[&xy].map(f32::to_bits) != after_tone[&xy].map(f32::to_bits);
            if tone_moved {
                assert!(
                    active,
                    "tone movement outside active positive-luminance support"
                );
                seeds += 1;
            }
            let crosses = old_pre.map(quant) != new_pre.map(quant);
            let predicate = active && tone_moved && crosses;
            assert_eq!(
                *p != q,
                predicate,
                "changed pixel outside pointwise tone / quantizer predicate at {xy:?}"
            );
            if *p != q {
                max_delta = max_delta.max((0..3).map(|c| p[c].abs_diff(q[c])).max().unwrap());
                changed.push(format!(r#"{{"x":{},"y":{},"index":{},"before":{:?},"after":{:?},"tone_input":{:?},"tone_before":{:?},"tone_after":{:?},"encoded_before":{:?},"encoded_after":{:?},"dither":{},"pre_round_before":{:?},"pre_round_after":{:?},"predicate":true}}"#,xy.0,xy.1,xy.1*300+xy.0,p,q,before_in[&xy],before_tone[&xy],after_tone[&xy],before_encoded[&xy],after_encoded[&xy],noise,old_pre,new_pre));
            }
        }
        report.push(format!(r#"{{"case":{},"pixels":{},"tone_changed_pixels":{},"changed_pixels":{},"max_encoded_delta_u8":{},"outside_predicate":0,"before_fnv":"{:#x}","after_fnv":"{:#x}","changes":[{}]}}"#,case,old.len(),seeds,changed.len(),max_delta,old_hash,new_hash,changed.join(",")));
    }
    std::fs::write(
        dir.join("sdr-report.json"),
        format!("[{}]\n", report.join(",\n")),
    )
    .unwrap();
    eprintln!("[{}]", report.join(",\n"));
}
