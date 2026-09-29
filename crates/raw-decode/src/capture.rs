//! Bounded ephemeral RAW capture ownership.
//!
//! A capture names a frozen stream, not an atomic snapshot of a mutable source.
//! Held bytes are not owned decoded pixels and do not establish render admission.
//! All reads are bounded and identity is derived from the completed private stage.
//! Metadata comparisons reject observed changes but cannot prove snapshot atomicity.

use engine_api::{
    EngineError, EngineResult, id::Digest, jobs::CancellationToken,
    pinned_raw::PinnedRawDecoderRoute,
};
use std::{
    fs, io,
    io::{Read, Write},
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

/// Verified ephemeral bytes retained until this owner is closed or dropped.
///
/// This is not decoded RAW data, a recipe snapshot, or durable reopen storage.
/// A future closed decoder adapter must retain this owner through all probe/read
/// operations and return fully owned pixels/metadata before releasing it (or
/// transfer ownership into a lazy decoder). No public pathname escapes here.
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

enum StageStorage {
    Writable(NamedTempFile),
    Sealed {
        path: TempPath,
        file: Option<fs::File>,
    },
}

struct StageGuard {
    storage: Option<StageStorage>,
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

    // Both byte capture and ownership tests use this reservation/cleanup path.
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
            storage: Some(StageStorage::Writable(file)),
            reservation,
        })
    }

    pub fn capture(
        &self,
        source: &Path,
        route: PinnedRawDecoderRoute,
        suffix: &str,
        expected: Option<CapturedAssetIdentity>,
        cancel: &CancellationToken,
    ) -> EngineResult<CapturedRaw> {
        #[cfg(unix)]
        {
            self.capture_internal(
                source,
                route,
                suffix,
                expected,
                cancel,
                CaptureIo::default(),
            )
        }
        #[cfg(not(unix))]
        {
            let _ = (source, route, suffix, expected, cancel);
            Err(EngineError::Unsupported {
                what: "RAW capture requires nonblocking regular-file admission".into(),
            })
        }
    }

    #[cfg(unix)]
    fn capture_internal(
        &self,
        source: &Path,
        route: PinnedRawDecoderRoute,
        suffix: &str,
        expected: Option<CapturedAssetIdentity>,
        cancel: &CancellationToken,
        mut io: CaptureIo,
    ) -> EngineResult<CapturedRaw> {
        use std::os::unix::fs::OpenOptionsExt;
        cancel.check()?;
        let suffix = suffix.to_ascii_lowercase();
        if suffix.is_empty()
            || suffix.len() > 16
            || !suffix
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        {
            return Err(EngineError::invalid(
                "suffix",
                "expected 1–16 ASCII alphanumeric characters",
            ));
        }
        let mut original = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(source)
            .map_err(|e| EngineError::io_at(source, &e))?;
        let metadata = original
            .metadata()
            .map_err(|e| EngineError::io_at(source, &e))?;
        if !metadata.is_file() || metadata.len() == 0 {
            return Err(EngineError::invalid(
                "source",
                "capture requires a nonempty regular file",
            ));
        }
        let limit = self.inner.limits.max_asset_bytes;
        if metadata.len() > limit {
            return Err(capture_limit());
        }
        let before = SourceStamp::from(&metadata);
        check_source(source, &original, &before)?;
        let mut stage = self.allocate_stage(&suffix, cancel)?;
        let mut buffer = [0u8; CHUNK_BYTES];
        let mut copied = 0u64;
        loop {
            cancel.check()?;
            let capacity = read_capacity(limit, copied);
            let read = io.source_read(&mut original, &mut buffer[..capacity]);
            cancel.check()?;
            let count = match read {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                result => result.map_err(|e| EngineError::io_at(source, &e))?,
            };
            if count > capacity {
                return Err(invalid_read_count());
            }
            if count == 0 {
                break;
            }
            copied = copied.checked_add(count as u64).ok_or_else(capture_limit)?;
            if copied > limit {
                return Err(capture_limit());
            }
            let mut offset = 0;
            while offset < count {
                cancel.check()?;
                let write = io.stage_write(stage.writable()?, &buffer[offset..count]);
                cancel.check()?;
                match write {
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) => return Err(EngineError::io_at(stage.path()?, &error)),
                    Ok(0) => {
                        return Err(EngineError::io_at(
                            stage.path()?,
                            &io::Error::from(io::ErrorKind::WriteZero),
                        ));
                    }
                    Ok(n) if n > count - offset => return Err(invalid_read_count()),
                    Ok(n) => offset += n,
                }
            }
        }
        if copied == 0 {
            return Err(EngineError::invalid("source", "captured stream is empty"));
        }
        cancel.check()?;
        io.after_copy(source, stage.path()?)
            .map_err(|e| EngineError::io_at(stage.path().unwrap_or(source), &e))?;
        cancel.check()?;
        io.stage_sync(stage.writable()?)
            .map_err(|e| EngineError::io_at(stage.path().unwrap_or(source), &e))?;
        cancel.check()?;
        stage.seal(&mut io)?;
        cancel.check()?;
        let mut hasher = blake3::Hasher::new_derive_key(ASSET_DOMAIN);
        let mut hashed = 0u64;
        loop {
            cancel.check()?;
            let capacity = read_capacity(limit, hashed);
            let read = io.hash_read(stage.readonly()?, &mut buffer[..capacity]);
            cancel.check()?;
            let count = match read {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                result => {
                    result.map_err(|e| EngineError::io_at(stage.path().unwrap_or(source), &e))?
                }
            };
            if count > capacity {
                return Err(invalid_read_count());
            }
            if count == 0 {
                break;
            }
            hashed = hashed.checked_add(count as u64).ok_or_else(capture_limit)?;
            if hashed > limit {
                return Err(capture_limit());
            }
            hasher.update(&buffer[..count]);
        }
        if hashed != copied {
            return Err(EngineError::Conflict {
                message: "completed RAW stage length changed".into(),
            });
        }
        check_source(source, &original, &before)?;
        let identity = CapturedAssetIdentity {
            digest: Digest(*hasher.finalize().as_bytes()),
            byte_len: hashed,
        };
        if expected.is_some_and(|expected| expected != identity) {
            return Err(EngineError::Conflict {
                message: "captured RAW identity does not match expected bytes".into(),
            });
        }
        cancel.check()?;
        Ok(CapturedRaw {
            stage,
            identity,
            route,
            suffix,
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
        let Some(storage) = self.storage.take() else {
            return Ok(());
        };
        // Both states preserve disabled cleanup; close any descriptor before unlink.
        let path = match storage {
            StageStorage::Writable(file) => file.into_temp_path(),
            StageStorage::Sealed { path, file } => {
                drop(file);
                path
            }
        };
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
        self.path().unwrap()
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

#[cfg(all(test, unix))]
impl CapturePool {
    fn capture_with_hooks_for_test(
        &self,
        source: &Path,
        route: PinnedRawDecoderRoute,
        suffix: &str,
        expected: Option<CapturedAssetIdentity>,
        cancel: &CancellationToken,
        hooks: tests::stream::StreamHooks,
    ) -> EngineResult<CapturedRaw> {
        self.capture_internal(source, route, suffix, expected, cancel, CaptureIo { hooks })
    }
}
#[cfg(all(test, unix))]
impl CapturedRaw {
    fn path_for_test(&self) -> &Path {
        self.stage.path().unwrap()
    }
    fn readonly_file_for_test(&self) -> &fs::File {
        match self.stage.storage.as_ref().unwrap() {
            StageStorage::Sealed {
                file: Some(file), ..
            } => file,
            _ => panic!("capture must retain a sealed readonly descriptor"),
        }
    }
}

fn capture_limit() -> EngineError {
    EngineError::ResourceExhausted {
        resource: "RAW capture byte limit".into(),
    }
}
fn invalid_read_count() -> EngineError {
    EngineError::from(io::Error::new(
        io::ErrorKind::InvalidData,
        "I/O operation returned excess count",
    ))
}
fn read_capacity(limit: u64, consumed: u64) -> usize {
    // create rejects u64::MAX and each loop rejects excess before coming here.
    (limit - consumed + 1).min(CHUNK_BYTES as u64) as usize
}

#[cfg(unix)]
#[derive(PartialEq, Eq)]
struct SourceStamp {
    device: u64,
    inode: u64,
    length: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}
#[cfg(unix)]
impl From<&fs::Metadata> for SourceStamp {
    fn from(metadata: &fs::Metadata) -> Self {
        use std::os::unix::fs::MetadataExt;
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            length: metadata.len(),
            modified: (metadata.mtime(), metadata.mtime_nsec()),
            changed: (metadata.ctime(), metadata.ctime_nsec()),
        }
    }
}
#[cfg(unix)]
fn check_source(path: &Path, opened: &fs::File, expected: &SourceStamp) -> EngineResult<()> {
    let conflict = || EngineError::Conflict {
        message: "RAW source changed during capture".into(),
    };
    let handle = opened.metadata().map_err(|_| conflict())?;
    let locator = fs::metadata(path).map_err(|_| conflict())?;
    if !handle.is_file()
        || !locator.is_file()
        || SourceStamp::from(&handle) != *expected
        || SourceStamp::from(&locator) != *expected
    {
        return Err(conflict());
    }
    Ok(())
}

impl StageGuard {
    fn path(&self) -> EngineResult<&Path> {
        match &self.storage {
            Some(StageStorage::Writable(file)) => Ok(file.path()),
            Some(StageStorage::Sealed { path, .. }) => Ok(path),
            None => Err(EngineError::internal("RAW stage already closed")),
        }
    }
    fn writable(&mut self) -> EngineResult<&mut fs::File> {
        match &mut self.storage {
            Some(StageStorage::Writable(file)) => Ok(file.as_file_mut()),
            _ => Err(EngineError::internal("RAW stage is not writable")),
        }
    }
    fn readonly(&mut self) -> EngineResult<&mut fs::File> {
        match &mut self.storage {
            Some(StageStorage::Sealed {
                file: Some(file), ..
            }) => Ok(file),
            _ => Err(EngineError::internal(
                "RAW stage has no readonly descriptor",
            )),
        }
    }
    fn seal(&mut self, io: &mut CaptureIo) -> EngineResult<()> {
        let storage = self
            .storage
            .take()
            .ok_or_else(|| EngineError::internal("RAW stage missing"))?;
        match storage {
            StageStorage::Writable(file) => {
                // No fallible operation between closing writer and restoring owner.
                self.storage = Some(StageStorage::Sealed {
                    path: file.into_temp_path(),
                    file: None,
                });
            }
            other => {
                self.storage = Some(other);
                return Err(EngineError::internal("RAW stage already sealed"));
            }
        }
        if let Some(StageStorage::Sealed { path, file }) = &mut self.storage {
            *file = Some(
                io.readonly_reopen(path)
                    .map_err(|e| EngineError::io_at(&*path, &e))?,
            );
        }
        Ok(())
    }
}

#[derive(Default)]
struct CaptureIo {
    #[cfg(all(test, unix))]
    hooks: tests::stream::StreamHooks,
}
impl CaptureIo {
    fn source_read(&mut self, file: &mut fs::File, buffer: &mut [u8]) -> io::Result<usize> {
        #[cfg(all(test, unix))]
        if let Some(hook) = &mut self.hooks.source_read {
            return hook(file, buffer);
        }
        file.read(buffer)
    }
    fn stage_write(&mut self, file: &mut fs::File, bytes: &[u8]) -> io::Result<usize> {
        #[cfg(all(test, unix))]
        if let Some(hook) = &mut self.hooks.stage_write {
            return hook(file, bytes);
        }
        file.write(bytes)
    }
    fn stage_sync(&mut self, file: &fs::File) -> io::Result<()> {
        #[cfg(all(test, unix))]
        if let Some(hook) = &mut self.hooks.stage_sync {
            return hook(file);
        }
        file.sync_all()
    }
    fn after_copy(&mut self, _source: &Path, _stage: &Path) -> io::Result<()> {
        #[cfg(all(test, unix))]
        if let Some(hook) = &mut self.hooks.after_copy {
            return hook(_source, _stage);
        }
        Ok(())
    }
    fn readonly_reopen(&mut self, path: &Path) -> io::Result<fs::File> {
        #[cfg(all(test, unix))]
        if let Some(hook) = &mut self.hooks.readonly_reopen {
            return hook(path);
        }
        fs::File::open(path)
    }
    fn hash_read(&mut self, file: &mut fs::File, buffer: &mut [u8]) -> io::Result<usize> {
        #[cfg(all(test, unix))]
        if let Some(hook) = &mut self.hooks.hash_read {
            return hook(file, buffer);
        }
        file.read(buffer)
    }
}

// Deliberately test-only: generic T does not prove an owned decoded result.
// A production decoder adapter requires a separately reviewed closed output type.
#[cfg(all(test, unix))]
impl CapturedRaw {
    fn consume_for_test<T>(
        self,
        cancel: &CancellationToken,
        consumer: impl FnOnce(&Path, PinnedRawDecoderRoute) -> EngineResult<T>,
    ) -> EngineResult<T> {
        cancel.check()?;
        let result = consumer(self.stage.path()?, self.route);
        let pool = self.stage.reservation.pool.clone();
        let cleanup = self.close();
        match result {
            Ok(value) => {
                cleanup?;
                Ok(value)
            }
            Err(primary) => {
                if let Err(secondary) = cleanup {
                    pool.operations.report(&secondary);
                }
                Err(primary)
            }
        }
    }
}

mod decode;
pub use decode::DecodedCapturedCfa;

mod owned;
pub use owned::{CfaPlaneFacts, OwnedCapturedCfa};
