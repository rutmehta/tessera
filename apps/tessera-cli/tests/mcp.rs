use assert_cmd::Command;

#[test]
fn mcp_subcommand_is_advertised() {
    Command::new(assert_cmd::cargo::cargo_bin!("tessera"))
        .args(["mcp", "--help"])
        .assert()
        .success();
}

#[cfg(unix)]
#[test]
fn mcp_exec_forwards_app_directory_and_exit_status_without_stdout_wrapper() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let fake = dir.path().join("tessera-mcp");
    std::fs::write(&fake, "#!/bin/sh\nprintf '%s\\n' \"$@\"\nexit 23\n").unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    Command::new(assert_cmd::cargo::cargo_bin!("tessera"))
        .env("TESSERA_MCP_BIN", &fake)
        .args(["mcp", "--app-dir"])
        .arg(dir.path().join("app"))
        .assert()
        .code(23)
        .stdout(format!("--app-dir\n{}\n", dir.path().join("app").display()));
}
