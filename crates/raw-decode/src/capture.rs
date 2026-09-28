//! Bounded ephemeral RAW capture ownership.
//!
//! A capture names a frozen stream, not an atomic snapshot of a mutable source.
//! Held bytes are not owned decoded pixels and do not establish render admission.
//! Byte capture is not yet implemented; the current slice owns private stages.

use engine_api::{
    EngineError, EngineResult, id::Digest, jobs::CancellationToken,
    pinned_raw::PinnedRawDecoderRoute,
};
use std::{
    fs, io,
    io::Write,
    path::Path,
    sync::{Arc, Mutex},
};
use tempfile::{NamedTempFile, TempDir, TempPath};

pub const ASSET_DOMAIN: &str = "tessera pinned RAW asset v1";
pub const CHUNK_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug)]
pub struct CaptureLimits {
    pub max_asset_bytes: u64,
    pub max_staged_bytes: u64,
    pub max_live_captures: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapturedAssetIdentity {
    pub digest: Digest,
    pub byte_len: u64,
}

#[derive(Clone)]
pub struct CapturePool {
    inner: Arc<PoolInner>,
}

/// No public constructor: only a future verified copy may create this owner.
pub struct CapturedRaw {
    stage: StageGuard,
    identity: CapturedAssetIdentity,
    route: PinnedRawDecoderRoute,
    suffix: String,
}

struct PoolInner {
    directory: TempDir,
    limits: CaptureLimits,
    accounting: Mutex<Accounting>,
    operations: Operations,
}

#[derive(Default)]
struct Accounting {
    bytes: u64,
    slots: usize,
    quarantine: Vec<Quarantined>,
}

struct Quarantined {
    path: TempPath,
    bytes: u64,
}

struct Reservation {
    pool: Arc<PoolInner>,
    active: bool,
}

struct StageGuard {
    file: Option<NamedTempFile>,
    reservation: Reservation,
}

#[derive(Default)]
struct Operations {
    #[cfg(test)]
    injected: Option<tests::TestOperations>,
}

impl Operations {
    fn setup(&self, path: &Path) -> io::Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        }
        #[cfg(test)]
        if let Some(ops) = &self.injected {
            (ops.setup)(path)?;
        }
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        #[cfg(test)]
        if let Some(ops) = &self.injected {
            return (ops.remove_file)(path);
        }
        fs::remove_file(path)
    }

    fn remove_dir(&self, path: &Path) -> io::Result<()> {
        #[cfg(test)]
        if let Some(ops) = &self.injected {
            return (ops.remove_dir)(path);
        }
        fs::remove_dir(path)
    }

    fn report(&self, error: &EngineError) {
        #[cfg(test)]
        if let Some(ops) = &self.injected {
            ops.diagnostics
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(error.to_string());
            return;
        }
        // Drop cannot return an error; diagnostics must not panic on stderr I/O.
        let _ = writeln!(io::stderr().lock(), "RAW capture cleanup: {error}");
    }
}

fn absent_or_removed(result: io::Result<()>, path: &Path) -> EngineResult<()> {
    match result {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(EngineError::io_at(path, &error)),
    }
}

impl CapturePool {
    pub fn create(parent: &Path, limits: CaptureLimits) -> EngineResult<Self> {
        Self::create_internal(parent, limits, Operations::default())
    }

    fn create_internal(
        parent: &Path,
        limits: CaptureLimits,
        operations: Operations,
    ) -> EngineResult<Self> {
        if limits.max_asset_bytes == 0
            || limits.max_asset_bytes == u64::MAX
            || limits.max_staged_bytes < limits.max_asset_bytes
            || limits.max_live_captures == 0
        {
            return Err(EngineError::invalid(
                "capture limits",
                "require positive bounded asset, byte and slot budgets",
            ));
        }
        let mut directory = tempfile::Builder::new()
            .prefix("raw-capture-")
            .tempdir_in(parent)
            .map_err(|e| EngineError::io_at(parent, &e))?;
        // From here every removal is explicit, including setup failure. Never let
        // TempDir perform a second, recursive deletion after a reported failure.
        directory.disable_cleanup(true);
        if let Err(error) = operations.setup(directory.path()) {
            let primary = EngineError::io_at(directory.path(), &error);
            if let Err(cleanup) =
                absent_or_removed(operations.remove_dir(directory.path()), directory.path())
            {
                operations.report(&cleanup);
            }
            return Err(primary);
        }
        Ok(Self {
            inner: Arc::new(PoolInner {
                directory,
                limits,
                accounting: Mutex::new(Accounting::default()),
                operations,
            }),
        })
    }

    // Task 2 will call this same stage owner; byte copying is deliberately absent.
    #[allow(dead_code)]
    fn allocate_stage(&self, suffix: &str, cancel: &CancellationToken) -> EngineResult<StageGuard> {
        cancel.check()?;
        let mut accounting = self
            .inner
            .accounting
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let bytes = accounting
            .bytes
            .checked_add(self.inner.limits.max_asset_bytes);
        let slots = accounting.slots.checked_add(1);
        let (Some(bytes), Some(slots)) = (bytes, slots) else {
            return Err(EngineError::ResourceExhausted {
                resource: "RAW capture accounting".into(),
            });
        };
        if bytes > self.inner.limits.max_staged_bytes || slots > self.inner.limits.max_live_captures
        {
            return Err(EngineError::ResourceExhausted {
                resource: "RAW capture staging".into(),
            });
        }
        accounting.bytes = bytes;
        accounting.slots = slots;
        drop(accounting);
        let reservation = Reservation {
            pool: self.inner.clone(),
            active: true,
        };
        let mut file = tempfile::Builder::new()
            .prefix("raw-")
            .suffix(&format!(".{suffix}"))
            .tempfile_in(self.inner.directory.path())
            .map_err(|e| EngineError::io_at(self.inner.directory.path(), &e))?;
        file.disable_cleanup(true);
        Ok(StageGuard {
            file: Some(file),
            reservation,
        })
    }

    pub fn capture(
        &self,
        _source: &Path,
        _route: PinnedRawDecoderRoute,
        _suffix: &str,
        _expected: Option<CapturedAssetIdentity>,
        _cancel: &CancellationToken,
    ) -> EngineResult<CapturedRaw> {
        Err(EngineError::Unsupported {
            what: "RAW byte capture is not implemented".into(),
        })
    }
}

impl Reservation {
    fn release(&mut self) {
        if self.active {
            let mut accounting = self
                .pool
                .accounting
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            accounting.bytes -= self.pool.limits.max_asset_bytes;
            accounting.slots -= 1;
            self.active = false;
        }
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        self.release();
    }
}

impl StageGuard {
    fn cleanup(&mut self) -> EngineResult<()> {
        let Some(file) = self.file.take() else {
            return Ok(());
        };
        // Closing the file preserves the disabled TempPath cleanup flag.
        let path = file.into_temp_path();
        let result = absent_or_removed(self.reservation.pool.operations.remove_file(&path), &path);
        if result.is_ok() {
            self.reservation.release();
        } else {
            let mut accounting = self
                .reservation
                .pool
                .accounting
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            accounting.quarantine.push(Quarantined {
                path,
                bytes: self.reservation.pool.limits.max_asset_bytes,
            });
            self.reservation.active = false;
        }
        result
    }

    fn close(mut self) -> EngineResult<()> {
        self.cleanup()
    }
}

impl Drop for StageGuard {
    fn drop(&mut self) {
        if let Err(error) = self.cleanup() {
            self.reservation.pool.operations.report(&error);
        }
    }
}

impl Drop for PoolInner {
    fn drop(&mut self) {
        let accounting = self.accounting.get_mut().unwrap_or_else(|e| e.into_inner());
        for stage in accounting.quarantine.drain(..) {
            match absent_or_removed(self.operations.remove_file(&stage.path), &stage.path) {
                Ok(()) => {
                    accounting.bytes -= stage.bytes;
                    accounting.slots -= 1;
                }
                Err(error) => self.operations.report(&error),
            }
        }
        if let Err(error) = absent_or_removed(
            self.operations.remove_dir(self.directory.path()),
            self.directory.path(),
        ) {
            self.operations.report(&error);
        }
    }
}

impl CapturedRaw {
    pub fn identity(&self) -> CapturedAssetIdentity {
        self.identity
    }
    pub fn route(&self) -> PinnedRawDecoderRoute {
        self.route
    }
    pub fn suffix(&self) -> &str {
        &self.suffix
    }
    pub fn close(self) -> EngineResult<()> {
        self.stage.close()
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
impl StageGuard {
    fn path_for_test(&self) -> &Path {
        self.file.as_ref().unwrap().path()
    }
}
#[cfg(test)]
impl CapturePool {
    fn accounting_for_test(&self) -> (u64, usize) {
        let accounting = self.inner.accounting.lock().unwrap();
        (accounting.bytes, accounting.slots)
    }
    fn directory_for_test(&self) -> &Path {
        self.inner.directory.path()
    }
    fn create_with_cleanup_for_test(
        parent: &Path,
        limits: CaptureLimits,
        remove: tests::PathOperation,
    ) -> EngineResult<Self> {
        Self::create_with_operations_for_test(
            parent,
            limits,
            tests::TestOperations {
                remove_file: remove,
                ..tests::TestOperations::default()
            },
        )
    }
    fn create_with_operations_for_test(
        parent: &Path,
        limits: CaptureLimits,
        ops: tests::TestOperations,
    ) -> EngineResult<Self> {
        Self::create_internal(
            parent,
            limits,
            Operations {
                injected: Some(ops),
            },
        )
    }
}
