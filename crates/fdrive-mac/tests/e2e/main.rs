#![cfg(target_os = "macos")]

use std::path::Path;
use std::process::Command;

#[test]
#[ignore = "requires a running and connected Filestash app with a live server"]
fn mac_e2e_script_passes() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/mac-e2e.sh");
    assert!(script.is_file(), "missing {}", script.display());

    let status = Command::new("/bin/bash")
        .arg(&script)
        .arg("--keep-going")
        .status()
        .unwrap_or_else(|err| panic!("launch {}: {err}", script.display()));

    assert!(status.success(), "{} exited with {status}", script.display());
}
