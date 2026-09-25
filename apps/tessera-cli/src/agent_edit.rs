use agent::{
    Agent, BatchInput, Config,
    providers::{AnthropicMessages, Ollama, OpenAiResponses, Planner},
};
use anyhow::{Result, ensure};
use clap::{Args, Subcommand};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};
use style_profile::{Profile, Questionnaire};

#[derive(Subcommand)]
pub enum Command {
    Edit(Options),
}
#[derive(Args)]
pub struct Options {
    pub image: PathBuf,
    /// Explicit opt-in to cloud/local model calls. Omitted = style-profile only.
    #[arg(long,value_parser=["none","anthropic","openai","ollama"])]
    pub provider: Option<String>,
    #[arg(long)]
    pub model: Option<String>,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub redo: Option<String>,
    #[arg(long, default_value = "default")]
    pub library: String,
    #[arg(long,default_value_t=3,value_parser=clap::value_parser!(u32).range(1..=20))]
    pub iterations: u32,
    #[arg(long,default_value_t=60,value_parser=clap::value_parser!(u64).range(1..))]
    pub budget_seconds: u64,
    #[arg(long)]
    pub visual_critic: bool,
}
pub fn run(app: &Path, index: &mut index::Index, command: &Command) -> Result<Value> {
    let Command::Edit(options) = command;
    let mut planner: Option<Box<dyn Planner>> = match options.provider.as_deref().unwrap_or("none")
    {
        "anthropic" => {
            let mut p = AnthropicMessages::from_env()?;
            if let Some(m) = &options.model {
                p.model = m.clone();
            }
            Some(Box::new(p))
        }
        "openai" => {
            let mut p = OpenAiResponses::from_env()?;
            if let Some(m) = &options.model {
                p.model = m.clone();
            }
            Some(Box::new(p))
        }
        "ollama" => {
            let mut p = Ollama::from_env()?;
            if let Some(m) = &options.model {
                p.model = m.clone();
            }
            Some(Box::new(p))
        }
        _ => None,
    };
    ensure!(
        !options.visual_critic || planner.is_some(),
        "--visual-critic requires a provider"
    );
    let profile = Profile::open(app, &options.library)?
        .unwrap_or(Profile::new(&options.library, Questionnaire::default())?);
    let mut agent = Agent::open(
        app,
        profile,
        Config {
            max_iterations: options.iterations as usize,
            time_budget: Duration::from_secs(options.budget_seconds),
            visual_critic: options.visual_critic,
            ..Default::default()
        },
    )?;
    let queue = if options.image.is_dir() {
        let root = options.image.canonicalize()?;
        index.scan(&root, &crate::catalog::Reader, &crate::catalog::RawMetadata)?;
        let session = cull::CullSession::open(index, cull::Source::Folder(root))?;
        let mut groups = BTreeMap::new();
        for (ordinal, group) in session.groups().iter().enumerate() {
            for id in &group.images {
                groups.insert(*id, format!("sequence-{ordinal}"));
            }
        }
        let inputs = session
            .images()
            .iter()
            .map(|id| {
                Ok(BatchInput {
                    path: index.image_info(*id)?.path,
                    burst: groups.get(id).cloned(),
                    people: vec![],
                })
            })
            .collect::<Result<Vec<_>>>()?;
        ensure!(!inputs.is_empty(), "no indexed images in directory");
        agent.edit_batch(
            &inputs,
            planner.as_mut().map(|p| &mut **p as &mut dyn Planner),
            options.redo.as_deref(),
            options.dry_run,
        )?
    } else {
        vec![agent.edit(
            &options.image,
            planner.as_mut().map(|p| &mut **p as &mut dyn Planner),
            options.redo.as_deref(),
            options.dry_run,
        )?]
    };
    Ok(
        json!({"mode":options.provider.as_deref().unwrap_or("style-profile only"),"dry_run":options.dry_run,"review_queue":queue}),
    )
}
