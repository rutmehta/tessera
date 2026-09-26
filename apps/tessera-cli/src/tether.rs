use anyhow::{Result, bail};
use clap::{Args, Subcommand};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tether::{Session, TetherBackend, native::NativeBackend};

#[derive(Subcommand)]
pub enum Command {
    List,
    /// Receive physical-shutter captures until Ctrl-C; emits JSON lines.
    Start(Options),
    /// Open a session, trigger one capture, wait for ingest, then close.
    Capture(Options),
}
#[derive(Args)]
pub struct Options {
    pub session_folder: PathBuf,
    #[arg(long, default_value = "{sequence}_{original}.{ext}")]
    pub naming: String,
    /// Capture timeout (seconds). Start remains active until Ctrl-C.
    #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u64).range(1..))]
    pub timeout: u64,
}
pub fn run(app: &Path, command: &Command) -> Result<Value> {
    let mut backend = NativeBackend::new()?;
    if matches!(command, Command::List) {
        return Ok(serde_json::to_value(backend.devices()?)?);
    }
    let (Command::Start(options) | Command::Capture(options)) = command else {
        unreachable!()
    };
    std::fs::create_dir_all(app)?;
    let mut session = Session::start(
        backend,
        &options.session_folder,
        &options.naming,
        &app.join("index.sqlite"),
        Some(app.into()),
    )?;
    let running = Arc::new(AtomicBool::new(true));
    let signal = running.clone();
    ctrlc::set_handler(move || signal.store(false, Ordering::Relaxed))?;
    let capture = matches!(command, Command::Capture(_));
    if capture {
        session.capture()?;
    }
    let start = Instant::now();
    let mut received = 0;
    while running.load(Ordering::Relaxed) {
        session.poll()?;
        let frames: Vec<_> = session.events().try_iter().collect();
        for frame in frames {
            received += 1;
            if capture {
                session.stop()?;
                if let Some(error) = &frame.error {
                    bail!("ingest failed: {error}");
                }
                return Ok(serde_json::to_value(frame)?);
            }
            println!("{}", serde_json::to_string(&frame)?);
        }
        if capture && start.elapsed() >= Duration::from_secs(options.timeout) {
            bail!("capture timed out before a completed frame arrived");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    session.stop()?;
    for frame in session.events().try_iter() {
        received += 1;
        println!("{}", serde_json::to_string(&frame)?);
    }
    Ok(json!({"stopped":true,"frames":received}))
}
