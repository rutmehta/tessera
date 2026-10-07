use engine_api::jobs::CancellationToken;
use export::{AfterExportActions, run_after_export};
use std::{ffi::OsString, path::PathBuf};

#[test]
#[cfg(target_os = "macos")]
fn gui_command_plans_keep_paths_as_literal_arguments() {
    let output = PathBuf::from("/tmp/a ; $(touch nope).jpg");
    let actions = AfterExportActions {
        reveal: true,
        open_in_app: Some("/Applications/My App.app".into()),
        run_script: Some("/tmp/post process.sh".into()),
        ..Default::default()
    };
    let commands = actions.commands(std::slice::from_ref(&output)).unwrap();
    assert_eq!(commands.len(), 3);
    assert_eq!(commands[0].program, PathBuf::from("/usr/bin/open"));
    assert_eq!(
        commands[0].args,
        vec![
            OsString::from("-R"),
            "--".into(),
            output.clone().into_os_string()
        ]
    );
    assert_eq!(commands[1].program, PathBuf::from("/usr/bin/open"));
    assert_eq!(
        commands[1].args,
        vec![
            OsString::from("-a"),
            "/Applications/My App.app".into(),
            "--".into(),
            output.clone().into_os_string()
        ]
    );
    assert_eq!(commands[2].program, PathBuf::from("/tmp/post process.sh"));
    assert_eq!(commands[2].args, vec![output.into_os_string()]);
    assert!(actions.commands(&[]).unwrap().is_empty());
    assert!(actions.commands(&["relative.jpg".into()]).is_err());
}

#[test]
#[cfg(unix)]
#[cfg_attr(
    debug_assertions,
    ignore = "release-only latency bound: skipped in debug builds"
)]
fn script_timeout_cancellation_empty_and_spawn_failure() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("script.sh");
    let marker = dir.path().join("marker");
    let actions = AfterExportActions {
        run_script: Some(script.clone()),
        timeout_seconds: 1,
        ..Default::default()
    };
    std::fs::write(
        &script,
        "#!/bin/sh\nprintf started > \"$1\"\nwhile :; do :; done\n",
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let cancel = CancellationToken::new();
    assert!(run_after_export(&actions, &[], &cancel).is_empty());
    assert!(!marker.exists());
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert!(run_after_export(&actions, std::slice::from_ref(&marker), &cancelled).is_empty());
    assert!(!marker.exists());
    // Cancellation is ordered after the child's ready marker, not a bound on
    // scheduler/process-start latency. The guard detects a hung test only.
    let running = CancellationToken::new();
    let worker_cancel = running.clone();
    let worker_actions = AfterExportActions {
        timeout_seconds: 60,
        ..actions.clone()
    };
    let worker_marker = marker.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        tx.send(run_after_export(
            &worker_actions,
            &[worker_marker],
            &worker_cancel,
        ))
        .unwrap();
    });
    let guard = std::time::Instant::now();
    while !marker.exists() && guard.elapsed() < std::time::Duration::from_secs(30) {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let ready = marker.exists();
    running.cancel();
    let cancellation = rx
        .recv_timeout(std::time::Duration::from_secs(30))
        .expect("cancellation hang guard");
    worker.join().unwrap();
    assert!(ready, "child must announce readiness before cancellation");
    assert_eq!(std::fs::read_to_string(&marker).unwrap(), "started");
    assert_eq!(cancellation.len(), 1);
    assert!(cancellation[0].contains("cancelled"));
    std::fs::remove_file(&marker).unwrap();
    // The timeout remains one second. Its contract is the outcome; a child
    // need not have been scheduled before its timeout expires under load.
    let errors = run_after_export(&actions, std::slice::from_ref(&marker), &cancel);
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("timed out"));
    std::fs::remove_file(&script).unwrap();
    assert_eq!(run_after_export(&actions, &[marker], &cancel).len(), 1);
}

#[test]
fn validates_timeout_and_absolute_executable_paths() {
    for timeout_seconds in [0, 3601] {
        assert!(
            AfterExportActions {
                timeout_seconds,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
    for path in ["relative.sh", "", "/tmp/nul\0script"] {
        assert!(
            AfterExportActions {
                run_script: Some(path.into()),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
}
