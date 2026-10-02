use filters::adjust::Adjustment;
#[test]
fn eng3_photo_one_ulp_sensitivity() {
    let rgb = [2., -0.2627_f32 / 0.678, 1e-6, 0.7];
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
    assert!(gap <= 1e-3);
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
    let adjust = include_str!("../src/shaders/adjust.wgsl")
        .replace("luma(rgb)/y", "bitcast<f32>(bitcast<u32>(y)+1u)/y")
        .replace(
            "let target = luma(rgb);",
            "let target = bitcast<f32>(bitcast<u32>(y)+1u);",
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
