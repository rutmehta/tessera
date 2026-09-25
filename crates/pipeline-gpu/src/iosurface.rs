//! Same-device IOSurface import. All foreign API access is isolated here.
#![allow(unsafe_code)]
use crate::GpuContext;
use engine_api::{EngineError, EngineResult};

/// Imports a retained RGBA8 IOSurface as a writable texture on this context's
/// Metal device. The texture retains the surface storage until GPU use ends.
#[cfg(target_os = "macos")]
pub fn write_to_iosurface(ctx: &GpuContext, id: u32) -> EngineResult<wgpu::Texture> {
    use objc2_io_surface::IOSurfaceRef;
    use objc2_metal::{
        MTLDevice, MTLPixelFormat, MTLStorageMode, MTLTextureDescriptor, MTLTextureType,
        MTLTextureUsage,
    };
    let surface = IOSurfaceRef::lookup(id)
        .ok_or_else(|| EngineError::invalid("IOSurface", "id not found"))?;
    if surface.bytes_per_element() != 4
        || surface.pixel_format() != u32::from_be_bytes(*b"RGBA")
        || surface.plane_count() != 0
    {
        return Err(EngineError::invalid(
            "IOSurface",
            "expected non-planar RGBA8",
        ));
    }
    let width = u32::try_from(surface.width())
        .map_err(|_| EngineError::invalid("IOSurface", "width overflow"))?;
    let height = u32::try_from(surface.height())
        .map_err(|_| EngineError::invalid("IOSurface", "height overflow"))?;
    if width == 0
        || height == 0
        || width > ctx.device.limits().max_texture_dimension_2d
        || height > ctx.device.limits().max_texture_dimension_2d
    {
        return Err(EngineError::invalid(
            "IOSurface",
            "dimensions exceed device limits",
        ));
    }
    let desc = MTLTextureDescriptor::new();
    desc.setTextureType(MTLTextureType::Type2D);
    desc.setPixelFormat(MTLPixelFormat::RGBA8Unorm);
    // SAFETY: nonzero dimensions validated against the device limits above.
    unsafe {
        desc.setWidth(width as usize);
        desc.setHeight(height as usize);
    }
    desc.setStorageMode(MTLStorageMode::Shared);
    desc.setUsage(MTLTextureUsage::ShaderWrite | MTLTextureUsage::ShaderRead);
    // SAFETY: obtain the exact backing Metal device; descriptors agree in
    // format, extent, layers and mip count. No concurrent native mutation.
    let hal = unsafe { ctx.device.as_hal::<wgpu::hal::api::Metal>() }
        .ok_or_else(|| EngineError::internal("not a Metal device"))?;
    let raw = hal
        .raw_device()
        .newTextureWithDescriptor_iosurface_plane(&desc, &surface, 0)
        .ok_or_else(|| EngineError::internal("Metal IOSurface texture creation failed"))?;
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    // SAFETY: raw is retained, created on this device and matches descriptor.
    let texture = unsafe {
        wgpu::hal::metal::Device::texture_from_raw(
            raw,
            wgpu::TextureFormat::Rgba8Unorm,
            MTLTextureType::Type2D,
            1,
            1,
            wgpu::hal::CopyExtent {
                width,
                height,
                depth: 1,
            },
            None,
        )
    };
    drop(hal);
    // SAFETY: Metal textures require no layout transition; existing surface
    // bytes are initialized. STORAGE_READ_WRITE preserves untouched regions.
    Ok(unsafe {
        ctx.device.create_texture_from_hal::<wgpu::hal::api::Metal>(
            texture,
            &wgpu::TextureDescriptor {
                label: Some("develop IOSurface"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::STORAGE_BINDING,
                view_formats: &[],
            },
            wgpu::TextureUses::STORAGE_READ_WRITE,
        )
    })
}

#[cfg(not(target_os = "macos"))]
pub fn write_to_iosurface(_ctx: &GpuContext, _id: u32) -> EngineResult<wgpu::Texture> {
    Err(EngineError::invalid("IOSurface", "requires macOS"))
}
