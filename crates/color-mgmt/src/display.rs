use crate::{Error, Profile, Registry, Result};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct DisplayProfile {
    pub display_id: u32,
    pub profile: Arc<Profile>,
}
impl Registry {
    /// Refresh one monitor by OS display ID. Missing or unusable ICC data falls
    /// back to the registry's sRGB profile, including disconnected monitors.
    pub fn display_profile(&mut self, display_id: u32) -> Result<DisplayProfile> {
        #[cfg(target_os = "macos")]
        let profile =
            macos::profile_bytes(display_id).and_then(|bytes| self.load_bytes(&bytes).ok());
        #[cfg(not(target_os = "macos"))]
        let profile = None;
        Ok(DisplayProfile {
            display_id,
            profile: match profile {
                Some(profile) => profile,
                None => self.builtin(crate::Builtin::Srgb)?,
            },
        })
    }

    /// Query active monitors each time; OS profile changes are not cached by display ID.
    pub fn display_profiles(&mut self) -> Result<Vec<DisplayProfile>> {
        #[cfg(target_os = "macos")]
        {
            macos::discover(self)
        }
        #[cfg(not(target_os = "macos"))]
        {
            Err(Error::Unsupported("display discovery requires macOS"))
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use objc2_core_foundation::CFRetained;
    use objc2_core_graphics::{CGColorSpace, CGError, CGGetActiveDisplayList};
    use std::ptr::{self, NonNull};

    // objc2 0.3.2's generated wrapper assumes this is non-null and panics on
    // a disconnected display. Keep its typed RAII ownership but admit null so
    // that hot-unplug and missing profiles can use the required sRGB fallback.
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C-unwind" {
        fn CGDisplayCopyColorSpace(display: u32) -> Option<NonNull<CGColorSpace>>;
    }

    pub fn profile_bytes(display_id: u32) -> Option<Vec<u8>> {
        // CoreGraphics may return the main monitor for sentinel/unknown IDs.
        // Membership in the active list, not CGDisplayIsActive, defines identity.
        if !active_display_ids().ok()?.contains(&display_id) {
            return None;
        }
        // SAFETY: Copy-rule +1 ownership transfers to CFRetained exactly once.
        // The copied ICC CFData is immutable and retained while we copy bytes.
        unsafe {
            let space = CFRetained::from_raw(CGDisplayCopyColorSpace(display_id)?);
            let data = CGColorSpace::icc_data(Some(&space))?;
            Some(data.as_bytes_unchecked().to_vec())
        }
    }

    fn active_display_ids() -> Result<Vec<u32>> {
        // SAFETY: counts and arrays are valid writable storage. Copy-rule objects
        // are checked for null and released exactly once by the RAII guards.
        unsafe {
            let mut count = 0;
            if CGGetActiveDisplayList(0, ptr::null_mut(), &mut count) != CGError::Success {
                return Err(Error::Unsupported(
                    "CoreGraphics display enumeration failed",
                ));
            }
            if count == 0 {
                return Ok(Vec::new());
            }
            let mut ids = vec![0; count as usize];
            if CGGetActiveDisplayList(count, ids.as_mut_ptr(), &mut count) != CGError::Success {
                return Err(Error::Unsupported(
                    "CoreGraphics display enumeration failed",
                ));
            }
            ids.truncate(count as usize);
            Ok(ids)
        }
    }

    pub fn discover(registry: &mut Registry) -> Result<Vec<DisplayProfile>> {
        active_display_ids()?
            .into_iter()
            .map(|id| registry.display_profile(id))
            .collect()
    }
}
