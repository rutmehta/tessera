//! GPU-required kernel and cropped-viewport tests; no skipped-adapter success.
use super::{ResidentRenderer, styles_gpu::StylesGpu};
use crate::{gpu::GpuCompositor, render::styles::*, *};
use engine_api::tile::Extent;
use std::sync::Arc;
use wgpu::util::DeviceExt;

fn effects() -> Vec<StyleEffect> {
    let mut effects = vec![
        StyleEffect::DropShadow(Shadow {
            size: 2.25,
            spread: 1.5,
            distance: 9.7,
            ..Default::default()
        }),
        StyleEffect::InnerShadow(Shadow {
            size: 2.25,
            spread: 1.5,
            distance: 9.7,
            ..Default::default()
        }),
        StyleEffect::OuterGlow(Glow {
            size: 2.25,
            spread: 1.5,
            ..Default::default()
        }),
        StyleEffect::InnerGlow(Glow {
            size: 2.25,
            spread: 1.5,
            ..Default::default()
        }),
        StyleEffect::InnerGlow(Glow {
            size: 2.25,
            spread: 1.5,
            center: true,
            ..Default::default()
        }),
        StyleEffect::Satin(Satin {
            size: 2.25,
            distance: 9.7,
            invert: true,
            ..Default::default()
        }),
        StyleEffect::ColorOverlay(Overlay::default()),
        StyleEffect::GradientOverlay(Overlay {
            fill: Fill::Gradient {
                gradient: GradientKind::Radial,
                start: [7.0, 2.0],
                end: [900.0, 16.0],
                stops: vec![
                    GradientStop {
                        position: 0.0,
                        color: [0.1, 0.3, 0.7, 0.2],
                    },
                    GradientStop {
                        position: 1.0,
                        color: [0.8, 0.5, 0.2, 0.9],
                    },
                ],
            },
            ..Default::default()
        }),
        StyleEffect::PatternOverlay(Overlay {
            fill: Fill::Pattern {
                width: 2,
                height: 1,
                rgba: vec![0.1, 0.3, 0.6, 0.4, 0.8, 0.5, 0.2, 0.9],
                origin: [-3.5, 1.25],
            },
            ..Default::default()
        }),
    ];
    for kind in [
        BevelKind::Inner,
        BevelKind::Outer,
        BevelKind::Emboss,
        BevelKind::Pillow,
    ] {
        effects.push(StyleEffect::Bevel(Bevel {
            kind,
            size: 2.25,
            soften: 1.5,
            down: true,
            ..Default::default()
        }));
    }
    for position in [
        StrokePosition::Inside,
        StrokePosition::Center,
        StrokePosition::Outside,
    ] {
        effects.push(StyleEffect::Stroke(Stroke {
            position,
            size: 2.75,
            ..Default::default()
        }));
    }
    let disabled: Vec<_> = effects
        .iter()
        .cloned()
        .map(|mut effect| {
            let enabled = match &mut effect {
                StyleEffect::DropShadow(s) | StyleEffect::InnerShadow(s) => &mut s.enabled,
                StyleEffect::OuterGlow(s) | StyleEffect::InnerGlow(s) => &mut s.enabled,
                StyleEffect::Overlay(s)
                | StyleEffect::ColorOverlay(s)
                | StyleEffect::GradientOverlay(s)
                | StyleEffect::PatternOverlay(s) => &mut s.enabled,
                StyleEffect::Stroke(s) => &mut s.enabled,
                StyleEffect::Satin(s) => &mut s.enabled,
                StyleEffect::Bevel(s) => &mut s.enabled,
            };
            *enabled = false;
            effect
        })
        .collect();
    effects.extend(disabled);
    effects.push(StyleEffect::Bevel(Bevel {
        kind: BevelKind::Emboss,
        size: 0.0,
        soften: 0.0,
        highlight_opacity: 0.0,
        shadow_opacity: 0.0,
        ..Default::default()
    }));
    effects.push(StyleEffect::OuterGlow(Glow {
        size: 0.0,
        opacity: 0.0,
        ..Default::default()
    }));
    effects
}

#[test]
fn bevel_plane_count_includes_both_lighting_planes_per_enabled_side() {
    for (kind, count) in [
        (BevelKind::Inner, 2),
        (BevelKind::Outer, 2),
        (BevelKind::Emboss, 4),
        (BevelKind::Pillow, 4),
    ] {
        for enabled in [true, false] {
            let styles = LayerStyles {
                effects: vec![StyleEffect::Bevel(Bevel {
                    kind,
                    enabled,
                    size: 0.0,
                    soften: 0.0,
                    highlight_opacity: 0.0,
                    shadow_opacity: 0.0,
                    ..Default::default()
                })],
                ..Default::default()
            };
            assert_eq!(
                StylesGpu::plane_count(&styles),
                if enabled { count } else { 0 }
            );
        }
    }
}

fn raster(e: Extent) -> Raster {
    let mut r = Raster::new(e, 4, Depth::F32, 0.0);
    r.edit_region(Rect::of_extent(e), 1, |x, y, p| {
        *p = [
            0.6,
            0.2,
            0.4,
            if x % 17 < 11 && y > 1 {
                (x % 5) as f32 * 0.2
            } else {
                0.0
            },
        ];
    })
    .unwrap();
    r
}

#[test]
fn exact_style_planes_and_fractional_geometry() {
    let gpu = GpuCompositor::new().expect("M5-31 needs a GPU");
    let r = ResidentRenderer::new(&gpu).unwrap();
    let kernels = StylesGpu::new(&r.device).unwrap();
    let e = Extent::new(33, 17);
    let source = raster(e);
    let pixels: Vec<f32> = (0..e.height)
        .flat_map(|y| {
            (0..e.width).flat_map({
                let source = &source;
                move |x| source.pixel(x, y)
            })
        })
        .collect();
    let input = r
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("style parity"),
            contents: bytemuck::cast_slice(&pixels),
            usage: wgpu::BufferUsages::STORAGE,
        });
    for scale in [0.0, 0.5, 1.0, 1.75] {
        for effect in effects() {
            let exact = !matches!(
                effect,
                StyleEffect::Bevel(_) | StyleEffect::GradientOverlay(_)
            );
            let styles = LayerStyles {
                effects: vec![effect],
                scale,
            };
            let cpu =
                crate::render::styles::render(&source, &styles, GlobalLight::default()).unwrap();
            let got = kernels
                .render(
                    &r.device,
                    &r.queue,
                    &input,
                    e,
                    &styles,
                    GlobalLight::default(),
                    [0, 0],
                )
                .unwrap();
            assert_eq!(got.len(), cpu.len());
            assert_eq!(got.len(), StylesGpu::plane_count(&styles));
            for (got, want) in got.iter().zip(cpu) {
                assert_eq!(
                    (got.mode, got.opacity, got.outside, got.stroke),
                    (want.mode, want.opacity, want.outside, want.stroke)
                );
                let data =
                    gpu_core::read_buffer(&r.device, &r.queue, &got.pixels, 0, got.pixels.size())
                        .unwrap();
                let data: &[f32] = bytemuck::cast_slice(&data);
                for y in 0..e.height {
                    for x in 0..e.width {
                        for (c, want) in want.raster.pixel(x, y).iter().enumerate() {
                            let got = data[((y * e.width + x) * 4) as usize + c];
                            if exact {
                                assert_eq!(
                                    got.to_bits(),
                                    want.to_bits(),
                                    "{styles:?} at {x}/{y}/{c}"
                                );
                            } else {
                                assert!((got - want).abs() <= 1e-4, "{styles:?}: {got} != {want}");
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn every_effect_viewport_halo_l0_l2_matches_full_cpu() {
    let gpu = GpuCompositor::new().expect("M5-31 needs a GPU");
    let e = Extent::new(1101, 19);
    for effect in effects() {
        let mut state = DocState::new(e, Depth::F32);
        let mut layer = Layer::new("styled", LayerKind::Pixel(raster(e)));
        layer.props.styles = LayerStyles {
            effects: vec![effect],
            scale: 1.25,
        };
        layer.props.opacity = 0.65;
        state.root.push(Arc::new(layer));
        let doc = Document::new(state);
        let cpu = Compositor::new(64 << 20);
        let mut resident = ResidentRenderer::new(&gpu).unwrap();
        for level in [0, 2] {
            let (le, expected) = cpu.render_level_rgba(&doc, level).unwrap();
            for x in [250, 256] {
                let view = Rect::new(x, 0, x + 9, i64::from(le.height));
                resident.render_viewport(&doc, level, view, 0).unwrap();
                resident.wait_for_specializations();
                resident.invalidate();
                resident.render_viewport(&doc, level, view, 0).unwrap();
                let st = &resident.levels[&level];
                let bytes = gpu_core::read_buffer(
                    &resident.device,
                    &resident.queue,
                    &st.out,
                    0,
                    st.out.size(),
                )
                .unwrap();
                let values: &[f32] = bytemuck::cast_slice(&bytes);
                for y in view.y0..view.y1 {
                    for x in view.x0..view.x1 {
                        let i = ((y - st.region.y0) * st.region.width() + x - st.region.x0)
                            as usize
                            * 4;
                        let j = (y as u32 * le.width + x as u32) as usize * 4;
                        for c in 0..4 {
                            let want = if c < 3 {
                                expected[j + c] * expected[j + 3]
                            } else {
                                expected[j + 3]
                            };
                            assert!(
                                (values[i + c] - want).abs() <= 1e-4,
                                "L{level} at {x}/{y}/{c}: {} != {want}",
                                values[i + c]
                            );
                        }
                    }
                }
            }
        }
    }
}
