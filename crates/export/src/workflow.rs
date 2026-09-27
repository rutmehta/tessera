//! Opt-in host post-processing. Rendering/encoding never invokes these helpers.
use engine_api::{EngineError, EngineResult, jobs::CancellationToken};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// Executed by a host only after a successful, uncancelled export. Scripts
/// must be executable files with a shebang; output paths are separate argv
/// entries, never shell text. Ordering: reveal, open in app, run script.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AfterExportActions {
    pub reveal: bool,
    pub open_in_app: Option<PathBuf>,
    pub run_script: Option<PathBuf>,
    /// Per child process, 1–3600 seconds. Does not wait for a launched GUI app.
    pub timeout_seconds: u32,
}

impl Default for AfterExportActions {
    fn default() -> Self {
        Self {
            reveal: false,
            open_in_app: None,
            run_script: None,
            timeout_seconds: 60,
        }
    }
}

/// A directly executable host command, inspectable without opening a GUI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AfterExportCommand {
    pub program: PathBuf,
    pub args: Vec<OsString>,
}

impl AfterExportActions {
    pub fn validate(&self) -> EngineResult<()> {
        if !(1..=3600).contains(&self.timeout_seconds) {
            return Err(EngineError::invalid(
                "after_export",
                "timeout must be 1–3600 seconds",
            ));
        }
        for path in [&self.open_in_app, &self.run_script].into_iter().flatten() {
            if !path.is_absolute() || path.as_os_str().as_encoded_bytes().contains(&0) {
                return Err(EngineError::invalid(
                    "after_export",
                    "app and script paths must be absolute, without NUL",
                ));
            }
        }
        if !cfg!(target_os = "macos") && (self.reveal || self.open_in_app.is_some()) {
            return Err(EngineError::invalid(
                "after_export",
                "reveal and open-in-app require macOS",
            ));
        }
        Ok(())
    }

    pub fn commands(&self, outputs: &[PathBuf]) -> EngineResult<Vec<AfterExportCommand>> {
        self.validate()?;
        if outputs.is_empty() {
            return Ok(Vec::new());
        }
        if outputs.iter().any(|p| !p.is_absolute()) {
            return Err(EngineError::invalid(
                "after_export",
                "output paths must be absolute",
            ));
        }
        let paths = || outputs.iter().map(|p| p.as_os_str().to_owned());
        let mut commands = Vec::new();
        if self.reveal {
            commands.push(AfterExportCommand {
                program: "/usr/bin/open".into(),
                args: [OsString::from("-R"), OsString::from("--")]
                    .into_iter()
                    .chain(paths())
                    .collect(),
            });
        }
        if let Some(app) = &self.open_in_app {
            commands.push(AfterExportCommand {
                program: "/usr/bin/open".into(),
                args: [
                    OsString::from("-a"),
                    app.as_os_str().to_owned(),
                    OsString::from("--"),
                ]
                .into_iter()
                .chain(paths())
                .collect(),
            });
        }
        if let Some(script) = &self.run_script {
            commands.push(AfterExportCommand {
                program: script.clone(),
                args: paths().collect(),
            });
        }
        Ok(commands)
    }
}

/// Explicit host call, never invoked by `export_one` / batch rendering. No
/// shell interpolation or PATH search; child stdio cannot corrupt host JSON.
/// A failing action is reported separately from successful image publication.
/// Timeout/cancellation kills and reaps the direct child (not its descendants).
pub fn run_after_export(
    actions: &AfterExportActions,
    outputs: &[PathBuf],
    cancel: &CancellationToken,
) -> Vec<String> {
    let commands = match actions.commands(outputs) {
        Ok(commands) => commands,
        Err(error) => return vec![error.to_string()],
    };
    let mut errors = Vec::new();
    for command in commands {
        if cancel.is_cancelled() {
            break;
        }
        if let Err(error) = run_command(
            &command,
            Duration::from_secs(u64::from(actions.timeout_seconds)),
            cancel,
        ) {
            errors.push(format!("{}: {error}", command.program.display()));
        }
    }
    errors
}

fn run_command(
    command: &AfterExportCommand,
    timeout: Duration,
    cancel: &CancellationToken,
) -> Result<(), String> {
    let mut child = Command::new(&command.program)
        .args(&command.args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.success() {
                    Ok(())
                } else {
                    Err(format!("exited with {status}"))
                };
            }
            Ok(None) => {}
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.to_string());
            }
        }
        if cancel.is_cancelled() || started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(if cancel.is_cancelled() {
                "cancelled"
            } else {
                "timed out"
            }
            .into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
