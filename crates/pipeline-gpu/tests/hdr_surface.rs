#![cfg(target_os = "macos")]
#[path = "../../image-core/tests/common/mod.rs"]
mod common;
use engine_api::{jobs::CancellationToken, recipe::DevelopSettings};
use image_core::{Headroom, PixelRect, RenderOutput, Renderer, RendererConfig, TileCache};
use objc2_core_foundation::{CFDictionary, CFNumber, CFType};
use objc2_io_surface::{
    IOSurfaceLockOptions, IOSurfaceRef, kIOSurfaceBytesPerElement, kIOSurfaceHeight,
    kIOSurfacePixelFormat, kIOSurfaceWidth,
};
use pipeline_gpu::{GpuContext, GpuStageOp};
use std::sync::Arc;

fn fnv(bytes: impl IntoIterator<Item = u8>) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in bytes {
        h = (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3);
    }
    h
}

fn surface(
    w: u32,
    h: u32,
    bytes: i64,
    format: [u8; 4],
) -> objc2_core_foundation::CFRetained<IOSurfaceRef> {
    let numbers = [
        CFNumber::new_i64(i64::from(w)),
        CFNumber::new_i64(i64::from(h)),
        CFNumber::new_i64(bytes),
        CFNumber::new_i64(i64::from(u32::from_be_bytes(format))),
    ];
    // SAFETY: immutable CFString keys, CFNumber values and retained Copy-rule surface.
    unsafe {
        let keys: [&CFType; 4] = [
            kIOSurfaceWidth.as_ref(),
            kIOSurfaceHeight.as_ref(),
            kIOSurfaceBytesPerElement.as_ref(),
            kIOSurfacePixelFormat.as_ref(),
        ];
        let values: Vec<&CFType> = numbers.iter().map(|n| (**n).as_ref()).collect();
        let dict = CFDictionary::from_slices(&keys, &values);
        IOSurfaceRef::new(dict.as_opaque()).unwrap()
    }
}

fn read(s: &IOSurfaceRef, w: u32, h: u32, bpe: usize) -> Vec<u8> {
    // SAFETY: rendering waited for completion; reads stay in the locked allocation.
    unsafe {
        s.lock(IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut());
        let base = s.base_address().as_ptr() as *const u8;
        let stride = s.bytes_per_row();
        let mut out = Vec::new();
        for y in 0..h as usize {
            out.extend_from_slice(std::slice::from_raw_parts(
                base.add(y * stride),
                w as usize * bpe,
            ));
        }
        s.unlock(IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut());
        out
    }
}

fn sdr_cases() -> Vec<DevelopSettings> {
    let mut a = DevelopSettings::default();
    a.tone.exposure = 1.7;
    a.tone.highlights = -30.0;
    let mut b = DevelopSettings::default();
    b.color.vibrance = 40.0;
    b.tone.shadows = 25.0;
    vec![DevelopSettings::default(), a, b]
}

/// SDR fingerprints recorded on the pre-M2-22 tree (commit c3232ee) with
/// this exact probe: the EDR work must not move a single SDR byte.
/// `(cpu tiles, GPU resident tiles, GPU RGBA8 surface)` per case. The CPU
/// reference is deterministic on this platform; the GPU values are asserted
/// on the recording adapter and reported elsewhere.
const PRE_EDR: [(u64, u64, u64); 3] = [
    (0x44fae39b8cad1e, 0x50eac09770927b7a, 0xdbf920dcf5887ebe),
    (0xcc470d33eeb4d28a, 0x9888e139c4c750cd, 0xfc167ff4409e51d9),
    (0x39f4bd02fec83fd1, 0x9f14a1d177fda67c, 0x6955ddd3131422e4),
];
const PRE_EDR_ADAPTER: &str = "Apple M4";

fn gpu_renderer() -> (Arc<GpuContext>, Renderer) {
    let ctx = Arc::new(GpuContext::new().unwrap());
    let gpu = Arc::new(GpuStageOp::new(ctx.clone()));
    let config = RendererConfig::default();
    let r = Renderer::with_ops(
        gpu,
        Arc::new(TileCache::new(config.cache_budget_bytes)),
        config,
    );
    (ctx, r)
}

#[test]
fn sdr_output_is_bit_identical_to_pre_edr_fingerprints() {
    let (w, h) = (300, 211);
    let image = common::synthetic(2201, w, h, common::RGGB, [0, 0, w, h]);
    let cpu = Renderer::new(RendererConfig::default());
    let (ctx, r) = gpu_renderer();
    let same_adapter = ctx.adapter_info.name == PRE_EDR_ADAPTER;
    for (i, s) in sdr_cases().iter().enumerate() {
        let rect = PixelRect::full(image.level_extent(0));
        let tiles = cpu
            .render_region_as(&image, s, 0, rect, RenderOutput::Display)
            .unwrap();
        let c = fnv(tiles
            .iter()
            .flat_map(|t| t.samples::<u8>().unwrap().to_vec()));
        let tiles = r
            .render_region_as(&image, s, 0, rect, RenderOutput::Display)
            .unwrap();
        let g = fnv(tiles
            .iter()
            .flat_map(|t| t.samples::<u8>().unwrap().to_vec()));
        let surf = surface(w, h, 4, *b"RGBA");
        r.render_surface(&image, s, 0, surf.id(), &CancellationToken::new())
            .unwrap()
            .unwrap();
        let sf = fnv(read(&surf, w, h, 4));
        eprintln!(
            "case={i} cpu={c:#x} gpu_tiles={g:#x} gpu_surface={sf:#x} ({})",
            ctx.adapter_info.name
        );
        #[cfg(target_arch = "aarch64")]
        assert_eq!(c, PRE_EDR[i].0, "CPU SDR case {i} changed");
        if same_adapter {
            assert_eq!(
                (g, sf),
                (PRE_EDR[i].1, PRE_EDR[i].2),
                "GPU SDR case {i} changed"
            );
        }
    }
}

fn half_pixels(bytes: &[u8]) -> Vec<f32> {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| half::f16::from_le_bytes(*b).to_f32())
        .collect()
}

fn bright() -> DevelopSettings {
    let mut s = DevelopSettings::default();
    s.tone.exposure = 2.0;
    s
}

#[test]
fn edr_surface_keeps_values_above_sdr_white_without_pixel_readback() {
    let (w, h) = (300, 211);
    let image = common::synthetic(2202, w, h, common::RGGB, [0, 0, w, h]);
    let cpu = Renderer::new(RendererConfig::default());
    let (_, r) = gpu_renderer();
    let s = bright();
    let headroom = 4.0;
    let output = RenderOutput::DisplayLinear(Headroom::new(headroom));
    let rect = PixelRect::full(image.level_extent(0));
    let reference = cpu.render_region_as(&image, &s, 0, rect, output).unwrap();
    let surf = surface(w, h, 8, *b"RGhA");
    let hist = r
        .render_surface_as(&image, &s, 0, surf.id(), output, &CancellationToken::new())
        .unwrap()
        .expect("resident-capable recipe");
    let px = half_pixels(&read(&surf, w, h, 8));
    let mut above = 0;
    let mut max_err = 0.0f32;
    let mut expected_hist = [[0u32; 256]; 4];
    for t in &reference {
        let l = t.layout();
        let n = l.plane_len();
        let v = t.samples::<f32>().unwrap();
        let (ox, oy) = t.coord().pixel_origin(engine_api::tile::TILE_SIZE);
        for y in 0..l.extent.height as usize {
            for x in 0..l.extent.width as usize {
                let i = y * l.extent.width as usize + x;
                let o = ((oy as usize + y) * w as usize + ox as usize + x) * 4;
                let mut enc = [0u32; 3];
                for c in 0..3 {
                    let (a, b) = (px[o + c], v[c * n + i]);
                    assert!((0.0..=headroom).contains(&a), "{a}");
                    max_err = max_err.max((a - b).abs() / b.max(0.05));
                    if a > 1.0 {
                        above += 1;
                    }
                    enc[c] =
                        (pipeline_cpu::srgb_oetf(b.clamp(0.0, 1.0)) * 255.0 + 0.5).floor() as u32;
                    expected_hist[c][enc[c] as usize] += 1;
                }
                expected_hist[3][((54 * enc[0] + 183 * enc[1] + 19 * enc[2]) >> 8) as usize] += 1;
                assert_eq!(px[o + 3], 1.0);
            }
        }
    }
    eprintln!("EDR: {above} samples above SDR white, max relative error {max_err:e}");
    assert!(above > 1000, "{above}");
    // f16 storage (2^-11 relative) plus GPU/CPU transcendental differences.
    assert!(max_err < 5e-3, "{max_err}");
    // Histogram of the encoded SDR range: equal up to boundary rounding.
    for c in 0..4 {
        let moved: u32 = hist[c]
            .iter()
            .zip(expected_hist[c].iter())
            .map(|(a, b)| a.abs_diff(*b))
            .sum();
        assert!(moved <= (w * h) / 50, "channel {c}: {moved}");
        assert_eq!(hist[c].iter().sum::<u32>(), w * h);
    }
    assert!(hist[3][255] > 0, "EDR highlights land in the top bin");
}

#[test]
fn edr_tiles_match_cpu_reference_and_unit_headroom_is_sdr_linear() {
    let (w, h) = (257, 190);
    let image = common::synthetic(2203, w, h, common::RGGB, [0, 0, w, h]);
    let cpu = Renderer::new(RendererConfig::default());
    let (_, r) = gpu_renderer();
    let s = bright();
    let rect = PixelRect::full(image.level_extent(0));
    for headroom in [1.0, 2.5, 16.0] {
        let output = RenderOutput::DisplayLinear(Headroom::new(headroom));
        let a = r.render_region_as(&image, &s, 0, rect, output).unwrap();
        let b = cpu.render_region_as(&image, &s, 0, rect, output).unwrap();
        let mut max = 0.0f32;
        for (a, b) in a.iter().zip(&b) {
            for (x, y) in a
                .samples::<f32>()
                .unwrap()
                .iter()
                .zip(b.samples::<f32>().unwrap())
            {
                max = max.max((x - y).abs() / y.max(0.05));
            }
        }
        // Resident checkpoints are f16 (the SceneLinear tolerance, 5e-3).
        assert!(max < 5e-3, "headroom {headroom}: {max}");
    }
    // Headroom 1: the SDR picture, linear. Encoded it is the SDR output
    // within the SDR path's dither (±1 code value, rounding).
    let lin = cpu
        .render_region_as(
            &image,
            &s,
            0,
            rect,
            RenderOutput::DisplayLinear(Headroom::SDR),
        )
        .unwrap();
    let sdr = cpu
        .render_region_as(&image, &s, 0, rect, RenderOutput::Display)
        .unwrap();
    for (l, e) in lin.iter().zip(&sdr) {
        for (x, y) in l
            .samples::<f32>()
            .unwrap()
            .iter()
            .zip(e.samples::<u8>().unwrap())
        {
            assert!(*x <= 1.0);
            let enc = pipeline_cpu::srgb_oetf(*x) * 255.0;
            assert!((enc - f32::from(*y)).abs() <= 1.0, "{enc} {y}");
        }
    }
}

#[test]
fn surface_format_must_match_output() {
    let (w, h) = (64, 48);
    let image = common::synthetic(2204, w, h, common::RGGB, [0, 0, w, h]);
    let (_, r) = gpu_renderer();
    let s = DevelopSettings::default();
    let cancel = CancellationToken::new();
    let rgba8 = surface(w, h, 4, *b"RGBA");
    let half = surface(w, h, 8, *b"RGhA");
    let edr = RenderOutput::DisplayLinear(Headroom::new(2.0));
    assert!(
        r.render_surface_as(&image, &s, 0, rgba8.id(), edr, &cancel)
            .is_err()
    );
    assert!(
        r.render_surface_as(&image, &s, 0, half.id(), RenderOutput::Display, &cancel)
            .is_err()
    );
    assert!(
        r.render_surface_as(&image, &s, 0, half.id(), RenderOutput::SceneLinear, &cancel)
            .is_err()
    );
    assert!(
        r.render_surface_as(&image, &s, 0, half.id(), edr, &cancel)
            .unwrap()
            .is_some()
    );
}
