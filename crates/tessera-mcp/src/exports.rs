use crate::{
    Console, catalog,
    console::{meta, record_empty},
    pixels::Source,
    unsupported,
};
use engine_api::{
    EngineError, EngineResult,
    id::{ImageId, JobId},
    tools::*,
};
use std::{
    collections::BTreeSet,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT_JOB: AtomicU64 = AtomicU64::new(1);
impl Console {
    pub(crate) fn index_folder(
        &mut self,
        path: &str,
        recursive: bool,
        request: &ToolRequest,
    ) -> EngineResult<ToolOutput> {
        if !recursive {
            return Err(unsupported("index::Index::scan has no non-recursive mode"));
        }
        if request.expect_recipe.is_some() {
            return Err(EngineError::invalid(
                "expect_recipe",
                "index_folder has no target recipe",
            ));
        }
        let root = Path::new(path);
        if !root.is_absolute() {
            return Err(EngineError::invalid("path", "absolute folder required"));
        }
        let root = root.canonicalize()?;
        self.index.scan(&root, &catalog::Reader, &catalog::Reader)?;
        let mut found = 0;
        for image in self.index.search(&index::Query {
            limit: i64::MAX as usize,
            ..Default::default()
        })? {
            let (path, mut doc) = self.document(image)?;
            if !path.starts_with(&root) {
                continue;
            }
            record_empty(&mut doc, meta(request));
            self.save(&path, &mut doc)?;
            found += 1;
        }
        Ok(ToolOutput::Indexing {
            job: JobId(NEXT_JOB.fetch_add(1, Ordering::Relaxed)),
            files_found: found,
        })
    }
    pub(crate) fn export_images(
        &mut self,
        images: &[ImageId],
        settings: &ExportSettings,
        request: &ToolRequest,
    ) -> EngineResult<ToolOutput> {
        if images.is_empty() {
            return Err(EngineError::invalid("images", "must not be empty"));
        }
        if settings.profile.is_some() || settings.hdr {
            return Err(unsupported(
                "custom ICC handles and HDR export need a registry/output mapping; only sRGB SDR is supported",
            ));
        }
        let format = match settings.format {
            ExportFormat::Jpeg { quality: 1..=100 } => {
                if let ExportFormat::Jpeg { quality } = settings.format {
                    export::Format::Jpeg { quality }
                } else {
                    unreachable!()
                }
            }
            ExportFormat::Png { bit_depth: 8 } => export::Format::Png,
            ExportFormat::Tiff { bit_depth: 8 | 16 } => {
                if let ExportFormat::Tiff { bit_depth } = settings.format {
                    export::Format::Tiff { bits: bit_depth }
                } else {
                    unreachable!()
                }
            }
            _ => {
                return Err(unsupported(
                    "export supports JPEG quality 1..=100, PNG 8-bit, TIFF 8/16-bit",
                ));
            }
        };
        let resize = match settings.resize {
            None => export::Resize::None,
            Some(Resize::LongEdge { pixels }) => export::Resize::LongEdge(pixels),
            Some(Resize::Within { width, height }) => export::Resize::Fit(width, height),
            Some(Resize::Megapixels { .. }) => {
                return Err(unsupported(
                    "megapixel resize is not exposed by export::Resize",
                ));
            }
        };
        let options = export::ExportSettings {
            format,
            resize,
            naming: settings.name_template.clone(),
            output_dir: settings.destination.clone().into(),
            metadata: if settings.embed_metadata {
                export::Metadata::All
            } else {
                export::Metadata::None
            },
            ..Default::default()
        };
        let extension = match format {
            export::Format::Jpeg { .. } => "jpg",
            export::Format::Png => "png",
            export::Format::Tiff { .. } => "tif",
        };
        let mut pending = Vec::new();
        let mut names = BTreeSet::new();
        for image in images.iter().copied().collect::<BTreeSet<_>>() {
            let (path, doc) = self.document(image)?;
            self.check(&doc, request)?;
            catalog::writable(&path)?;
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| EngineError::invalid("path", "UTF-8 filename required"))?
                .to_owned();
            let target = options.output_dir.join(export::filename(
                &options.naming,
                &name,
                pending.len() + 1,
                "",
                extension,
            )?);
            if target.exists()
                || sidecar::Sidecar::paths(&target).xmp.exists()
                || !names.insert(target.clone())
            {
                return Err(EngineError::invalid("destination", "output collision"));
            }
            pending.push((image, path, doc, name));
        }
        let count = pending.len() as u32;
        for (sequence, (image, path, mut doc, name)) in pending.into_iter().enumerate() {
            let source = Source::open(&path)?;
            let output = export::export_one(
                &export::ExportImage {
                    source: source.borrowed(),
                    name: &name,
                    sequence: sequence + 1,
                    date: "",
                    metadata: None,
                },
                &doc.recipe,
                &options,
            )?;
            record_empty(&mut doc, meta(request));
            self.save(&path, &mut doc)?;
            self.index
                .record_export(image, &output.to_string_lossy(), false)?;
        }
        Ok(ToolOutput::ExportQueued {
            job: JobId(NEXT_JOB.fetch_add(1, Ordering::Relaxed)),
            images: count,
        })
    }
}
