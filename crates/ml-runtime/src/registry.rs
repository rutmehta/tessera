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
        })
    }
    pub fn models(&self) -> &[ModelSpec] {
        &self.models
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
    fn resolve_model(&self, spec: &ModelSpec) -> Result<ModelHandle> {
        let path = self.cache.join(format!("{}.onnx", spec.sha256));
        if !path.exists() {
            let mut tmp = tempfile::NamedTempFile::new_in(&self.cache)?;
            if spec.source == ModelSource::Local {
                let source = spec.local_path.as_ref().context("missing local_path")?;
                std::io::copy(&mut fs::File::open(self.base.join(source))?, &mut tmp)?;
            } else if let Some(source) = spec.download_url.strip_prefix("file:") {
                std::io::copy(&mut fs::File::open(self.base.join(source))?, &mut tmp)?;
            } else {
                let mut response = ureq::get(&spec.download_url).call()?;
                std::io::copy(&mut response.body_mut().as_reader(), &mut tmp)?;
            }
            tmp.flush()?;
            verify(tmp.path(), &spec.sha256)?;
            tmp.as_file().sync_all()?;
            tmp.persist(&path)?;
        }
        verify(&path, &spec.sha256)?;
        Ok(ModelHandle {
            spec: spec.clone(),
            path,
        })
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
