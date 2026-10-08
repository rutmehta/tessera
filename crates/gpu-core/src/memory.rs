//! Device memory diagnostics. All foreign API access is isolated here.
#![allow(unsafe_code)]

/// Bytes the Metal device has allocated for this process's resources
/// (`MTLDevice.currentAllocatedSize`): every buffer, texture, pipeline and
/// internal staging copy, whether or not a budget counts it. A diagnostic
/// for measuring true export footprints (ENG-14); None off Metal. The
/// count is process-wide, so measurements must not overlap other GPU work.
#[cfg(target_os = "macos")]
pub fn device_allocated_bytes(device: &wgpu::Device) -> Option<u64> {
    use objc2_metal::MTLDevice;
    // SAFETY: only reads a counter of the backing Metal device; no wgpu
    // state is touched.
    let hal = unsafe { device.as_hal::<wgpu::hal::api::Metal>() }?;
    Some(hal.raw_device().currentAllocatedSize() as u64)
}

#[cfg(not(target_os = "macos"))]
pub fn device_allocated_bytes(_device: &wgpu::Device) -> Option<u64> {
    None
}
