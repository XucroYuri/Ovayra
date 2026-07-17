use std::{fs, path::Path, process::Command};

use spike_contracts::{
    ArtifactDigestProof, DistributionFfmpegProof, DistributionPackageProof,
    DistributionUpdateProof, GeminiResumeProof, GeminiStageProof, MediaCpuProof,
    MediaForcedFallbackProof, MediaHardwareProof, PhaseZeroMatrix, PhaseZeroProof,
    PlatformCheckpointProof, PlatformKeyringProof, PlatformNoTrayProof, PlatformProcessProof,
    PlatformTrayProof, PreviewProof, ProofComponent, ProofPayload, ProofRow, SpikeId,
};

fn matrix_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/phase-0-matrix.toml")
}

fn run_gate(evidence_dir: &std::path::Path, report: &std::path::Path) -> std::process::Output {
    run_gate_with_matrix(evidence_dir, &matrix_path(), report)
}

fn run_gate_with_matrix(evidence_dir: &Path, matrix: &Path, report: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ovayra-spike"))
        .args([
            "gate",
            "--evidence-dir",
            evidence_dir.to_str().unwrap(),
            "--matrix",
            matrix.to_str().unwrap(),
            "--report",
            report.to_str().unwrap(),
        ])
        .output()
        .unwrap()
}

fn digest(byte: char) -> String {
    std::iter::repeat_n(byte, 64).collect()
}

fn proof_row(required: &spike_contracts::RequiredEvidence) -> ProofRow {
    ProofRow {
        spike: required.id,
        target: required.target.clone(),
        session: required.session.clone(),
        backend: required.backend.clone(),
    }
}

fn artifacts(target: &str) -> Vec<ArtifactDigestProof> {
    let formats: &[&str] = match target {
        "macos-arm64-vt" => &["app", "dmg"],
        "windows-x64-mf" => &["wix"],
        "linux-x64-vaapi-wayland" => &["appimage", "deb"],
        _ => unreachable!("only release targets have distribution rows"),
    };
    formats
        .iter()
        .enumerate()
        .map(|(index, format)| ArtifactDigestProof {
            format: (*format).to_owned(),
            sha256: digest(char::from(b'a' + u8::try_from(index).unwrap())),
            length: u64::try_from(index + 1).unwrap(),
        })
        .collect()
}

#[allow(clippy::too_many_lines)] // Canonical matrix fixtures are intentionally colocated.
fn production_proofs() -> Vec<PhaseZeroProof> {
    let matrix = PhaseZeroMatrix::load(matrix_path()).unwrap();
    let mut proofs = Vec::new();
    for required in &matrix.required {
        let row = proof_row(required);
        match required.id {
            SpikeId::Preview => proofs.push(PhaseZeroProof::record(
                ProofComponent::Preview,
                row,
                ProofPayload::Preview(PreviewProof {
                    requested_duration_ms: 120_000,
                    measured_duration_ms: 120_000,
                    milli_fps: 24_000,
                    p95_ms: 100,
                    rss_growth_mib: 64,
                    frames_read: 2_880,
                    frames_applied: 2_880,
                    frames_dropped: 0,
                    hidden: true,
                    restored: true,
                    event_loop_errors: 0,
                    stream_errors: 0,
                    renderer: "production-test".to_owned(),
                }),
            )),
            SpikeId::Media if required.backend.as_deref() == Some("cpu-fallback") => {
                proofs.push(PhaseZeroProof::record(
                    ProofComponent::MediaCpu,
                    row,
                    ProofPayload::MediaCpu(MediaCpuProof {
                        actual_backend: "cpu".to_owned(),
                        output_duration_seconds: 10,
                        video_codec: "vp9".to_owned(),
                        audio_codec: "opus".to_owned(),
                        progress_complete: true,
                        output_sha256: digest('c'),
                    }),
                ));
            }
            SpikeId::Media => {
                let backend = required.backend.clone().unwrap();
                proofs.push(PhaseZeroProof::record(
                    ProofComponent::MediaHardware,
                    row.clone(),
                    ProofPayload::MediaHardware(MediaHardwareProof {
                        requested_backend: backend.clone(),
                        actual_backend: backend.clone(),
                        output_duration_seconds: 10,
                        output_sha256: digest('d'),
                    }),
                ));
                proofs.push(PhaseZeroProof::record(
                    ProofComponent::MediaForcedFallback,
                    row,
                    ProofPayload::MediaForcedFallback(MediaForcedFallbackProof {
                        requested_backend: backend,
                        cpu_restarts: 1,
                        session_quarantined: true,
                        video_codec: "vp9".to_owned(),
                        audio_codec: "opus".to_owned(),
                        output_sha256: digest('e'),
                    }),
                ));
            }
            SpikeId::Gemini => {
                let checkpoint_id = format!("checkpoint-{}", required.target.as_str());
                proofs.push(PhaseZeroProof::record(
                    ProofComponent::GeminiStage,
                    row.clone(),
                    ProofPayload::GeminiStage(GeminiStageProof {
                        checkpoint_id: checkpoint_id.clone(),
                        staged_offset: 8,
                        server_offset: 8,
                        retry_policy_observed: true,
                        chunk_granularity: 8,
                        encrypted: true,
                        plaintext_absent: true,
                    }),
                ));
                proofs.push(PhaseZeroProof::record(
                    ProofComponent::GeminiResume,
                    row,
                    ProofPayload::GeminiResume(GeminiResumeProof {
                        checkpoint_id,
                        resumed_offset: 8,
                        server_offset: 8,
                        server_authoritative: true,
                        remote_state: "ACTIVE".to_owned(),
                        analysis_nonempty: true,
                        model: "gemini-3.1-flash-lite".to_owned(),
                        http_status: 200,
                        remote_deleted: true,
                        checkpoint_deleted: true,
                        retry_policy_observed: true,
                    }),
                ));
            }
            SpikeId::Platform => {
                proofs.extend([
                    PhaseZeroProof::record(
                        ProofComponent::PlatformKeyring,
                        row.clone(),
                        ProofPayload::PlatformKeyring(PlatformKeyringProof {
                            set_ok: true,
                            get_ok: true,
                            delete_ok: true,
                            missing_after_delete: true,
                        }),
                    ),
                    PhaseZeroProof::record(
                        ProofComponent::PlatformTray,
                        row.clone(),
                        ProofPayload::PlatformTray(PlatformTrayProof {
                            hidden: true,
                            restored: true,
                            quit: true,
                        }),
                    ),
                    PhaseZeroProof::record(
                        ProofComponent::PlatformNoTray,
                        row.clone(),
                        ProofPayload::PlatformNoTray(PlatformNoTrayProof {
                            accessible: true,
                            warning_shown: true,
                            quit: true,
                        }),
                    ),
                    PhaseZeroProof::record(
                        ProofComponent::PlatformProcess,
                        row.clone(),
                        ProofPayload::PlatformProcess(PlatformProcessProof {
                            parent_dead: true,
                            grandchild_dead: true,
                            elapsed_ms: 5_000,
                        }),
                    ),
                    PhaseZeroProof::record(
                        ProofComponent::PlatformCheckpoint,
                        row,
                        ProofPayload::PlatformCheckpoint(PlatformCheckpointProof {
                            encrypted: true,
                            plaintext_absent: true,
                        }),
                    ),
                ]);
            }
            SpikeId::Distribution => {
                let source_lock = digest('f');
                let artifact_set = artifacts(required.target.as_str());
                let (platform_verification, notarization, updater_format) =
                    match required.target.as_str() {
                        "macos-arm64-vt" => ("codesign_notary_staple", Some("accepted"), "app"),
                        "windows-x64-mf" => ("authenticode", None, "wix"),
                        "linux-x64-vaapi-wayland" => ("minisign", None, "appimage"),
                        _ => unreachable!(),
                    };
                proofs.extend([
                    PhaseZeroProof::record(
                        ProofComponent::DistributionFfmpeg,
                        row.clone(),
                        ProofPayload::DistributionFfmpeg(DistributionFfmpegProof {
                            immutable_lock: true,
                            source_signature: true,
                            sbom: true,
                            reproducible: true,
                            lgpl_only: true,
                            source_correspondence: true,
                            source_lock_sha256: source_lock.clone(),
                            bundle_tree_sha256: digest('1'),
                        }),
                    ),
                    PhaseZeroProof::record(
                        ProofComponent::DistributionPackage,
                        row.clone(),
                        ProofPayload::DistributionPackage(DistributionPackageProof {
                            artifacts: artifact_set.clone(),
                            source_lock_sha256: source_lock,
                            inspection_sha256: digest('2'),
                            platform_verification: platform_verification.to_owned(),
                            notarization: notarization.map(str::to_owned),
                        }),
                    ),
                    PhaseZeroProof::record(
                        ProofComponent::DistributionUpdate,
                        row,
                        ProofPayload::DistributionUpdate(DistributionUpdateProof {
                            manifest_sha256: digest('3'),
                            artifacts: artifact_set,
                            updater_format: updater_format.to_owned(),
                            signature_verification: "pinned_minisign".to_owned(),
                            tamper_rejection: "updater_and_download".to_owned(),
                        }),
                    ),
                ]);
            }
        }
    }
    proofs
}

fn write_proofs(directory: &Path, proofs: &[PhaseZeroProof]) {
    fs::create_dir_all(directory).unwrap();
    for (index, proof) in proofs.iter().enumerate() {
        fs::write(
            directory.join(format!("proof-{index:03}.json")),
            proof.to_pretty_json().unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn complete_file_backed_production_proofs_pass_with_deterministic_rows_components_and_hashes() {
    let directory = tempfile::tempdir().unwrap();
    let evidence_dir = directory.path().join("evidence");
    let report = directory.path().join("feasibility-report.md");
    let proofs = production_proofs();
    write_proofs(&evidence_dir, &proofs);

    assert!(run_gate(&evidence_dir, &report).status.success());
    let first = fs::read_to_string(&report).unwrap();
    assert!(first.contains("Status: PASS"));
    assert!(first.contains("Required components"));
    assert_eq!(first.matches("| Distribution | ").count(), 3);
    assert_eq!(first.matches("Source JSON SHA-256").count(), 1);
    assert!(run_gate(&evidence_dir, &report).status.success());
    assert_eq!(first, fs::read_to_string(report).unwrap());
}

#[test]
#[allow(clippy::too_many_lines)] // File-backed rejection matrix keeps each adversarial mutation visible.
fn file_backed_gate_rejects_missing_duplicate_unmatched_and_non_v2_proofs() {
    type Mutation = Box<dyn Fn(&Path, &mut Vec<PhaseZeroProof>)>;
    let cases: Vec<(&str, Mutation)> = vec![
        (
            "missing",
            Box::new(|dir, proofs| {
                proofs.pop();
                write_proofs(dir, proofs);
            }),
        ),
        (
            "duplicate",
            Box::new(|dir, proofs| {
                write_proofs(dir, proofs);
                fs::write(
                    dir.join("duplicate.json"),
                    proofs[0].to_pretty_json().unwrap(),
                )
                .unwrap();
            }),
        ),
        (
            "unmatched",
            Box::new(|dir, proofs| {
                proofs[0].row.target = spike_contracts::TargetId::new("windows-x64-mf").unwrap();
                write_proofs(dir, proofs);
            }),
        ),
        (
            "preview-threshold",
            Box::new(|dir, proofs| {
                let Some(PhaseZeroProof {
                    proof: ProofPayload::Preview(value),
                    ..
                }) = proofs
                    .iter_mut()
                    .find(|proof| proof.component == ProofComponent::Preview)
                else {
                    unreachable!();
                };
                value.p95_ms = 101;
                write_proofs(dir, proofs);
            }),
        ),
        (
            "media-contract",
            Box::new(|dir, proofs| {
                let Some(PhaseZeroProof {
                    proof: ProofPayload::MediaForcedFallback(value),
                    ..
                }) = proofs
                    .iter_mut()
                    .find(|proof| proof.component == ProofComponent::MediaForcedFallback)
                else {
                    unreachable!();
                };
                value.cpu_restarts = 2;
                write_proofs(dir, proofs);
            }),
        ),
        (
            "gemini-binding",
            Box::new(|dir, proofs| {
                let Some(PhaseZeroProof {
                    proof: ProofPayload::GeminiResume(value),
                    ..
                }) = proofs
                    .iter_mut()
                    .find(|proof| proof.component == ProofComponent::GeminiResume)
                else {
                    unreachable!();
                };
                value.checkpoint_id = "unrelated-checkpoint".to_owned();
                write_proofs(dir, proofs);
            }),
        ),
        (
            "gemini-offset",
            Box::new(|dir, proofs| {
                let Some(PhaseZeroProof {
                    proof: ProofPayload::GeminiResume(value),
                    ..
                }) = proofs
                    .iter_mut()
                    .find(|proof| proof.component == ProofComponent::GeminiResume)
                else {
                    unreachable!();
                };
                value.resumed_offset = 1;
                write_proofs(dir, proofs);
            }),
        ),
        (
            "platform-contract",
            Box::new(|dir, proofs| {
                let Some(PhaseZeroProof {
                    proof: ProofPayload::PlatformCheckpoint(value),
                    ..
                }) = proofs
                    .iter_mut()
                    .find(|proof| proof.component == ProofComponent::PlatformCheckpoint)
                else {
                    unreachable!();
                };
                value.encrypted = false;
                write_proofs(dir, proofs);
            }),
        ),
        (
            "distribution-relationship",
            Box::new(|dir, proofs| {
                let Some(PhaseZeroProof {
                    proof: ProofPayload::DistributionUpdate(value),
                    ..
                }) = proofs
                    .iter_mut()
                    .find(|proof| proof.component == ProofComponent::DistributionUpdate)
                else {
                    unreachable!();
                };
                value.artifacts[0].sha256 = digest('9');
                write_proofs(dir, proofs);
            }),
        ),
        (
            "v1",
            Box::new(|dir, proofs| {
                write_proofs(dir, proofs);
                fs::write(
                    dir.join("stale.json"),
                    r#"{"schema_version":1,"spike":"preview"}"#,
                )
                .unwrap();
            }),
        ),
        (
            "unknown",
            Box::new(|dir, proofs| {
                write_proofs(dir, proofs);
                fs::write(dir.join("unknown.json"), r#"{"schema_version":2,"component":"preview","row":{},"proof":{},"extra":true}"#).unwrap();
            }),
        ),
    ];
    for (name, mutate) in cases {
        let directory = tempfile::tempdir().unwrap();
        let evidence_dir = directory.path().join("evidence");
        let report = directory.path().join("report.md");
        let mut proofs = production_proofs();
        mutate(&evidence_dir, &mut proofs);
        assert!(!run_gate(&evidence_dir, &report).status.success(), "{name}");
    }
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
