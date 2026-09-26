//! Auxiliary tables grow with user-supplied adjustment LUTs. Check the
//! power-of-two allocation bound, not just the populated byte count.

pub(super) fn validate_aux(samples: usize, limits: &wgpu::Limits) -> engine_api::EngineResult<()> {
    let allocation = (samples as u64)
        .checked_mul(std::mem::size_of::<f32>() as u64)
        .and_then(|bytes| bytes.max(16).checked_next_power_of_two())
        .map(|bytes| bytes.max(256));
    if allocation.is_some_and(|bytes| {
        bytes <= limits.max_buffer_size && bytes <= limits.max_storage_buffer_binding_size
    }) {
        Ok(())
    } else {
        Err(engine_api::EngineError::ResourceExhausted {
            resource: "resident adjustment auxiliary tables exceed device buffer/binding limits"
                .into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auxiliary_allocation_respects_both_device_limits() {
        let limits = wgpu::Limits {
            max_storage_buffer_binding_size: 128 << 20,
            max_buffer_size: 256 << 20,
            ..Default::default()
        };
        assert!(validate_aux(0, &limits).is_ok());
        assert!(validate_aux((128 << 20) / 4, &limits).is_ok());
        assert!(validate_aux((128 << 20) / 4 + 1, &limits).is_err());
        assert!(validate_aux(256_usize.pow(3) * 3, &limits).is_err());
        assert!(validate_aux(usize::MAX, &limits).is_err());
        let limits = wgpu::Limits {
            max_buffer_size: 1024,
            ..limits
        };
        assert!(validate_aux(256, &limits).is_ok());
        assert!(validate_aux(257, &limits).is_err());
    }
}
