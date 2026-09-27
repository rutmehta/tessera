//! Explicit model acquisition, independent of catalog and renderer lifetimes.
use crate::{Result, failure};
use engine_api::id::ModelRef;
use ml_runtime::ModelRegistry;
use std::sync::Arc;

#[derive(Clone, Debug, uniffi::Enum)]
pub enum ModelDownloadEvent {
    Queued,
    Downloading { bytes: u64, total: Option<u64> },
    Ready { path: String },
    Failed { reason: String },
}

/// Events for one request arrive in order on its worker thread. No engine or
/// registry locks are held during callbacks. Retained until the terminal event.
#[uniffi::export(with_foreign)]
pub trait ModelDownloadListener: Send + Sync {
    fn on_event(&self, event: ModelDownloadEvent);
}

#[derive(uniffi::Object)]
pub struct ModelDownloads {
    registry: Arc<ModelRegistry>,
    allow_downloads: bool,
}

#[uniffi::export]
impl ModelDownloads {
    #[uniffi::constructor]
    pub fn open(
        manifest_path: String,
        cache_path: String,
        allow_downloads: bool,
    ) -> Result<Arc<Self>> {
        Ok(Arc::new(Self {
            registry: Arc::new(ModelRegistry::open(manifest_path, cache_path).map_err(failure)?),
            allow_downloads,
        }))
    }

    /// Returns after scheduling. Each accepted request emits Queued, zero or
    /// more Downloading events, then exactly one Ready or Failed. A cache hit
    /// is digest-verified even when downloads are disabled. Dropping this
    /// object does not cancel accepted requests. Use a listener per request.
    pub fn request(
        &self,
        id: String,
        version: String,
        listener: Arc<dyn ModelDownloadListener>,
    ) -> Result<()> {
        let registry = self.registry.clone();
        let allow_downloads = self.allow_downloads;
        std::thread::Builder::new()
            .name("model-download".into())
            .spawn(move || {
                listener.on_event(ModelDownloadEvent::Queued);
                let model = ModelRef {
                    id: id.as_str().into(),
                    version,
                };
                let result = registry.download(&model, allow_downloads, |bytes, total| {
                    listener.on_event(ModelDownloadEvent::Downloading { bytes, total });
                });
                let terminal = match result {
                    Ok(handle) => ModelDownloadEvent::Ready {
                        path: handle.path().to_string_lossy().into_owned(),
                    },
                    Err(error) => ModelDownloadEvent::Failed {
                        reason: format!("{error:#}"),
                    },
                };
                listener.on_event(terminal);
            })
            .map_err(failure)?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/models/downloads.rs"]
mod tests;
