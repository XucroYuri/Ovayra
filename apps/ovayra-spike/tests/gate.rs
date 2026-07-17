use std::{fs, process::Command};

fn matrix_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/phase-0-matrix.toml")
}

fn run_gate(evidence_dir: &std::path::Path, report: &std::path::Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ovayra-spike"))
        .args([
            "gate",
            "--evidence-dir",
            evidence_dir.to_str().unwrap(),
            "--matrix",
            matrix_path().to_str().unwrap(),
            "--report",
            report.to_str().unwrap(),
        ])
        .output()
        .unwrap()
}

#[test]
fn empty_evidence_is_a_deterministic_no_go_report() {
    let directory = tempfile::tempdir().unwrap();
    let evidence_dir = directory.path().join("evidence");
    let report = directory.path().join("feasibility-report.md");
    fs::create_dir(&evidence_dir).unwrap();

    let output = run_gate(&evidence_dir, &report);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(!output.status.success());
    assert!(stdout.contains("PHASE_0_GATE=NO_GO"));
    assert!(!stdout.contains("PHASE_0_GATE=PASS"));
    let rendered = fs::read_to_string(report).unwrap();
    assert!(rendered.contains("# Phase 0 Feasibility NO-GO"));
    assert!(rendered.contains("missing required evidence"));
}

#[test]
fn evidence_gitkeep_is_ignored_like_the_linter_ignores_it() {
    let directory = tempfile::tempdir().unwrap();
    let evidence_dir = directory.path().join("evidence");
    let report = directory.path().join("feasibility-report.md");
    fs::create_dir(&evidence_dir).unwrap();
    fs::write(evidence_dir.join(".gitkeep"), "").unwrap();

    let output = run_gate(&evidence_dir, &report);
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("PHASE_0_GATE=NO_GO")
    );
}

#[test]
fn gate_does_not_echo_sensitive_evidence_or_untrusted_filenames() {
    let directory = tempfile::tempdir().unwrap();
    let evidence_dir = directory.path().join("evidence");
    let report = directory.path().join("feasibility-report.md");
    fs::create_dir(&evidence_dir).unwrap();
    fs::write(
        evidence_dir.join("api_token-very-secret.json"),
        r#"{"schema_version":1,"spike":"gemini","target":"macos-arm64-vt","verdict":"pass","duration_ms":1,"measurements":{"api_token":"very-secret"},"observations":[]}"#,
    )
    .unwrap();

    let output = run_gate(&evidence_dir, &report);
    let combined = format!(
        "{}{}{}",
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
        fs::read_to_string(report).unwrap(),
    );
    assert!(!output.status.success());
    assert!(combined.contains("PHASE_0_GATE=FAIL"));
    assert!(!combined.contains("very-secret"));
    assert!(!combined.contains("api_token"));
}

#[test]
fn no_go_report_replaces_the_previous_report_atomically() {
    let directory = tempfile::tempdir().unwrap();
    let evidence_dir = directory.path().join("evidence");
    let report = directory.path().join("feasibility-report.md");
    fs::create_dir(&evidence_dir).unwrap();
    fs::write(&report, "old report").unwrap();

    let output = run_gate(&evidence_dir, &report);
    assert!(!output.status.success());
    assert!(
        fs::read_to_string(&report)
            .unwrap()
            .contains("# Phase 0 Feasibility NO-GO")
    );
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
}

#[test]
fn no_go_inventory_hashes_are_deterministic_and_never_include_source_names() {
    let directory = tempfile::tempdir().unwrap();
    let evidence_dir = directory.path().join("evidence");
    let report = directory.path().join("feasibility-report.md");
    fs::create_dir(&evidence_dir).unwrap();
    fs::write(
        evidence_dir.join("untrusted-but-redacted-name.json"),
        r#"{"schema_version":1,"spike":"gemini","target":"macos-arm64-vt","verdict":"pass","duration_ms":1,"measurements":{},"observations":[]}"#,
    )
    .unwrap();

    assert!(!run_gate(&evidence_dir, &report).status.success());
    let first = fs::read_to_string(&report).unwrap();
    assert!(!run_gate(&evidence_dir, &report).status.success());
    let second = fs::read_to_string(&report).unwrap();
    assert_eq!(first, second);
    assert!(first.contains("Source JSON SHA-256"));
    assert!(!first.contains("untrusted-but-redacted-name"));
}

#[cfg(unix)]
#[test]
fn symlinked_evidence_fails_before_gate_evaluation() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().unwrap();
    let evidence_dir = directory.path().join("evidence");
    let report = directory.path().join("feasibility-report.md");
    fs::create_dir(&evidence_dir).unwrap();
    symlink(matrix_path(), evidence_dir.join("record.json")).unwrap();

    let output = run_gate(&evidence_dir, &report);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(!output.status.success());
    assert!(stderr.contains("PHASE_0_GATE=FAIL"));
    assert!(
        fs::read_to_string(report)
            .unwrap()
            .contains("evidence lint rejected")
    );
}
