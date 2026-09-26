//! macOS ImageCaptureCore backend.
//!
//! Construct, call, and drop this backend on the **process main thread**, not a
//! Tokio worker or a dedicated background thread. It is deliberately !Send and
//! !Sync. ImageCaptureCore delivers delegate events on the main run loop; these
//! methods pump it in bounded slices (discovery up to 2 s, session/catalog up to
//! 15 s, shutdown up to 5 s). Call `poll` regularly even when no shutter command
//! was sent, to receive physical-shutter captures and asynchronous errors.
//!
//! `capture` means the command was submitted, not that a file is ready. Only
//! completed downloads are returned by `poll`. Initial camera inventory is
//! never imported. macOS SDKs provide tethering automatically for cameras with
//! the remote-capture capability; deprecated enable/disable calls are not used.

use crate::{Device, Result, TetherBackend};
use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use crate::Error;
    use std::ffi::{CStr, CString, OsStr, c_char, c_int, c_void};
    use std::marker::PhantomData;
    use std::os::unix::ffi::OsStrExt;
    use std::ptr::NonNull;
    use std::rc::Rc;

    type DeviceVisitor = unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, c_int);
    type PathVisitor = unsafe extern "C" fn(*mut c_void, *const c_char);

    unsafe extern "C" {
        fn tessera_tether_new() -> *mut c_void;
        fn tessera_tether_free(handle: *mut c_void);
        fn tessera_tether_string_free(value: *mut c_char);
        fn tessera_tether_devices(
            handle: *mut c_void,
            visit: DeviceVisitor,
            user: *mut c_void,
        ) -> *mut c_char;
        fn tessera_tether_start(handle: *mut c_void, folder: *const c_char) -> *mut c_char;
        fn tessera_tether_capture(handle: *mut c_void) -> *mut c_char;
        fn tessera_tether_poll(
            handle: *mut c_void,
            visit: PathVisitor,
            user: *mut c_void,
        ) -> *mut c_char;
        fn tessera_tether_stop(handle: *mut c_void) -> *mut c_char;
    }

    /// Main-thread-only camera backend. See the module-level run-loop contract.
    pub struct NativeBackend {
        handle: NonNull<c_void>,
        _main_thread_only: PhantomData<Rc<()>>,
    }

    impl NativeBackend {
        pub fn new() -> Result<Self> {
            // SAFETY: no arguments; the shim checks NSThread.isMainThread before
            // creating or touching any ImageCaptureCore objects.
            let handle = NonNull::new(unsafe { tessera_tether_new() }).ok_or_else(|| {
                Error::Message(
                    "ImageCaptureCore must be constructed on the process main thread".into(),
                )
            })?;
            Ok(Self {
                handle,
                _main_thread_only: PhantomData,
            })
        }
    }

    fn check(error: *mut c_char) -> Result<()> {
        if error.is_null() {
            return Ok(());
        }
        // SAFETY: the bridge returns an owned, NUL-terminated strdup allocation.
        let text = unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned();
        unsafe { tessera_tether_string_free(error) };
        Err(Error::Message(text))
    }

    unsafe extern "C" fn device_visitor(
        user: *mut c_void,
        id: *const c_char,
        name: *const c_char,
        can_capture: c_int,
    ) {
        // SAFETY: visitors are synchronous, called only within devices(), with
        // non-null borrowed strings and its exclusively borrowed Vec pointer.
        let devices = unsafe { &mut *user.cast::<Vec<Device>>() };
        devices.push(Device {
            id: unsafe { CStr::from_ptr(id) }.to_string_lossy().into_owned(),
            name: unsafe { CStr::from_ptr(name) }
                .to_string_lossy()
                .into_owned(),
            can_capture: can_capture != 0,
        });
    }

    unsafe extern "C" fn path_visitor(user: *mut c_void, path: *const c_char) {
        // SAFETY: same synchronous visitor contract as device_visitor. Preserve
        // filesystem bytes rather than lossy UTF-8 conversions for paths.
        let paths = unsafe { &mut *user.cast::<Vec<PathBuf>>() };
        let bytes = unsafe { CStr::from_ptr(path) }.to_bytes();
        paths.push(PathBuf::from(OsStr::from_bytes(bytes)));
    }

    impl TetherBackend for NativeBackend {
        fn devices(&mut self) -> Result<Vec<Device>> {
            let mut devices = Vec::<Device>::new();
            // SAFETY: handle lives until Drop; !Send/!Sync enforces thread
            // affinity, and the visitor/user borrow lasts until this returns.
            check(unsafe {
                tessera_tether_devices(
                    self.handle.as_ptr(),
                    device_visitor,
                    (&mut devices as *mut Vec<Device>).cast(),
                )
            })?;
            Ok(devices)
        }

        fn start(&mut self, folder: &Path) -> Result<()> {
            // Canonicalization supplies an absolute existing directory and
            // preserves OS errors. The caller owns directory creation policy.
            let folder = std::fs::canonicalize(folder).map_err(Error::Io)?;
            if !folder.is_dir() {
                return Err(Error::Message("Download folder is not a directory".into()));
            }
            let folder = CString::new(folder.as_os_str().as_bytes())
                .map_err(|_| Error::Message("Download folder contains a NUL byte".into()))?;
            // SAFETY: valid handle, and folder is borrowed only during this call.
            check(unsafe { tessera_tether_start(self.handle.as_ptr(), folder.as_ptr()) })
        }

        fn capture(&mut self) -> Result<()> {
            // SAFETY: valid main-thread handle, exclusively borrowed.
            check(unsafe { tessera_tether_capture(self.handle.as_ptr()) })
        }

        fn poll(&mut self) -> Result<Vec<PathBuf>> {
            let mut paths = Vec::<PathBuf>::new();
            // SAFETY: synchronous visitor with a live, exclusively borrowed Vec.
            check(unsafe {
                tessera_tether_poll(
                    self.handle.as_ptr(),
                    path_visitor,
                    (&mut paths as *mut Vec<PathBuf>).cast(),
                )
            })?;
            Ok(paths)
        }

        fn stop(&mut self) -> Result<()> {
            // SAFETY: valid main-thread handle, exclusively borrowed.
            check(unsafe { tessera_tether_stop(self.handle.as_ptr()) })
        }
    }

    impl Drop for NativeBackend {
        fn drop(&mut self) {
            // SAFETY: the owned handle is released exactly once on its creating
            // main thread. Outstanding native operations retain their own
            // delegate; none retain a pointer into Rust or this handle.
            unsafe { tessera_tether_free(self.handle.as_ptr()) };
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn construction_off_process_main_thread_is_rejected() {
            // libtest itself also uses worker threads, but explicitly spawn so
            // this assertion doesn't depend on its execution configuration.
            assert!(
                std::thread::spawn(|| matches!(NativeBackend::new(), Err(Error::Message(_))))
                    .join()
                    .unwrap()
            );
        }

        #[test]
        fn successful_bridge_status_is_ok() {
            assert!(check(std::ptr::null_mut()).is_ok());
        }

        #[test]
        fn device_visitor_preserves_capability_and_identity() {
            let mut result = Vec::<Device>::new();
            let id = CString::new("camera-id").unwrap();
            let name = CString::new("Camera α").unwrap();
            unsafe {
                device_visitor(
                    (&mut result as *mut Vec<Device>).cast(),
                    id.as_ptr(),
                    name.as_ptr(),
                    1,
                )
            };
            assert_eq!(result.len(), 1);
            assert_eq!(result[0].id, "camera-id");
            assert_eq!(result[0].name, "Camera α");
            assert!(result[0].can_capture);
        }

        #[test]
        fn path_visitor_preserves_non_utf8_filesystem_bytes() {
            let mut result = Vec::<PathBuf>::new();
            let path = CString::new(b"/tmp/capture-\xff.raw".as_slice()).unwrap();
            unsafe { path_visitor((&mut result as *mut Vec<PathBuf>).cast(), path.as_ptr()) };
            assert_eq!(result[0].as_os_str().as_bytes(), b"/tmp/capture-\xff.raw");
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::*;
    use crate::Error;

    /// ImageCaptureCore is only available on macOS.
    pub struct NativeBackend;

    impl NativeBackend {
        pub fn new() -> Result<Self> {
            Err(Error::Unsupported)
        }
    }
    impl TetherBackend for NativeBackend {
        fn devices(&mut self) -> Result<Vec<Device>> {
            Err(Error::Unsupported)
        }
        fn start(&mut self, _folder: &Path) -> Result<()> {
            Err(Error::Unsupported)
        }
        fn capture(&mut self) -> Result<()> {
            Err(Error::Unsupported)
        }
        fn poll(&mut self) -> Result<Vec<PathBuf>> {
            Err(Error::Unsupported)
        }
        fn stop(&mut self) -> Result<()> {
            Err(Error::Unsupported)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn unsupported_platform_is_explicit() {
            assert!(matches!(NativeBackend::new(), Err(Error::Unsupported)));
        }
    }
}

pub use platform::NativeBackend;
