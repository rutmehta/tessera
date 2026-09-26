mod cpu;

use std::process::Command;

fn main() -> anyhow::Result<()> {
    if std::env::args().nth(1).as_deref() == Some("--measure-cpu") {
        println!("{}", serde_json::to_string(&cpu::measure()?)?);
        return Ok(());
    }
    let status = Command::new("python3")
        .arg("-B")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/runner.py"))
        .arg("--worker")
        .arg(std::env::current_exe()?)
        .args(std::env::args_os().skip(1))
        .status()?;
    std::process::exit(status.code().unwrap_or(1));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runner_contracts() {
        assert!(
            Command::new("python3")
                .args(["-B", concat!(env!("CARGO_MANIFEST_DIR"), "/test_runner.py")])
                .status()
                .unwrap()
                .success()
        );
    }

    #[test]
    #[ignore = "measures the full CPU suite; run in isolation in release mode"]
    fn cpu_smoke() {
        let rows = cpu::measure().unwrap();
        let benches: std::collections::BTreeSet<_> =
            rows.iter().map(|r| r["bench"].as_str().unwrap()).collect();
        assert_eq!(benches.len(), 6);
        for row in rows {
            assert!(row["value"].as_f64().unwrap() > 0.0);
            assert_eq!(row["backend"], "cpu");
        }
    }
}
