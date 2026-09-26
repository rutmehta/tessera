//! `tessera actions`: play recorded `.tessera-action` files on documents
//! (Photoshop's Batch: each group of inputs is opened, the action is played,
//! and the result is written to `--out-dir`).
use anyhow::{Context, Result, bail};
use clap::{Subcommand, ValueEnum};
use engine_api::document::{DocumentExportSettings, DocumentFormat};
use engine_api::tools::{
    DocumentToolCall, DocumentToolRequest, DocumentToolResponse, ExportFormat,
};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use tessera_mcp::Console;
use tessera_mcp::actions::ActionFile;

#[derive(Subcommand)]
pub enum Command {
    /// Play FILE on each input (or group of inputs, for multi-document
    /// actions) and write every result to --out-dir.
    Play {
        file: PathBuf,
        /// Documents: .tessera-doc, .psd/.psb, .jpg or .png.
        inputs: Vec<PathBuf>,
        /// Output directory (required when inputs are given).
        #[arg(long)]
        out_dir: Option<PathBuf>,
        /// Output container.
        #[arg(long, value_enum, default_value = "tessera-doc")]
        format: OutFormat,
    },
    /// Validate FILE and list its steps.
    Show { file: PathBuf },
}

#[derive(Clone, Copy, ValueEnum)]
pub enum OutFormat {
    TesseraDoc,
    Psd,
    Png,
    Png16,
    Jpeg,
    Tiff,
}

impl OutFormat {
    fn container(self) -> (DocumentFormat, &'static str) {
        let image = |encoding| DocumentFormat::Image {
            encoding,
            resize: None,
            profile: None,
        };
        match self {
            Self::TesseraDoc => (DocumentFormat::TesseraDoc, "tessera-doc"),
            Self::Psd => (
                DocumentFormat::Psd {
                    maximize_compatibility: true,
                },
                "psd",
            ),
            Self::Png => (image(ExportFormat::Png { bit_depth: 8 }), "png"),
            Self::Png16 => (image(ExportFormat::Png { bit_depth: 16 }), "png"),
            Self::Jpeg => (image(ExportFormat::Jpeg { quality: 92 }), "jpg"),
            Self::Tiff => (image(ExportFormat::Tiff { bit_depth: 16 }), "tif"),
        }
    }
}

fn absolute(path: &Path) -> Result<PathBuf> {
    path.canonicalize()
        .with_context(|| format!("resolve {}", path.display()))
}

pub fn run(app: &Path, command: &Command) -> Result<Value> {
    match command {
        Command::Show { file } => {
            let action = ActionFile::read(file)?;
            Ok(json!({
                "name": action.name,
                "inputs": action.inputs,
                "steps": action.steps.iter().map(|s| json!({
                    "command": s.action.command,
                    "enabled": s.enabled,
                    "rationale": s.rationale,
                })).collect::<Vec<_>>(),
            }))
        }
        Command::Play {
            file,
            inputs,
            out_dir,
            format,
        } => {
            let action = ActionFile::read(file)?;
            let per_run = action.inputs as usize;
            let groups: Vec<&[PathBuf]> = if per_run == 0 {
                if !inputs.is_empty() {
                    bail!("action `{}` takes no input documents", action.name);
                }
                vec![&[]]
            } else {
                if inputs.is_empty() || inputs.len() % per_run != 0 {
                    bail!(
                        "action `{}` takes {per_run} document(s) per run; got {} input(s)",
                        action.name,
                        inputs.len()
                    );
                }
                inputs.chunks(per_run).collect()
            };
            let out_dir = match out_dir {
                Some(d) => {
                    std::fs::create_dir_all(d)?;
                    Some(absolute(d)?)
                }
                None if per_run > 0 => bail!("--out-dir is required to keep the results"),
                None => None,
            };
            let mut console = Console::open(app)?;
            let mut runs = Vec::new();
            for group in groups {
                let mut documents = Vec::new();
                for input in group {
                    documents.push(
                        console
                            .open_document(absolute(input)?)
                            .with_context(|| format!("open {}", input.display()))?,
                    );
                }
                let report = console.play_action(&action, &documents)?;
                if let Some(failure) = &report.failed {
                    bail!(
                        "{}: step {} (`{}`) failed: {}",
                        group
                            .iter()
                            .map(|p| p.display().to_string())
                            .collect::<Vec<_>>()
                            .join(", "),
                        failure.step + 1,
                        action.steps[failure.step].action.command,
                        failure.error
                    );
                }
                let mut outputs = Vec::new();
                if let Some(dir) = &out_dir {
                    let (container, ext) = format.container();
                    for (input, document) in group.iter().zip(&documents) {
                        let stem = input
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .context("input file name must be UTF-8")?;
                        let target = dir.join(format!("{stem}.{ext}"));
                        if target.exists() {
                            bail!("{} already exists", target.display());
                        }
                        let response = console.execute_document(DocumentToolRequest {
                            call: DocumentToolCall::ExportDocument {
                                document: *document,
                                settings: DocumentExportSettings {
                                    path: target.to_str().context("UTF-8 path")?.into(),
                                    format: container.clone(),
                                },
                            },
                            rationale: Some(format!("batch output of action `{}`", action.name)),
                            group: None,
                            expect_head: None,
                        });
                        if let DocumentToolResponse::Error(e) = response {
                            bail!("export {}: {e}", target.display());
                        }
                        outputs.push(target);
                    }
                }
                runs.push(json!({
                    "inputs": group,
                    "documents": documents,
                    "steps": report.steps.len(),
                    "outputs": outputs,
                }));
            }
            Ok(json!({"action": action.name, "runs": runs}))
        }
    }
}
