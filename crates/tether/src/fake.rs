//! A camera stand-in for tests and acceptance runs: it "shoots" the image files of
//! a source folder, one at a time, in file-name order. A frame drops into the
//! session's staging directory when `capture` is requested (on the next `poll`)
//! and, with a non-zero interval, on a timer as if the photographer pressed the
//! physical shutter. Source files are copied, never moved or modified.

use crate::{Device, Error, Result, TetherBackend};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// Extensions the fake camera will shoot (the ingest pipeline decides what it can index).
const EXTENSIONS: [&str; 10] = [
    "jpg", "jpeg", "arw", "cr2", "cr3", "nef", "raf", "dng", "orf", "rw2",
];

pub struct FolderDropBackend {
    source: PathBuf,
    interval: Option<Duration>,
    queue: VecDeque<PathBuf>,
    staging: Option<PathBuf>,
    requested: usize,
    next_drop: Option<Instant>,
}

impl FolderDropBackend {
    /// `interval` of zero drops frames only on `capture`.
    pub fn new(source: &Path, interval: Duration) -> Result<Self> {
        if !source.is_dir() {
            return Err(Error::Message(format!(
                "fake camera folder {} is not a directory",
                source.display()
            )));
        }
        Ok(Self {
            source: source.into(),
            interval: (!interval.is_zero()).then_some(interval),
            queue: Self::frames(source)?,
            staging: None,
            requested: 0,
            next_drop: None,
        })
    }

    fn frames(source: &Path) -> Result<VecDeque<PathBuf>> {
        let mut frames: Vec<PathBuf> = std::fs::read_dir(source)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.is_file()
                    && !p
                        .file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with('.'))
                    && p.extension().is_some_and(|e| {
                        EXTENSIONS.contains(&e.to_string_lossy().to_lowercase().as_str())
                    })
            })
            .collect();
        frames.sort();
        Ok(frames.into())
    }

    /// Frames the fake camera has left to shoot.
    pub fn remaining(&self) -> usize {
        self.queue.len()
    }

    fn drop_next(&mut self, staging: &Path) -> Result<Option<PathBuf>> {
        let Some(source) = self.queue.pop_front() else {
            return Ok(None);
        };
        let name = source
            .file_name()
            .ok_or_else(|| Error::Message("fake frame has no file name".into()))?;
        // Write under a hidden name, then rename: the ingest never sees a partial file.
        let partial = staging.join(format!(".partial-{}", name.to_string_lossy()));
        std::fs::copy(&source, &partial)?;
        let target = staging.join(name);
        std::fs::rename(&partial, &target)?;
        Ok(Some(target))
    }
}

impl TetherBackend for FolderDropBackend {
    fn devices(&mut self) -> Result<Vec<Device>> {
        Ok(vec![Device {
            id: format!("fake:{}", self.source.display()),
            name: format!(
                "Test camera ({})",
                self.source
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            ),
            can_capture: true,
            battery_percent: Some(80),
            shots_remaining: Some(self.queue.len() as u32),
        }])
    }
    fn start(&mut self, folder: &Path) -> Result<()> {
        if self.staging.is_some() {
            return Err(Error::Message("fake camera session already started".into()));
        }
        self.staging = Some(folder.canonicalize()?);
        self.next_drop = self.interval.map(|i| Instant::now() + i);
        Ok(())
    }
    fn capture(&mut self) -> Result<()> {
        if self.staging.is_none() {
            return Err(Error::Message("no camera session".into()));
        }
        if self.requested >= self.queue.len() {
            return Err(Error::Message(
                "the test camera has no frames left in its folder".into(),
            ));
        }
        self.requested += 1;
        Ok(())
    }
    fn poll(&mut self) -> Result<Vec<PathBuf>> {
        let Some(staging) = self.staging.clone() else {
            return Ok(Vec::new());
        };
        let mut due = std::mem::take(&mut self.requested);
        if let (Some(interval), Some(next)) = (self.interval, self.next_drop)
            && Instant::now() >= next
        {
            due += 1;
            self.next_drop = Some(Instant::now() + interval);
        }
        let mut dropped = Vec::new();
        for _ in 0..due {
            match self.drop_next(&staging)? {
                Some(path) => dropped.push(path),
                None => break,
            }
        }
        Ok(dropped)
    }
    fn stop(&mut self) -> Result<()> {
        self.staging = None;
        self.requested = 0;
        self.next_drop = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(names: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for n in names {
            std::fs::write(dir.path().join(n), b"x").unwrap();
        }
        dir
    }

    #[test]
    fn capture_drops_frames_in_name_order_and_copies() {
        let src = folder(&["b.jpg", "a.JPG", "notes.txt", ".hidden.jpg"]);
        let staging = tempfile::tempdir().unwrap();
        let mut cam = FolderDropBackend::new(src.path(), Duration::ZERO).unwrap();
        assert_eq!(cam.devices().unwrap()[0].shots_remaining, Some(2));
        cam.start(staging.path()).unwrap();
        assert!(cam.poll().unwrap().is_empty(), "no timer, no capture");
        cam.capture().unwrap();
        let first = cam.poll().unwrap();
        assert_eq!(first.len(), 1);
        assert!(first[0].ends_with("a.JPG"));
        assert!(
            src.path().join("a.JPG").is_file(),
            "source is copied, not moved"
        );
        cam.capture().unwrap();
        assert!(cam.poll().unwrap()[0].ends_with("b.jpg"));
        assert!(cam.capture().is_err(), "folder exhausted");
        assert_eq!(cam.remaining(), 0);
    }

    #[test]
    fn timer_drops_without_capture() {
        let src = folder(&["1.jpg", "2.jpg"]);
        let staging = tempfile::tempdir().unwrap();
        let mut cam = FolderDropBackend::new(src.path(), Duration::from_millis(20)).unwrap();
        cam.start(staging.path()).unwrap();
        std::thread::sleep(Duration::from_millis(40));
        assert_eq!(cam.poll().unwrap().len(), 1);
        cam.stop().unwrap();
        std::thread::sleep(Duration::from_millis(40));
        assert!(cam.poll().unwrap().is_empty(), "stopped");
    }

    #[test]
    fn missing_folder_is_an_error() {
        assert!(FolderDropBackend::new(Path::new("/nonexistent/tessera"), Duration::ZERO).is_err());
    }
}
