use anyhow::{Context, Result, ensure};
use engine_api::id::ModelRef;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Dtype {
    Fp32,
    Fp16,
    Int8,
    Int64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TensorSpec {
    pub name: String,
    pub shape: Vec<usize>,
    pub dtype: Dtype,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelSpec {
    pub id: String,
    pub version: String,
    pub task: String,
    pub dtype: Dtype,
    pub inputs: Vec<TensorSpec>,
    pub outputs: Vec<TensorSpec>,
    pub sha256: String,
    #[serde(default)]
    pub download_url: String,
    #[serde(default)]
    pub source: ModelSource,
    #[serde(default)]
    pub local_path: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ModelSource {
    #[default]
    Url,
    Local,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    models: Vec<ModelSpec>,
}
#[derive(Debug)]
pub struct ModelHandle {
    spec: ModelSpec,
    path: PathBuf,
}
impl ModelHandle {
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn spec(&self) -> &ModelSpec {
        &self.spec
    }
    pub fn model_ref(&self) -> ModelRef {
        ModelRef {
            id: self.spec.id.as_str().into(),
            version: self.spec.version.clone(),
        }
    }
}
/// Version-pinned registry with an atomically populated, SHA-256 addressed cache.
/// Resolving is explicit: only a cache miss can cause a network download.
pub struct ModelRegistry {
    models: Vec<ModelSpec>,
    base: PathBuf,
    cache: PathBuf,
    allow_downloads: bool,
}
impl ModelRegistry {
    pub fn open(manifest: impl AsRef<Path>, cache: impl AsRef<Path>) -> Result<Self> {
        let manifest = manifest.as_ref().canonicalize()?;
        let parsed: Manifest = toml::from_str(&fs::read_to_string(&manifest)?)?;
        let mut keys = HashSet::new();
        for m in &parsed.models {
            ensure!(
                !m.id.is_empty() && !m.version.is_empty() && !m.task.is_empty(),
                "empty model identity/task"
            );
            ensure!(keys.insert((&m.id, &m.version)), "duplicate model/version");
            ensure!(
                m.sha256.len() == 64
                    && m.sha256
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
                "sha256 must be 64 lowercase hex characters"
            );
            ensure!(
                !m.inputs.is_empty() && !m.outputs.is_empty(),
                "missing tensor specs"
            );
            for specs in [&m.inputs, &m.outputs] {
                let mut names = HashSet::new();
                for s in specs {
                    ensure!(
                        !s.name.is_empty() && names.insert(&s.name),
                        "missing/duplicate tensor name"
                    );
                    ensure!(
                        !s.shape.is_empty() && s.shape.iter().all(|&d| d > 0),
                        "tensor specs need concrete probe shapes"
                    );
                    ensure!(
                        s.shape
                            .iter()
                            .try_fold(1usize, |a, b| a.checked_mul(*b))
                            .is_some(),
                        "tensor shape overflow"
                    );
                }
            }
            match m.source {
                ModelSource::Local => ensure!(
                    m.download_url.is_empty()
                        && m.local_path
                            .as_ref()
                            .is_some_and(|p| !p.as_os_str().is_empty()),
                    "local models need only local_path, not download_url"
                ),
                ModelSource::Url => ensure!(
                    m.local_path.is_none()
                        && (m.download_url.starts_with("https://")
                            || m.download_url.starts_with("file:")),
                    "URL models need https:// or file: and no local_path"
                ),
            }
        }
        fs::create_dir_all(cache.as_ref())?;
        Ok(Self {
            models: parsed.models,
            base: manifest.parent().context("manifest parent")?.into(),
            cache: cache.as_ref().into(),
            allow_downloads: true,
        })
    }
    /// Policy for adapter-driven resolve calls. Explicit download requests carry
    /// their own policy, so a UI can populate a cache used by offline adapters.
    pub fn with_downloads_allowed(mut self, allowed: bool) -> Self {
        self.allow_downloads = allowed;
        self
    }

    /// Shared application model cache. Automatic renderers must remain offline:
    /// the FFI downloader explicitly populates this same cache on user request.
    pub fn from_support(support: &Path) -> Result<Self> {
        let dir = support.join("models");
        fs::create_dir_all(&dir)?;
        let manifest = dir.join("models.toml");
        let text = include_str!("../models.toml");
        if fs::read_to_string(&manifest).ok().as_deref() != Some(text) {
            let mut temp = tempfile::NamedTempFile::new_in(&dir)?;
            temp.write_all(text.as_bytes())?;
            temp.persist(&manifest)?;
        }
        Ok(Self::open(manifest, dir.join("cache"))?.with_downloads_allowed(false))
    }

    pub fn models(&self) -> &[ModelSpec] {
        &self.models
    }
    /// Select ONLY when enabling a new edit, never while replaying a recipe.
    /// Optional research CFA weights must already be cached or locally present.
    /// Local import is digest verified and never performs network I/O.
    pub fn preferred_ai_denoise(&self) -> Result<ModelRef> {
        for id in ["enhance/cfa-unet-fp32", "enhance/cfa-unet-fp16"] {
            if let Some(spec) = self.models.iter().find(|s| s.id == id) {
                let model = ModelRef {
                    id: id.into(),
                    version: spec.version.clone(),
                };
                if self.resolve_cached_ref(&model)?.is_some() {
                    return Ok(model);
                }
                if spec.source == ModelSource::Local
                    && let Some(path) = &spec.local_path
                    && self.base.join(path).try_exists()?
                {
                    self.download(&model, true, |_, _| {})?;
                    return Ok(model);
                }
            }
        }
        let spec = self
            .models
            .iter()
            .find(|s| s.id == "enhance/drunet-color")
            .context("missing production AI Denoise model")?;
        Ok(ModelRef {
            id: spec.id.as_str().into(),
            version: spec.version.clone(),
        })
    }
    /// Rejects ambiguous IDs: use resolve_ref when multiple versions are installed.
    pub fn resolve(&self, id: &str) -> Result<ModelHandle> {
        let mut matches = self.models.iter().filter(|m| m.id == id);
        let model = matches.next().context("unknown model id")?;
        ensure!(
            matches.next().is_none(),
            "ambiguous model id; pin a ModelRef"
        );
        self.resolve_model(model)
    }
    pub fn resolve_ref(&self, model: &ModelRef) -> Result<ModelHandle> {
        let spec = self
            .models
            .iter()
            .find(|m| m.id == model.id.as_str() && m.version == model.version)
            .context("unknown model version")?;
        self.resolve_model(spec)
    }
    /// Cache-only lookup for automatic backend selection. Never downloads or
    /// imports local sources. Missing is distinct from corrupt/unreadable:
    /// callers may fall back on None, but must not hide integrity failures.
    pub fn resolve_cached_ref(&self, model: &ModelRef) -> Result<Option<ModelHandle>> {
        let spec = self
            .models
            .iter()
            .find(|m| m.id == model.id.as_str() && m.version == model.version)
            .context("unknown model version")?;
        let path = self.cache.join(format!("{}.onnx", spec.sha256));
        match fs::metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            other => {
                other?;
            }
        }
        verify(&path, &spec.sha256)?;
        Ok(Some(ModelHandle {
            spec: spec.clone(),
            path,
        }))
    }
    /// Explicit, verified acquisition. A cache hit is allowed with downloads
    /// disabled. Progress reports bytes copied; unknown HTTP lengths are None.
    /// The final notification happens after digest verification and atomic install.
    pub fn download(
        &self,
        model: &ModelRef,
        allow_downloads: bool,
        mut progress: impl FnMut(u64, Option<u64>),
    ) -> Result<ModelHandle> {
        if let Some(handle) = self.resolve_cached_ref(model)? {
            return Ok(handle);
        }
        ensure!(
            allow_downloads,
            "model is not cached and downloads are disabled"
        );
        let spec = self
            .models
            .iter()
            .find(|s| s.id == model.id.as_str() && s.version == model.version)
            .context("unknown model version")?;
        let path = self.cache.join(format!("{}.onnx", spec.sha256));
        let mut tmp = tempfile::NamedTempFile::new_in(&self.cache)?;
        let mut copy = |reader: &mut dyn Read, total: Option<u64>| -> Result<u64> {
            let mut bytes = 0u64;
            let mut buffer = [0u8; 65536];
            progress(0, total);
            loop {
                let n = reader.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                tmp.write_all(&buffer[..n])?;
                bytes += n as u64;
                progress(bytes, total);
            }
            if let Some(total) = total {
                ensure!(total == bytes, "model download length mismatch");
            }
            Ok(bytes)
        };
        let bytes = if spec.source == ModelSource::Local {
            let source = spec.local_path.as_ref().context("missing local_path")?;
            let mut file = fs::File::open(self.base.join(source))?;
            let total = file.metadata()?.len();
            copy(&mut file, Some(total))?
        } else if let Some(source) = spec.download_url.strip_prefix("file:") {
            let mut file = fs::File::open(self.base.join(source))?;
            let total = file.metadata()?.len();
            copy(&mut file, Some(total))?
        } else {
            let mut response = ureq::get(&spec.download_url).call()?;
            let total = response
                .headers()
                .get("content-length")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse().ok());
            copy(&mut response.body_mut().as_reader(), total)?
        };
        tmp.flush()?;
        verify(tmp.path(), &spec.sha256)?;
        tmp.as_file().sync_all()?;
        tmp.persist(&path)?;
        progress(bytes, Some(bytes));
        Ok(ModelHandle {
            spec: spec.clone(),
            path,
        })
    }

    fn resolve_model(&self, spec: &ModelSpec) -> Result<ModelHandle> {
        self.download(
            &ModelRef {
                id: spec.id.as_str().into(),
                version: spec.version.clone(),
            },
            self.allow_downloads,
            |_, _| {},
        )
    }
}
fn verify(path: &Path, expected: &str) -> Result<()> {
    let mut file = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    ensure!(
        format!("{:x}", hash.finalize()) == expected,
        "model SHA-256 mismatch: {}",
        path.display()
    );
    Ok(())
}

#[cfg(test)]
mod download_tests {
    use super::*;

    #[test]
    fn ai_denoise_prefers_available_local_or_cached_cfa() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let cache = dir.path().join("cache");
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("models.toml");
        let mut text = fs::read_to_string(source)?;
        // Use genuine digest-verified fixture bytes, not a fake model session.
        let fixture = include_bytes!("../tests/data/conv.onnx");
        let sha = format!("{:x}", Sha256::digest(fixture));
        text = text
            .replace(
                "a138c59a65846c10967839e85817231ec6ea92b318a57814cb153e8ac8bb311b",
                &sha,
            )
            .replace(
                "../../tools/orchestrate/wp/M3-16/artifacts/cfa-fp32.onnx",
                "research.onnx",
            );
        let manifest = dir.path().join("models.toml");
        fs::write(&manifest, text)?;
        let registry = ModelRegistry::open(&manifest, &cache)?.with_downloads_allowed(false);
        assert_eq!(
            registry.preferred_ai_denoise()?.id.as_str(),
            "enhance/drunet-color"
        );
        fs::write(dir.path().join("research.onnx"), fixture)?;
        let selected = registry.preferred_ai_denoise()?;
        assert_eq!(selected.id.as_str(), "enhance/cfa-unet-fp32");
        assert!(registry.resolve_cached_ref(&selected)?.is_some());
        fs::remove_file(dir.path().join("research.onnx"))?;
        assert_eq!(registry.preferred_ai_denoise()?, selected);
        Ok(())
    }

    #[test]
    fn adapter_policy_prevents_implicit_downloads() -> Result<()> {
        let cache = tempfile::tempdir()?;
        let registry = ModelRegistry::open(
            concat!(env!("CARGO_MANIFEST_DIR"), "/models.toml"),
            cache.path(),
        )?
        .with_downloads_allowed(false);
        assert!(
            registry
                .resolve("test/conv")
                .unwrap_err()
                .to_string()
                .contains("downloads are disabled")
        );
        let model = ModelRef {
            id: "test/conv".into(),
            version: "1".into(),
        };
        registry.download(&model, true, |_, _| {})?;
        assert!(registry.resolve_ref(&model).is_ok());
        let support = tempfile::tempdir()?;
        let automatic = ModelRegistry::from_support(support.path())?;
        assert!(support.path().join("models/models.toml").exists());
        assert!(support.path().join("models/cache").is_dir());
        assert!(
            automatic
                .resolve_ref(&model)
                .unwrap_err()
                .to_string()
                .contains("downloads are disabled")
        );
        Ok(())
    }

    #[test]
    fn explicit_download_policy_and_progress_verify_cache() -> Result<()> {
        let cache = tempfile::tempdir()?;
        let registry = ModelRegistry::open(
            concat!(env!("CARGO_MANIFEST_DIR"), "/models.toml"),
            cache.path(),
        )?;
        let model = ModelRef {
            id: "test/conv".into(),
            version: "1".into(),
        };
        let mut events = Vec::new();
        assert!(
            registry
                .download(&model, false, |done, total| events.push((done, total)))
                .is_err()
        );
        assert!(events.is_empty());
        assert_eq!(fs::read_dir(cache.path())?.count(), 0);
        let handle = registry.download(&model, true, |done, total| events.push((done, total)))?;
        let bytes = fs::metadata(handle.path())?.len();
        assert!(bytes > 0);
        assert_eq!(events.last(), Some(&(bytes, Some(bytes))));
        events.clear();
        registry.download(&model, false, |done, total| events.push((done, total)))?;
        assert!(events.is_empty());
        fs::write(handle.path(), b"corrupt")?;
        assert!(registry.download(&model, false, |_, _| {}).is_err());
        Ok(())
    }
}
