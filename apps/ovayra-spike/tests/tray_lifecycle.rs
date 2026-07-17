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
    assert_eq!(evidence["component"], "platform_no_tray");
    let proof = &evidence["proof"];
    assert_eq!(proof["kind"], "platform_no_tray");
    assert_eq!(proof["accessible"], true);
    assert_eq!(proof["warning_shown"], true);
    assert_eq!(proof["quit"], true);
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
    assert_eq!(evidence["component"], "platform_tray");
    let proof = &evidence["proof"];
    assert_eq!(proof["kind"], "platform_tray");
    assert_eq!(proof["hidden"], true);
    assert_eq!(proof["restored"], true);
    assert_eq!(proof["quit"], true);
}
