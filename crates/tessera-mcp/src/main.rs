use rmcp::ServiceExt;
use std::path::PathBuf;
#[tokio::main]
async fn main() -> std::process::ExitCode {
    match run().await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("tessera-mcp: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let mut app = None;
    while let Some(arg) = args.next() {
        if arg == "--help" || arg == "-h" {
            eprintln!("Usage: tessera-mcp --app-dir <directory>\nMCP JSON-RPC over stdin/stdout.");
            return Ok(());
        }
        if arg != "--app-dir" || app.is_some() {
            return Err("expected --app-dir <directory>".into());
        }
        app = Some(PathBuf::from(args.next().ok_or("missing --app-dir value")?));
    }
    let console = tessera_mcp::Console::open(app.ok_or("--app-dir is required")?)?;
    tessera_mcp::Server::new(console)
        .serve(rmcp::transport::stdio())
        .await?
        .waiting()
        .await?;
    Ok(())
}
