#![cfg(target_os = "macos")]

use std::process::Command;

#[test]
fn forced_fallback_observes_a_native_close_request_and_keeps_the_window_visible() {
    let evidence = tempfile::NamedTempFile::new().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ovayra-spike"))
        .env("OVAYRA_TARGET_ID", "macos-arm64-vt")
        .args([
            "platform",
            "tray",
            "--automation",
            "--force-no-tray",
            "--evidence",
            evidence.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let evidence: serde_json::Value =
        serde_json::from_slice(&std::fs::read(evidence.path()).unwrap()).unwrap();
    let measured = &evidence["measurements"];
    assert_eq!(measured["automation_native_close_event"], true);
    assert_eq!(measured["window_accessible"], true);
    assert_eq!(measured["warning_visible"], true);
    assert_eq!(measured["tray_status"], "forced_no_tray");
}

#[test]
fn tray_mode_observes_native_hide_restore_and_quit() {
    let evidence = tempfile::NamedTempFile::new().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ovayra-spike"))
        .env("OVAYRA_TARGET_ID", "macos-arm64-vt")
        .args([
            "platform",
            "tray",
            "--automation",
            "--evidence",
            evidence.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let evidence: serde_json::Value =
        serde_json::from_slice(&std::fs::read(evidence.path()).unwrap()).unwrap();
    let measured = &evidence["measurements"];
    assert_eq!(measured["automation_native_close_event"], true);
    assert_eq!(measured["automation_hide"], true);
    assert_eq!(measured["automation_restore"], true);
    assert_eq!(measured["automation_quit_callback"], true);
}
