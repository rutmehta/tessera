//! Volumes (CS-1). A root on an external drive is remembered by the volume's
//! UUID and the path relative to its mount point, so the same drive opens on
//! any Mac wherever macOS mounts it (`/Volumes/Rut photo-1` included). The
//! boot volume keeps plain absolute paths: its files never move with a drive.
use engine_api::error::{EngineError, EngineResult};
use std::path::{Path, PathBuf};

/// An external volume: removable, external or network, mounted under `/Volumes`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Volume {
    /// Uppercase RFC 4122 text, as `diskutil info` prints it.
    pub uuid: String,
    pub mount_point: PathBuf,
}
impl Volume {
    /// What the user calls the drive: the mount point's last component.
    pub fn name(&self) -> String {
        self.mount_point
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

/// Which volume holds a path. Hosts and tests may substitute their own.
pub trait VolumeResolver: Send + Sync + std::fmt::Debug {
    /// The external volume holding `path`; `None` on the boot volume (or where
    /// the platform cannot identify volumes, which also means plain paths).
    fn volume_of(&self, path: &Path) -> EngineResult<Option<Volume>>;
    /// External volumes mounted right now.
    fn mounted(&self) -> EngineResult<Vec<Volume>>;
}

/// Where a root lives: a plain path on the boot volume, or a path relative to
/// an external volume (`relative_path` has no leading slash; empty for the
/// volume's own root).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RootLocation {
    pub volume_uuid: Option<String>,
    pub relative_path: String,
}
impl RootLocation {
    pub fn resolve(volumes: &dyn VolumeResolver, path: &Path) -> EngineResult<Self> {
        Ok(match volumes.volume_of(path)? {
            Some(volume) => Self {
                relative_path: relative_path(&volume.mount_point, path),
                volume_uuid: Some(volume.uuid),
            },
            None => Self {
                volume_uuid: None,
                relative_path: path.to_string_lossy().into_owned(),
            },
        })
    }
    /// The path this location has while `mounted` lists its volume; a boot
    /// location is its own path. `None` while the volume is not attached.
    pub fn locate(&self, mounted: &[Volume]) -> Option<PathBuf> {
        match &self.volume_uuid {
            None => Some(PathBuf::from(&self.relative_path)),
            Some(uuid) => mounted
                .iter()
                .find(|v| &v.uuid == uuid)
                .map(|v| join_relative(&v.mount_point, &self.relative_path)),
        }
    }
}

/// `path` relative to `mount_point`, with forward slashes and no leading slash.
pub fn relative_path(mount_point: &Path, path: &Path) -> String {
    path.strip_prefix(mount_point)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

pub fn join_relative(mount_point: &Path, relative: &str) -> PathBuf {
    if relative.is_empty() {
        mount_point.to_path_buf()
    } else {
        mount_point.join(relative)
    }
}

/// Volumes as macOS reports them (`statfs` for the mount point, `getattrlist`
/// for the UUID). Elsewhere every path is a plain path.
#[derive(Debug, Default)]
pub struct SystemVolumes;

/// A fixed set of volumes matched by mount-point prefix (tests and hosts that
/// know their volumes). Longest mount point wins.
#[derive(Debug, Default)]
pub struct FixedVolumes(pub Vec<Volume>);

impl VolumeResolver for FixedVolumes {
    fn volume_of(&self, path: &Path) -> EngineResult<Option<Volume>> {
        Ok(self
            .0
            .iter()
            .filter(|v| path.starts_with(&v.mount_point))
            .max_by_key(|v| v.mount_point.as_os_str().len())
            .cloned())
    }
    fn mounted(&self) -> EngineResult<Vec<Volume>> {
        Ok(self.0.clone())
    }
}

impl VolumeResolver for SystemVolumes {
    #[cfg(target_os = "macos")]
    fn volume_of(&self, path: &Path) -> EngineResult<Option<Volume>> {
        macos::volume_of(path).map_err(|e| EngineError::io_at(path, &e))
    }
    #[cfg(not(target_os = "macos"))]
    fn volume_of(&self, _path: &Path) -> EngineResult<Option<Volume>> {
        Ok(None)
    }
    #[cfg(target_os = "macos")]
    fn mounted(&self) -> EngineResult<Vec<Volume>> {
        let mut out = Vec::new();
        let entries = match std::fs::read_dir("/Volumes") {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(EngineError::io_at(Path::new("/Volumes"), &e)),
        };
        for entry in entries.flatten() {
            // A detached or unreadable volume is simply not listed.
            if let Ok(Some(volume)) = macos::volume_of(&entry.path())
                && volume.mount_point == entry.path()
                && !out.contains(&volume)
            {
                out.push(volume);
            }
        }
        out.sort_by(|a, b| a.mount_point.cmp(&b.mount_point));
        Ok(out)
    }
    #[cfg(not(target_os = "macos"))]
    fn mounted(&self) -> EngineResult<Vec<Volume>> {
        Ok(Vec::new())
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::Volume;
    use std::{
        ffi::{CStr, CString},
        io,
        os::unix::ffi::OsStrExt,
        path::{Path, PathBuf},
    };

    /// `/Volumes/<name>` is an external volume; `/` and the system's own
    /// `/System/Volumes/*` (Data, Preboot, VM) are the boot volume.
    fn is_external(mount_point: &Path) -> bool {
        mount_point.starts_with("/Volumes") && mount_point != Path::new("/Volumes")
    }

    pub(super) fn volume_of(path: &Path) -> io::Result<Option<Volume>> {
        let c = CString::new(path.as_os_str().as_bytes())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        let mut st: libc::statfs = unsafe { std::mem::zeroed() };
        // SAFETY: a valid NUL-terminated path and an out-pointer to a statfs.
        if unsafe { libc::statfs(c.as_ptr(), &mut st) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the kernel NUL-terminates f_mntonname within its fixed array.
        let mount = PathBuf::from(std::ffi::OsStr::from_bytes(
            unsafe { CStr::from_ptr(st.f_mntonname.as_ptr()) }.to_bytes(),
        ));
        if !is_external(&mount) {
            return Ok(None);
        }
        Ok(volume_uuid(&mount)?.map(|uuid| Volume {
            uuid,
            mount_point: mount,
        }))
    }

    /// The volume UUID, or `None` when the file system has none (a zero UUID),
    /// in which case the root keeps plain paths.
    fn volume_uuid(mount: &Path) -> io::Result<Option<String>> {
        let c = CString::new(mount.as_os_str().as_bytes())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        let mut list: libc::attrlist = unsafe { std::mem::zeroed() };
        list.bitmapcount = libc::ATTR_BIT_MAP_COUNT;
        list.volattr = libc::ATTR_VOL_INFO | libc::ATTR_VOL_UUID;
        // u32 length followed by the 16-byte uuid_t.
        let mut buffer = [0u8; 4 + 16];
        // SAFETY: the attrlist and buffer are valid for the lengths given.
        let rc = unsafe {
            libc::getattrlist(
                c.as_ptr(),
                &mut list as *mut libc::attrlist as *mut libc::c_void,
                buffer.as_mut_ptr() as *mut libc::c_void,
                buffer.len(),
                0,
            )
        };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        let length = u32::from_ne_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]) as usize;
        if length < buffer.len() {
            return Ok(None);
        }
        let uuid = &buffer[4..20];
        if uuid.iter().all(|b| *b == 0) {
            return Ok(None);
        }
        Ok(Some(format_uuid(uuid)))
    }

    fn format_uuid(bytes: &[u8]) -> String {
        let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02X}")).collect();
        format!(
            "{}-{}-{}-{}-{}",
            hex[0..4].concat(),
            hex[4..6].concat(),
            hex[6..8].concat(),
            hex[8..10].concat(),
            hex[10..16].concat()
        )
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn boot_paths_have_no_volume_and_uuids_format_as_diskutil() {
            assert_eq!(super::volume_of(std::path::Path::new("/")).unwrap(), None);
            assert_eq!(
                super::volume_of(&std::env::temp_dir()).unwrap(),
                None,
                "the temp dir is on the boot volume"
            );
            assert_eq!(
                super::format_uuid(&[
                    0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x11, 0x22, 0x33, 0x44, 0x55,
                    0x66, 0x77, 0x88
                ]),
                "12345678-9ABC-DEF0-1122-334455667788"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn volume(uuid: &str, mount: &str) -> Volume {
        Volume {
            uuid: uuid.into(),
            mount_point: PathBuf::from(mount),
        }
    }

    #[test]
    fn locations_follow_the_volume_and_keep_boot_paths() {
        let drive = volume("AAAA-1", "/Volumes/Rut photo");
        let fixed = FixedVolumes(vec![drive.clone(), volume("BBBB-2", "/Volumes/Rut photo/nested")]);
        let location =
            RootLocation::resolve(&fixed, Path::new("/Volumes/Rut photo/2026/wedding")).unwrap();
        assert_eq!(location.volume_uuid.as_deref(), Some("AAAA-1"));
        assert_eq!(location.relative_path, "2026/wedding");
        // Longest mount point wins, and the volume root is the empty relative path.
        let nested = RootLocation::resolve(&fixed, Path::new("/Volumes/Rut photo/nested")).unwrap();
        assert_eq!(nested.volume_uuid.as_deref(), Some("BBBB-2"));
        assert_eq!(nested.relative_path, "");
        // Remounted elsewhere, the root is found there; absent, it is not.
        let moved = volume("AAAA-1", "/Volumes/Rut photo-1");
        assert_eq!(
            location.locate(&[moved]),
            Some(PathBuf::from("/Volumes/Rut photo-1/2026/wedding"))
        );
        assert_eq!(location.locate(&[]), None);
        assert_eq!(nested.locate(&[volume("BBBB-2", "/mnt/x")]), Some(PathBuf::from("/mnt/x")));
        let boot = RootLocation::resolve(&fixed, Path::new("/Users/rut/Pictures")).unwrap();
        assert_eq!(boot.volume_uuid, None);
        assert_eq!(boot.locate(&[]), Some(PathBuf::from("/Users/rut/Pictures")));
        assert_eq!(drive.name(), "Rut photo");
    }
}
