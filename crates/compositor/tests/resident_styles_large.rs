//! Round-two viewport regression and explicit cold/warm timing gate.
use std::{sync::Arc, time::Instant};

use compositor::{gpu::GpuCompositor, render::styles::*, resident::ResidentRenderer, *};
use engine_api::tile::{Extent, TILE_SIZE, TileCoord};
use rayon::prelude::*;

fn document(extent: Extent) -> Document {
    let mut state = DocState::new(extent, Depth::F32);
    let mut raster = Raster::new(extent, 4, Depth::F32, 0.0);
    // Sparse source is deliberate: canvas size must not dictate effect allocation.
    raster
        .edit_region(Rect::new(900, 700, 2100, 1600), 1, |x, y, p| {
            *p = [0.6, 0.2, 0.4, if (x + y) % 19 < 13 { 0.7 } else { 0.3 }];
        })
        .unwrap();
    let effects = [
        StyleEffect::DropShadow(Shadow::default()),
        StyleEffect::OuterGlow(Glow::default()),
        StyleEffect::Bevel(Bevel::default()),
        StyleEffect::Satin(Satin::default()),
        StyleEffect::Stroke(Stroke::default()),
    ];
    for effect in effects {
        let mut layer = Layer::new("styled", LayerKind::Pixel(raster.clone()));
        layer.props.styles.effects = vec![effect];
        state.root.push(Arc::new(layer));
    }
    Document::new(state)
}

fn cpu_view(cpu: &Compositor, doc: &Document, level: u8, view: Rect) {
    let ts = i64::from(TILE_SIZE);
    let coords: Vec<_> = (view.y0 / ts..=(view.y1 - 1) / ts)
        .flat_map(|y| {
            (view.x0 / ts..=(view.x1 - 1) / ts)
                .map(move |x| TileCoord::new(level, x as u32, y as u32))
        })
        .collect();
    coords.par_iter().for_each(|c| {
        cpu.render_tile(doc, *c).unwrap();
    });
}

#[test]
fn twenty_and_fifty_megapixel_viewports_l0_l1_l2() {
    let gpu = GpuCompositor::new().expect("M5-31 requires a GPU");
    for extent in [Extent::new(5000, 4000), Extent::new(10000, 5000)] {
        let doc = document(extent);
        let cpu = Compositor::new(128 << 20);
        let mut resident = ResidentRenderer::with_budget(&gpu, 128 << 20).unwrap();
        for level in [0, 1, 2] {
            let x = (900 >> level) - 8;
            let y = (700 >> level) - 8;
            // Exercise a real viewport, not just a few pixels on a large canvas.
            // The L2 20MP case also covers clipping at the document's right edge.
            let view = Rect::new(x, y, x + 1368, y + 912)
                .intersect(&Rect::of_extent(extent.at_level(level)));
            cpu_view(&cpu, &doc, level, view);
            let frame = resident.render_viewport(&doc, level, view, 0).unwrap();
            resident.wait().unwrap();
            assert!(frame.blocks > 0, "large viewport must dispatch work");
            assert_eq!(resident.style_evaluations(), 5 * (u64::from(level) + 1));
            assert!(cpu.stats().cache_bytes <= 128 << 20);
            assert_eq!(resident.filter_fallbacks(), 0);
            assert!(resident.style_cache_bytes() <= 128 << 20);
        }
    }
}

#[test]
#[ignore = "hardware timing gate: run alone in release mode"]
fn twenty_mp_five_styles_1368x912_l1_timing() {
    let gpu = GpuCompositor::new().expect("M5-31 requires a GPU");
    let doc = document(Extent::new(5000, 4000));
    let view = Rect::new(100, 100, 1468, 1012);
    let cpu = Compositor::new(512 << 20);
    let mut resident = ResidentRenderer::with_budget(&gpu, 512 << 20).unwrap();
    let start = Instant::now();
    cpu_view(&cpu, &doc, 1, view);
    let cpu_cold = start.elapsed();
    let start = Instant::now();
    let cold_frame = resident.render_viewport(&doc, 1, view, 0).unwrap();
    resident.wait().unwrap();
    let gpu_cold = start.elapsed();
    assert!(cold_frame.blocks > 0, "cold measurement must dispatch work");
    assert_eq!(resident.style_evaluations(), 5);
    assert_eq!(resident.filter_fallbacks(), 0);
    assert!(resident.style_cache_bytes() <= 512 << 20);
    resident.wait_for_specializations();
    // Neither measurement below is an unchanged-document idle frame.
    cpu.clear_composites();
    resident.invalidate();
    let start = Instant::now();
    cpu_view(&cpu, &doc, 1, view);
    let cpu_warm = start.elapsed();
    let start = Instant::now();
    let warm_frame = resident.render_viewport(&doc, 1, view, 0).unwrap();
    resident.wait().unwrap();
    let gpu_warm = start.elapsed();
    assert!(warm_frame.blocks > 0, "warm measurement must dispatch work");
    assert_eq!(
        resident.style_evaluations(),
        5,
        "reuse cached effect planes"
    );
    assert_eq!(resident.filter_fallbacks(), 0);
    eprintln!(
        "20MP 5 styles L1 1368x912 CPU cold={cpu_cold:?} warm={cpu_warm:?}; resident cold={gpu_cold:?} warm={gpu_warm:?}"
    );
    assert!(cpu_cold.as_secs_f64() < 2.0, "cold CPU {cpu_cold:?}");
    assert!(gpu_cold.as_secs_f64() < 0.1, "cold resident {gpu_cold:?}");
    assert!(cpu_warm.as_secs_f64() < 2.0, "warm CPU {cpu_warm:?}");
    assert!(gpu_warm.as_secs_f64() < 0.1, "warm resident {gpu_warm:?}");
}

#[test]
#[ignore = "hardware timing gate: run alone in release mode"]
fn twenty_mp_five_styles_1368x912_l1_unique_ids_timing() {
    let gpu = GpuCompositor::new().expect("M5-31 requires a GPU");
    // Keep the original requested fixture unchanged; this companion uses the
    // document edit API to assign real unique layer IDs and revisions.
    let fixture = document(Extent::new(5000, 4000));
    let mut doc = Document::new(DocState::new(Extent::new(5000, 4000), Depth::F32));
    for layer in &fixture.state().root {
        doc.apply(DocOp::AddLayer {
            parent: None,
            index: usize::MAX,
            layer: (**layer).clone(),
        })
        .unwrap();
    }
    let ids: std::collections::HashSet<_> = doc.state().root.iter().map(|l| l.id).collect();
    assert_eq!(ids.len(), 5);
    assert!(!ids.contains(&LayerId(0)));
    let view = Rect::new(100, 100, 1468, 1012);
    let cpu = Compositor::new(512 << 20);
    let mut resident = ResidentRenderer::with_budget(&gpu, 512 << 20).unwrap();
    let start = Instant::now();
    cpu_view(&cpu, &doc, 1, view);
    let cpu_cold = start.elapsed();
    let start = Instant::now();
    let cold_frame = resident.render_viewport(&doc, 1, view, 0).unwrap();
    resident.wait().unwrap();
    let gpu_cold = start.elapsed();
    assert!(cold_frame.blocks > 0, "cold measurement must dispatch work");
    assert_eq!(resident.style_evaluations(), 5);
    assert_eq!(resident.filter_fallbacks(), 0);
    assert!(resident.style_cache_bytes() <= 512 << 20);
    resident.wait_for_specializations();
    // Neither measurement below is an unchanged-document idle frame.
    cpu.clear_composites();
    resident.invalidate();
    let start = Instant::now();
    cpu_view(&cpu, &doc, 1, view);
    let cpu_warm = start.elapsed();
    let start = Instant::now();
    let warm_frame = resident.render_viewport(&doc, 1, view, 0).unwrap();
    resident.wait().unwrap();
    let gpu_warm = start.elapsed();
    assert!(warm_frame.blocks > 0, "warm measurement must dispatch work");
    assert_eq!(
        resident.style_evaluations(),
        5,
        "reuse cached effect planes"
    );
    assert_eq!(resident.filter_fallbacks(), 0);
    eprintln!(
        "20MP 5 styles unique IDs L1 1368x912 CPU cold={cpu_cold:?} warm={cpu_warm:?}; resident cold={gpu_cold:?} warm={gpu_warm:?}"
    );
    assert!(cpu_cold.as_secs_f64() < 2.0, "cold CPU {cpu_cold:?}");
    assert!(gpu_cold.as_secs_f64() < 0.1, "cold resident {gpu_cold:?}");
    assert!(cpu_warm.as_secs_f64() < 2.0, "warm CPU {cpu_warm:?}");
    assert!(gpu_warm.as_secs_f64() < 0.1, "warm resident {gpu_warm:?}");
}
