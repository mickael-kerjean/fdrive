#![cfg(windows)]

use std::path::Path;
use std::process::Command;

#[test]
#[ignore = "requires a running client, saved session, live server, and Explorer"]
fn windows_e2e_script_passes() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/windows-e2e.ps1");
    assert!(script.is_file(), "missing {}", script.display());

    let status = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&script)
        .arg("-KeepGoing")
        .status()
        .unwrap_or_else(|err| panic!("launch {}: {err}", script.display()));

    assert!(
        status.success(),
        "{} exited with {status}",
        script.display()
    );
}
