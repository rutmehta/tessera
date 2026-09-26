use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use index::{Index, Query};
use serde_json::{Value, json};
use std::{path::PathBuf, process::ExitCode, time::Instant};
mod agent_edit;
mod catalog;
mod develop;
mod export;
mod import;
mod media;
mod models;
mod understanding;

#[derive(Parser)]
#[command(name = "tessera", version, about = "Headless photo workflow")]
struct Cli {
    #[arg(long, global = true)]
    app_dir: Option<PathBuf>,
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    #[command(subcommand)]
    Agent(agent_edit::Command),
    /// Serve engine tools through the tessera-mcp stdio executable.
    Mcp,
    #[command(subcommand)]
    Import(Import),
    #[command(subcommand)]
    Ml(Ml),
    Export(export::Options),
    Render {
        image: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value = "1", value_parser = ["1/8", "1/4", "1/2", "1"])]
        scale: String,
        /// DevelopSettings JSON, inline or a path to a JSON file. Replaces sidecar settings.
        #[arg(long)]
        settings: Option<String>,
        /// Auto uses Adobe compatibility for sidecar PV3–6; native/adobe override it.
        #[arg(long, value_enum, default_value = "auto")]
        process: media::Process,
        /// Explicit user-supplied camera profile, used only by Adobe rendering.
        #[arg(long)]
        dcp: Option<PathBuf>,
    },
    Preview {
        image: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value_t = 1024, value_parser = clap::value_parser!(u32).range(1..))]
        max: u32,
    },
    #[command(subcommand)]
    Develop(Develop),
    #[command(subcommand)]
    Cull(Cull),
    Index {
        dir: Option<PathBuf>,
        #[command(subcommand)]
        command: Option<IndexCommand>,
    },
    Ls {
        #[arg(long)]
        query: Option<String>,
        #[arg(long, value_parser = ["keep", "reject", "undecided"])]
        decision: Option<String>,
    },
}
#[derive(Subcommand)]
enum IndexCommand {
    /// Remove catalog rows for files that no longer exist on disk.
    Prune {
        #[arg(long)]
        dry_run: bool,
    },
}
#[derive(Subcommand)]
enum Ml {
    Models,
    Check,
    Keywords(understanding::KeywordOptions),
    Caption(understanding::Options),
    Ocr(understanding::Options),
}
#[derive(Subcommand)]
enum Import {
    Lrcat {
        #[arg(required_unless_present = "make_fixture")]
        file: Option<PathBuf>,
        #[arg(
            long,
            conflicts_with = "apply",
            required_unless_present_any = ["apply", "make_fixture", "fidelity"]
        )]
        inspect: bool,
        /// Write the synthetic test catalog (JPEG originals, Previews.lrdata) into DIR.
        #[arg(long, value_name = "DIR", conflicts_with_all = ["inspect", "apply", "file", "fidelity", "reference_dir"])]
        make_fixture: Option<PathBuf>,
        #[arg(long, requires = "dest")]
        apply: bool,
        #[arg(long, requires = "apply")]
        dest: Option<PathBuf>,
        /// Compare RAW compatibility renders with user-supplied Lightroom exports.
        #[arg(long, requires = "reference_dir")]
        fidelity: bool,
        /// Directory of sRGB exports named <catalog_id>.jpg.
        #[arg(long, requires = "fidelity")]
        reference_dir: Option<PathBuf>,
    },
}
#[derive(Subcommand)]
enum Develop {
    Set {
        image: PathBuf,
        #[command(flatten)]
        basic: develop::Basic,
    },
    Show {
        image: PathBuf,
    },
}
#[derive(Subcommand)]
enum Cull {
    Set {
        image: PathBuf,
        #[arg(long, value_parser = ["X", "U", "P"])]
        decision: String,
        #[arg(long, value_parser = clap::value_parser!(u8).range(1..=3))]
        grade: Option<u8>,
        #[arg(long)]
        mark: Option<String>,
    },
    Groups {
        dir: PathBuf,
    },
    Sweep {
        dir: PathBuf,
        /// Repeat SIGNAL=VALUE; report scores below this value.
        #[arg(long)]
        below: Vec<String>,
        /// Repeat SIGNAL=VALUE; report scores above this value.
        #[arg(long)]
        above: Vec<String>,
    },
}
fn all() -> Query {
    Query {
        limit: i64::MAX as usize,
        ..Default::default()
    }
}
fn run(cli: &Cli) -> Result<Value> {
    let app = match &cli.app_dir {
        Some(path) => path.clone(),
        None => match std::env::var_os("TESSERA_APP_DIR").filter(|value| !value.is_empty()) {
            Some(path) => PathBuf::from(path),
            None => {
                PathBuf::from(std::env::var_os("HOME").context("HOME is not set; use --app-dir")?)
                    .join("Library/Application Support/Tessera")
            }
        },
    };
    if matches!(cli.command, Command::Mcp) {
        let executable = std::env::var_os("TESSERA_MCP_BIN")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::current_exe()
                    .ok()
                    .map(|p| p.with_file_name("tessera-mcp"))
                    .filter(|p| p.is_file())
            })
            .unwrap_or_else(|| PathBuf::from("tessera-mcp"));
        let mut command = std::process::Command::new(executable);
        command.arg("--app-dir").arg(&app);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            return Err(command.exec()).context(
                "exec tessera-mcp; install the tessera-mcp binary beside tessera or on PATH",
            );
        }
        #[cfg(not(unix))]
        {
            let status = command.status().context("start tessera-mcp")?;
            std::process::exit(status.code().unwrap_or(1));
        }
    }
    // Catalog import/inspection/fidelity never needs or mutates the local index.
    if let Command::Import(Import::Lrcat {
        file,
        inspect,
        apply,
        dest,
        fidelity,
        reference_dir,
        make_fixture,
    }) = &cli.command
    {
        if let Some(dir) = make_fixture {
            let fixture = import_lrcat::fixture::write(dir)?;
            return Ok(json!({
                "catalog": fixture.catalog,
                "photos": fixture.photos,
                "previews": fixture.previews,
                "moved_root": import_lrcat::fixture::MOVED_ROOT,
            }));
        }
        let file = file.as_ref().context("a catalog path is required")?;
        let mut result = if *inspect {
            serde_json::to_value(import_lrcat::inspect(file)?)?
        } else if *apply {
            import::apply(file, dest.as_ref().context("--apply requires --dest")?)?
        } else {
            json!({})
        };
        if *fidelity {
            result["fidelity"] = import::fidelity(
                file,
                reference_dir
                    .as_ref()
                    .context("--fidelity requires --reference-dir")?,
            )?;
        }
        return Ok(result);
    }
    std::fs::create_dir_all(&app)?;
    let mut index = Index::open(app.join("index.sqlite"))?;
    match &cli.command {
        Command::Agent(command) => agent_edit::run(&app, &mut index, command),
        Command::Mcp => unreachable!("MCP replaces this process before opening the catalog"),
        Command::Export(options) => export::run(&index, &app, options),
        Command::Ml(command) => match command {
            Ml::Models | Ml::Check => models::run(&app, matches!(command, Ml::Check)),
            _ => understanding::run(&app, &index, command),
        },
        Command::Import(_) => unreachable!("import handled before opening index"),
        Command::Render {
            image,
            out,
            scale,
            settings,
            process,
            dcp,
        } => {
            let recipe = catalog::document(image)?.recipe;
            let adobe = process.uses_adobe(recipe.process_version);
            anyhow::ensure!(
                dcp.is_none() || adobe,
                "DCP requires Adobe rendering (--process adobe)"
            );
            let settings = match settings {
                Some(text) => {
                    serde_json::from_str::<engine_api::recipe::DevelopSettings>(&if text
                        .trim_start()
                        .starts_with('{')
                    {
                        text.clone()
                    } else {
                        std::fs::read_to_string(text)?
                    })?
                }
                None => recipe.settings,
            };
            let level = match scale.as_str() {
                "1/8" => 3,
                "1/4" => 2,
                "1/2" => 1,
                _ => 0,
            };
            if adobe {
                media::render_adobe(image, out, 1 << level, &settings, dcp.as_deref())?;
            } else {
                media::render(image, out, level, &settings)?;
            }
            Ok(json!({"out":out,"process":if adobe { "adobe" } else { "native" }}))
        }
        Command::Preview { image, out, max } => {
            anyhow::ensure!(
                image::ImageFormat::from_path(out)? == image::ImageFormat::Jpeg,
                "preview output must be JPEG"
            );
            media::preview(image, out, *max)?;
            Ok(json!({"out":out}))
        }
        Command::Index {
            dir: Some(dir),
            command: None,
        } => {
            let start = Instant::now();
            let changed = index.scan(dir, &catalog::Reader, &catalog::RawMetadata)?;
            Ok(
                json!({"changed":changed,"total":index.search(&all())?.len(),"ms":start.elapsed().as_secs_f64()*1000.}),
            )
        }
        Command::Index {
            dir: None,
            command: Some(IndexCommand::Prune { dry_run }),
        } => {
            let counts = index.prune_missing(*dry_run)?;
            Ok(json!({"images":counts.images,"files":counts.files,"dry_run":dry_run}))
        }
        Command::Index { .. } => anyhow::bail!("use index DIR or index prune [--dry-run]"),
        Command::Cull(command) => culling(&mut index, command),
        Command::Develop(command) => {
            let image = match command {
                Develop::Set { image, .. } | Develop::Show { image } => image,
            };
            let path = if image.is_file() {
                image.canonicalize()?
            } else {
                catalog::resolve(&index, image)?.1
            };
            let settings = match command {
                Develop::Set { basic, .. } => develop::set(&path, basic)?,
                Develop::Show { .. } => catalog::document(&path)?.recipe.settings,
            };
            Ok(serde_json::to_value(settings)?)
        }
        Command::Ls { query, decision } => {
            let q = Query {
                text: query.clone(),
                decision: decision.as_deref().map(|d| match d {
                    "keep" => cull::Decision::Keep,
                    "reject" => cull::Decision::Reject,
                    _ => cull::Decision::Undecided,
                }),
                ..all()
            };
            let rows = index
                .search(&q)?
                .into_iter()
                .map(|id| -> Result<Value> {
                    let info = index.image_info(id)?;
                    Ok(json!({"id":id,"path":info.path,"selection":index.selection(id)?}))
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(json!(rows))
        }
    }
}
fn culling(index: &mut Index, command: &Cull) -> Result<Value> {
    use cull::{CullSession, Decision, Source, Threshold};
    match command {
        Cull::Set {
            image,
            decision,
            grade,
            mark,
        } => {
            anyhow::ensure!(
                grade.is_none() || decision == "P",
                "--grade requires --decision P"
            );
            let (id, path) = catalog::resolve(index, image)?;
            catalog::check_identity(&catalog::document(&path)?.recipe, id)?;
            let mut session = CullSession::open(index, all())?;
            session.set_position(
                session
                    .images()
                    .iter()
                    .position(|item| *item == id)
                    .context("image missing from cull queue")?,
            )?;
            session.set_auto_advance(false);
            session.decide(match decision.as_str() {
                "X" => Decision::Reject,
                "P" => Decision::Keep,
                _ => Decision::Undecided,
            })?;
            if let Some(grade) = grade {
                session.grade(*grade)?;
            }
            if let Some(mark) = mark {
                session.mark(mark)?;
            }
            Ok(json!({"id":id,"selection":index.selection(id)?}))
        }
        Cull::Groups { dir } | Cull::Sweep { dir, .. } => {
            index.scan(dir, &catalog::Reader, &catalog::RawMetadata)?;
            let session = CullSession::open(index, Source::Folder(dir.clone()))?;
            let warnings: Vec<_> = session
                .preview_errors()
                .iter()
                .map(|(id, e)| json!({"id":id,"error":e.to_string()}))
                .collect();
            if let Cull::Sweep { below, above, .. } = command {
                let mut thresholds = Vec::new();
                for (values, below) in [(below, true), (above, false)] {
                    for value in values {
                        let (signal, number) = value
                            .split_once('=')
                            .context("threshold must be SIGNAL=VALUE")?;
                        thresholds.push(if below {
                            Threshold::below(signal, number.parse()?)
                        } else {
                            Threshold::above(signal, number.parse()?)
                        });
                    }
                }
                if thresholds.is_empty() {
                    thresholds = vec![
                        Threshold::below("sharpness", 0.2),
                        Threshold::above("motion_blur", 0.8),
                        Threshold::below("exposure", 0.2),
                        Threshold::above("noise", 0.8),
                    ];
                }
                let defects: Vec<_> = session.defect_sweep(&thresholds)?.into_iter().map(|(id, reasons)| {
                    let reasons: Vec<_> = reasons.iter().map(|r| json!({"signal":r.signal,"value":r.value,"threshold":r.threshold,"direction":format!("{:?}",r.direction),"model":r.model})).collect();
                    json!({"id":id,"reasons":reasons})
                }).collect();
                Ok(json!({"defects":defects,"warnings":warnings}))
            } else {
                Ok(
                    json!({"groups":session.groups().iter().map(|g| json!({"images":g.images})).collect::<Vec<_>>(),"warnings":warnings}),
                )
            }
        }
    }
}
fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(value) => {
            println!(
                "{}",
                if cli.json {
                    value.to_string()
                } else {
                    serde_json::to_string_pretty(&value).expect("JSON value")
                }
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    #[test]
    fn fidelity_flags_compose_with_inspect_or_apply() {
        for extra in [
            vec![],
            vec!["--inspect"],
            vec!["--apply", "--dest", "bundle"],
        ] {
            let mut args = vec![
                "tessera",
                "import",
                "lrcat",
                "test.lrcat",
                "--fidelity",
                "--reference-dir",
                "refs",
            ];
            args.extend(extra);
            assert!(Cli::try_parse_from(args).is_ok());
        }
        for extra in [
            vec![],
            vec!["--fidelity"],
            vec!["--reference-dir", "refs"],
            vec!["--fidelity", "--reference-dir", "refs", "--apply"],
        ] {
            let mut args = vec!["tessera", "import", "lrcat", "test.lrcat"];
            args.extend(extra);
            assert!(Cli::try_parse_from(args).is_err());
        }
        let cli = Cli::try_parse_from(["tessera", "render", "in.dng", "--out", "out.png"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Render {
                process: media::Process::Auto,
                ..
            }
        ));
    }
}
