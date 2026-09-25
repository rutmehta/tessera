#![cfg(target_os = "macos")]
#[path = "../../image-core/tests/common/mod.rs"]
mod common;
use color_mgmt::{Builtin, Registry};
use engine_api::{color::IccProfileHandle, jobs::CancellationToken, recipe::DevelopSettings};
use image_core::{PixelRect, RendererConfig};
use objc2_core_foundation::{CFDictionary, CFNumber, CFType};
use objc2_io_surface::{
    IOSurfaceLockOptions, IOSurfaceRef, kIOSurfaceBytesPerElement, kIOSurfaceHeight,
    kIOSurfacePixelFormat, kIOSurfaceWidth,
};
use pipeline_cpu::{OutputContext, OutputTarget};
use pipeline_gpu::{GpuContext, GpuManagedOutput, ManagedRenderer};
use std::sync::Arc;

#[test]
fn proof_surface_matches_tile_output_without_pixel_readback() {
    let device = Arc::new(GpuContext::new().unwrap());
    let mut registry = Registry::new();
    let display = registry.builtin(Builtin::DisplayP3).unwrap();
    let proof = registry.builtin(Builtin::Srgb).unwrap();
    let mut settings = DevelopSettings::default();
    settings.output.proof_profile = Some(IccProfileHandle::from_profile_bytes(proof.icc_bytes()));
    let output = Arc::new(
        GpuManagedOutput::new(
            device,
            &settings,
            &mut OutputContext {
                registry: &mut registry,
                target: OutputTarget::Display(&display),
                proof: Some(&proof),
                options: Default::default(),
            },
        )
        .unwrap(),
    );
    let renderer = ManagedRenderer::new(output, RendererConfig::default());
    let (w, h) = (259, 35);
    let image = common::synthetic(916, w, h, common::RGGB, [0, 0, w, h]);
    let expected = renderer
        .render_region(&image, &settings, 0, PixelRect::full(image.level_extent(0)))
        .unwrap();
    let numbers = [
        CFNumber::new_i64(i64::from(w)),
        CFNumber::new_i64(i64::from(h)),
        CFNumber::new_i64(4),
        CFNumber::new_i64(i64::from(u32::from_be_bytes(*b"RGBA"))),
    ];
    // SAFETY: immutable CFString keys, CFNumber values and retained Copy-rule surface.
    let surface = unsafe {
        let keys: [&CFType; 4] = [
            kIOSurfaceWidth.as_ref(),
            kIOSurfaceHeight.as_ref(),
            kIOSurfaceBytesPerElement.as_ref(),
            kIOSurfacePixelFormat.as_ref(),
        ];
        let values: Vec<&CFType> = numbers.iter().map(|n| (**n).as_ref()).collect();
        let dict = CFDictionary::from_slices(&keys, &values);
        IOSurfaceRef::new(dict.as_opaque()).unwrap()
    };
    let before = renderer.stats();
    assert!(
        renderer
            .render_to_surface(
                &image,
                &settings,
                0,
                surface.id(),
                &CancellationToken::new()
            )
            .unwrap()
    );
    let after = renderer.stats();
    assert_eq!(after.pixel_readback_bytes, before.pixel_readback_bytes);
    assert_eq!(after.readbacks, before.readbacks);
    assert_eq!(after.submissions, before.submissions + 1);
    // SAFETY: rendering waits for GPU completion, lock establishes host visibility,
    // and every read stays within the retained surface's allocation/row stride.
    unsafe {
        let lock = IOSurfaceLockOptions::ReadOnly;
        assert_eq!(surface.lock(lock, std::ptr::null_mut()), 0);
        let bytes = std::slice::from_raw_parts(
            surface.base_address().as_ptr().cast::<u8>(),
            surface.alloc_size(),
        );
        for tile in &expected {
            let (ox, oy) = tile.coord().pixel_origin(engine_api::tile::TILE_SIZE);
            let n = tile.layout().plane_len();
            let values = tile.samples::<u8>().unwrap();
            for y in 0..tile.layout().extent.height {
                for x in 0..tile.layout().extent.width {
                    let i = (y * tile.layout().extent.width + x) as usize;
                    let dst = (oy + y) as usize * surface.bytes_per_row() + (ox + x) as usize * 4;
                    for c in 0..3 {
                        assert_eq!(bytes[dst + c], values[c * n + i]);
                    }
                    assert_eq!(bytes[dst + 3], 255);
                }
            }
        }
        assert_eq!(surface.unlock(lock, std::ptr::null_mut()), 0);
    }
}
