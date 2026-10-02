use filters::adjust::Adjustment;
#[test]
fn eng3_photo_one_ulp_sensitivity() {
    // Reproduces the 6.1035156e-5 maximum RGB response to green.next_up().
    // See tools/orchestrate/wp/ENG-3/HANDOFF.md (ENG-3e) for Y, rho and D.
    const WORST_CASE_RGBA: [f32; 4] = [2., -0.2627_f32 / 0.678, 1e-6, 0.7];
    let rgb = WORST_CASE_RGBA;
    let mut next = rgb;
    next[1] = next[1].next_up();
    let op = Adjustment::PhotoFilter {
        colour: [0.5, 1., 1.],
        density: 1.,
        preserve_luminosity: true,
    };
    let mut a = [rgb; 9];
    let mut b = [next; 9];
    op.apply(&mut a).unwrap();
    op.apply(&mut b).unwrap();
    let gap = a
        .iter()
        .flatten()
        .zip(b.iter().flatten())
        .map(|(a, b)| (a - b).abs())
        .fold(0f32, f32::max);
    eprintln!("ENG3 photo one-ulp gap={gap}");
    // The amended continuous denominator changes with rho: unlike the old
    // constant floor, its derivative contributes to this one-ulp response.
    // Check the prescribed formula independently as well as bounding the gap.
    for (input, output) in [(rgb, a[0]), (next, b[0])] {
        let filtered = [input[0] * 0.5, input[1], input[2]];
        let y = (0.2627 * filtered[0] + 0.678 * filtered[1] + 0.0593 * filtered[2]) as f64;
        let absolute = 0.2627 * filtered[0].abs() as f64
            + 0.678 * filtered[1].abs() as f64
            + 0.0593 * filtered[2].abs() as f64;
        let target = (0.2627 * input[0] + 0.678 * input[1] + 0.0593 * input[2]) as f64;
        let d = y
            .abs()
            .max(0.001 * (1. - y.abs() / absolute / 0.25).clamp(0., 1.));
        let gain = 1. + (target - y) / d.copysign(y);
        for c in 0..3 {
            assert!((output[c] as f64 - filtered[c] as f64 * gain).abs() < 3e-5);
        }
    }
    assert!(gap <= 1e-4);
}
#[path = "../../pipeline-gpu/tests/support/eng3_compute.rs"]
mod compute;
#[test]
fn eng3_photo_zero_delta_vs_one_ulp() {
    let rgb = [1., -0.2627_f32 / 0.678, 1e-6, 0.7];
    let y = 0.2627 * rgb[0] + 0.678 * rgb[1] + 0.0593 * rgb[2];
    assert!(y > 0. && y < 1e-3);
    let mut cpu = [rgb];
    Adjustment::PhotoFilter {
        colour: [1.; 3],
        density: 1.,
        preserve_luminosity: true,
    }
    .apply(&mut cpu)
    .unwrap();
    assert_eq!(cpu[0], rgb);
    // Only the target luminance is perturbed. This executes the real Photo
    // Filter recombination rather than a test copy of the gain formula.
    let src = include_str!("../src/shaders/adjust.wgsl");
    assert!(src.contains("luma(rgb)/y"));
    let src = src.replace("luma(rgb)/y", "bitcast<f32>(bitcast<u32>(y)+1u)/y");
    assert!(src.contains("let mapped_luma = luma(rgb);"));
    let adjust = src.replace(
        "let mapped_luma = luma(rgb);",
        "let mapped_luma = bitcast<f32>(bitcast<u32>(y)+1u);",
    );
    let source = format!(
        "{}\n{adjust}\n@compute @workgroup_size(1) fn eng3_regression() {{ dst[0]=vec4(adjustment(28u,vec3(p[40],p[41],p[42])),p[43]); }}",
        include_str!("../src/shaders/filters.wgsl")
    );
    let mut p = [0.; 44];
    p[32..35].fill(1.);
    p[35] = 1.;
    p[36] = 1.;
    p[40..44].copy_from_slice(&rgb);
    let gpu = compute::run(source, &p, 3, 2);
    let gap = (0..3)
        .map(|c| (gpu[c] - cpu[0][c]).abs())
        .fold(0f32, f32::max);
    eprintln!(
        "ENG3 photo zero/ulp L={y} delta={} gap={gap}",
        y.next_up() - y
    );
    assert!(gap <= 1e-8);
    assert_eq!(gpu[3], rgb[3]);
}
#[test]
fn eng3_photo_full_gpu_parity_and_alpha() {
    use compositor::{
        geom::Rect,
        raster::{Depth, Raster},
    };
    use engine_api::tile::Extent;
    use filters::{Effect, Filter, FilterParams, gpu::GpuFilters};
    let gpu = GpuFilters::new().unwrap();
    let p = FilterParams {
        amount: 1.,
        adjust: Adjustment::PhotoFilter {
            colour: [0.5, 1., 1.],
            density: 1.,
            preserve_luminosity: true,
        },
        ..Default::default()
    };
    for sign in [-1., 1.] {
        let mut input = Raster::new(Extent::new(3, 3), 4, Depth::F32, 0.);
        input
            .edit_region(Rect::of_extent(input.extent()), 1, |_, _, p| {
                *p = [sign * 2., sign * (-0.2627_f32 / 0.678), sign * 1e-6, 0.7]
            })
            .unwrap();
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let cpu = Effect::Adjust.apply(&input, &p, &cancel).unwrap();
        let out = gpu.apply(Effect::Adjust, &input, &p, &cancel).unwrap();
        let gap = (0..3)
            .map(|c| (cpu.pixel(1, 1)[c] - out.pixel(1, 1)[c]).abs())
            .fold(0f32, f32::max);
        eprintln!("ENG3 photo full GPU sign={sign} gap={gap}");
        assert!(gap <= 1e-4);
        assert_eq!(out.pixel(1, 1)[3].to_bits(), input.pixel(1, 1)[3].to_bits());
    }
}

#[test]
fn eng3b_photo_positive_shadows_preserve_luminance() {
    use compositor::{
        geom::Rect,
        raster::{Depth, Raster},
    };
    use engine_api::tile::Extent;
    use filters::{Effect, Filter, FilterParams, gpu::GpuFilters};
    let p = FilterParams {
        amount: 1.,
        adjust: Adjustment::PhotoFilter {
            colour: [0.5, 0.8, 1.],
            density: 1.,
            preserve_luminosity: true,
        },
        ..Default::default()
    };
    let mut input = Raster::new(Extent::new(257, 1), 4, Depth::F32, 0.);
    input
        .edit_region(Rect::of_extent(input.extent()), 1, |x, _, p| {
            let v = x as f32 * 0.002 / 256.;
            *p = [v, v, v, 0.7];
        })
        .unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let cpu = Effect::Adjust.apply(&input, &p, &cancel).unwrap();
    let gpu = GpuFilters::new()
        .unwrap()
        .apply(Effect::Adjust, &input, &p, &cancel)
        .unwrap();
    for x in 0..257 {
        let a = cpu.pixel(x, 0);
        let b = gpu.pixel(x, 0);
        let y = 0.2627 * a[0] + 0.678 * a[1] + 0.0593 * a[2];
        assert!((y - input.pixel(x, 0)[0]).abs() < 1e-9, "x={x} y={y}");
        for c in 0..3 {
            assert!((a[c] - b[c]).abs() < 1e-9);
        }
        assert_eq!(a[3], 0.7);
        assert_eq!(b[3], 0.7);
    }
}

#[test]
fn eng3b_coloured_photo_zero_crossing_documented() {
    use compositor::{
        geom::Rect,
        raster::{Depth, Raster},
    };
    use engine_api::tile::Extent;
    use filters::{Effect, Filter, FilterParams, gpu::GpuFilters};
    let p = FilterParams {
        amount: 1.,
        adjust: Adjustment::PhotoFilter {
            colour: [0.5, 1., 1.],
            density: 1.,
            preserve_luminosity: true,
        },
        ..Default::default()
    };
    let gpu = GpuFilters::new().unwrap();
    let mut red = Vec::new();
    for delta in [-1e-6, 1e-6] {
        let mut input = Raster::new(Extent::new(3, 3), 4, Depth::F32, 0.);
        input
            .edit_region(Rect::of_extent(input.extent()), 1, |_, _, p| {
                *p = [2., -0.2627_f32 / 0.678, delta / 0.0593, 0.7]
            })
            .unwrap();
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let a = Effect::Adjust.apply(&input, &p, &cancel).unwrap();
        let b = gpu.apply(Effect::Adjust, &input, &p, &cancel).unwrap();
        assert!((a.pixel(1, 1)[0] - b.pixel(1, 1)[0]).abs() < 1e-4);
        red.push(a.pixel(1, 1)[0]);
    }
    eprintln!("ENG3b coloured photo crossing red={red:?}");
    assert!(red[0] < -250. && red[1] > 250.); // Retained signed-floor discontinuity.
}

#[test]
fn eng3c_photo_chroma_continuity_and_primary_ramps() {
    use compositor::{
        geom::Rect,
        raster::{Depth, Raster},
    };
    use engine_api::tile::Extent;
    use filters::{Effect, Filter, FilterParams, gpu::GpuFilters};
    let params = FilterParams {
        amount: 1.,
        adjust: Adjustment::PhotoFilter {
            colour: [0.5, 0.8, 0.5],
            density: 1.,
            preserve_luminosity: true,
        },
        ..Default::default()
    };
    let gpu = GpuFilters::new().unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    for mode in 0..5 {
        let mut input = Raster::new(Extent::new(257, 1), 4, Depth::F32, 0.);
        input
            .edit_region(Rect::of_extent(input.extent()), 1, |x, _, p| {
                *p = [0., 0., 0., 0.7];
                if mode < 3 {
                    p[mode] = x as f32 * 0.02 / 256.;
                } else {
                    let center = if mode == 3 {
                        0.25
                    } else {
                        0.25 / (2. * 0.2627 - 0.25)
                    };
                    let rho = center + (x as f32 - 128.) * 0.00002;
                    let y = 0.0005;
                    let a = y / rho;
                    p[0] = (a + y) / (2. * 0.2627) / 0.5;
                    p[1] = (y - a) / (2. * 0.678) / 0.8;
                }
            })
            .unwrap();
        let cpu = Effect::Adjust.apply(&input, &params, &cancel).unwrap();
        let metal = gpu.apply(Effect::Adjust, &input, &params, &cancel).unwrap();
        let luma = |p: [f32; 4]| 0.2627 * p[0] + 0.678 * p[1] + 0.0593 * p[2];
        let mut max_step = 0_f32;
        for x in 0..257 {
            let a = cpu.pixel(x, 0);
            let b = metal.pixel(x, 0);
            for c in 0..4 {
                assert!((a[c] - b[c]).abs() < 2e-7);
            }
            assert_eq!(a[3], 0.7);
            assert_eq!(b[3], 0.7);
            if mode < 3 {
                assert!((luma(a) - luma(input.pixel(x, 0))).abs() < 1e-8);
            }
            if x > 0 {
                for out in [&cpu, &metal] {
                    let step = luma(out.pixel(x, 0)) - luma(out.pixel(x - 1, 0));
                    max_step = max_step.max(step.abs());
                    if mode < 3 {
                        assert!(step >= -1e-9);
                    } else {
                        assert!(step.abs() < 1e-6, "mode={mode} jump={step}");
                    }
                }
            }
        }
        eprintln!("ENG3c Photo mode={mode} CPU/Metal max adjacent Y step={max_step:e}");
    }
}

#[path = "../../pipeline-gpu/tests/support/eng3_switch.rs"]
mod switch;
#[test]
fn eng3f_photo_actual_switch_sweeps() {
    use compositor::{
        geom::Rect,
        raster::{Depth, Raster},
    };
    use engine_api::tile::Extent;
    use filters::{Effect, Filter, FilterParams, gpu::GpuFilters};
    let params = FilterParams {
        amount: 1.,
        adjust: Adjustment::PhotoFilter {
            colour: [0.5, 0.8, 0.5],
            density: 1.,
            preserve_luminosity: true,
        },
        ..Default::default()
    };
    // Filtered pixels are (r, g, 0) with r > 0 > g, so 0.2627 r = (A + L) / 2
    // and 0.678 g = (L - A) / 2. The source is (r / 0.5, g / 0.8, 0), hence the
    // target (source luminance) is T = (A + L) + (L - A) / 1.6
    //   = L * (1.625 + 0.375 / rho), with T - L = L * q, q = 0.625 + 0.375 / rho.
    // Floor branch, D >= L, Yout = L + (T - L) * L / D:
    //   d/d rho = -(0.375 L / rho^2) * L / D + L q * L * eps / (k D^2); the two
    //             terms have opposite signs, so |.| <= max(0.375 L / rho^2, q eps / k)
    //   d/d L   = 1 + 2 q L / D <= 1 + 2 q
    // Ratio branch: |d/d rho| = 0.375 L / rho^2 and d/d L = 1 + q, both covered.
    let slopes = |d: &switch::Domain| {
        let q = 0.625 + 0.375 / d.rho_min;
        [
            (0.375 * d.y_max / (d.rho_min * d.rho_min)).max(q * switch::EPSILON / switch::K),
            1. + 2. * q,
        ]
    };
    let gpu = GpuFilters::new().unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    for sweep in switch::sweeps() {
        let wanted = sweep.pixels();
        let n = wanted.len();
        let mut input = Raster::new(Extent::new(n as u32, 1), 4, Depth::F32, 0.);
        input
            .edit_region(Rect::of_extent(input.extent()), 1, |x, _, p| {
                let filtered = wanted[x as usize];
                *p = [filtered[0] / 0.5, filtered[1] / 0.8, 0., 0.7];
            })
            .unwrap();
        // The filtered pixels the operator really conditions (f32 tint products).
        let filtered: Vec<_> = (0..n)
            .map(|i| {
                let p = input.pixel(i as u32, 0);
                [p[0] * 0.5, p[1] * 0.8, 0.]
            })
            .collect();
        let cpu = Effect::Adjust.apply(&input, &params, &cancel).unwrap();
        let metal = gpu.apply(Effect::Adjust, &input, &params, &cancel).unwrap();
        for (backend, out) in [("CPU", cpu), ("Metal", metal)] {
            let outputs: Vec<_> = (0..n)
                .map(|i| {
                    let p = out.pixel(i as u32, 0);
                    assert_eq!(p[3], 0.7);
                    [p[0], p[1], p[2]]
                })
                .collect();
            let measured = sweep.check("Photo Filter", backend, &filtered, &outputs, slopes);
            assert!(measured.switch_step <= measured.max_step);
            // Past the switch the source luminance is preserved exactly.
            let source = input.pixel(n as u32 - 1, 0);
            let target = switch::luma([source[0], source[1], source[2]]);
            assert!((switch::luma(outputs[n - 1]) - target).abs() < 1e-7);
        }
    }
}
