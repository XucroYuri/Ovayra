use spike_contracts::{Evidence, PhaseZeroMatrix, SpikeId, TargetId, Verdict};

fn matrix() -> PhaseZeroMatrix {
    PhaseZeroMatrix::load(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../packaging/phase-0-matrix.toml"),
    )
    .unwrap()
}

fn evidence(spike: SpikeId, target: &str) -> Evidence {
    let mut report = Evidence::new(spike, TargetId::new(target).unwrap());
    report.finish(Verdict::Pass, 120_000);
    report
}

#[test]
fn missing_required_evidence_fails_closed() {
    let error = matrix().evaluate(&[]).unwrap_err();
    assert!(error.to_string().contains("missing required evidence"));
}

#[test]
fn conditional_skipped_and_failed_required_evidence_fail_closed() {
    for verdict in [Verdict::Conditional, Verdict::Skipped, Verdict::Fail] {
        let mut report = evidence(SpikeId::Gemini, "macos-arm64-vt");
        report.finish(verdict, 1);
        let error = matrix().evaluate(&[report]).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("required real-device evidence must pass")
        );
    }
}

#[test]
fn wrong_target_backend_and_missing_actual_backend_fail_closed() {
    let mut report = evidence(SpikeId::Media, "macos-arm64-vt");
    report.measure("requested_backend", "videotoolbox").unwrap();
    let error = matrix().evaluate(&[report]).unwrap_err();
    assert!(error.to_string().contains("actual_backend"));

    let mut report = evidence(SpikeId::Media, "macos-arm64-vt");
    report.measure("requested_backend", "videotoolbox").unwrap();
    report.measure("actual_backend", "vaapi").unwrap();
    let error = matrix().evaluate(&[report]).unwrap_err();
    assert!(error.to_string().contains("backend"));

    let report = evidence(SpikeId::Distribution, "linux-x64-nvidia");
    let error = matrix().evaluate(&[report]).unwrap_err();
    assert!(error.to_string().contains("unmatched evidence"));
}

#[test]
fn duplicate_and_unmatched_records_fail_closed() {
    let mut first = evidence(SpikeId::Gemini, "macos-arm64-vt");
    first.measure("observed_server_offset", 1).unwrap();
    let mut second = evidence(SpikeId::Gemini, "macos-arm64-vt");
    second.measure("observed_server_offset", 1).unwrap();
    let error = matrix().evaluate(&[first, second]).unwrap_err();
    assert!(error.to_string().contains("duplicate evidence"));
}

#[test]
fn preview_threshold_violation_fails_closed() {
    let mut report = evidence(SpikeId::Preview, "macos-arm64-vt");
    report.measure("session", "aqua").unwrap();
    report.measure("observed_milli_fps", 22_999).unwrap();
    let error = matrix().evaluate(&[report]).unwrap_err();
    assert!(error.to_string().contains("preview"));
}

#[test]
fn complete_synthetic_thirty_three_row_matrix_passes() {
    let matrix = matrix();
    let reports = matrix
        .required
        .iter()
        .map(synthetic_required)
        .collect::<Vec<_>>();
    matrix.evaluate(&reports).unwrap();
}

fn synthetic_required(required: &spike_contracts::RequiredEvidence) -> Evidence {
    let mut report = evidence(required.id, required.target.as_str());
    if let Some(session) = &required.session {
        report.measure("session", session).unwrap();
    }
    match required.id {
        SpikeId::Preview => {
            for (name, value) in [
                ("observed_milli_fps", 24_000),
                ("requested_duration_seconds", 120),
                ("measured_elapsed_ms", 120_000),
                ("frames_read", 2_880),
                ("frames_applied", 2_800),
                ("p95_ms", 100),
                ("rss_growth_mib", 64),
                ("event_loop_errors", 0),
                ("preview_stream_errors", 0),
            ] {
                report.measure(name, value).unwrap();
            }
            report.measure("automation_hide", true).unwrap();
            report.measure("automation_restore", true).unwrap();
            report.measure("rss_samples_complete", true).unwrap();
        }
        SpikeId::Media => {
            let backend = required.backend.as_deref().unwrap();
            report.measure("requested_backend", backend).unwrap();
            report
                .measure(
                    "actual_backend",
                    if backend == "cpu-fallback" {
                        "cpu"
                    } else {
                        backend
                    },
                )
                .unwrap();
            report
                .measure(
                    "content_sha256",
                    "0000000000000000000000000000000000000000000000000000000000000000",
                )
                .unwrap();
            if backend == "cpu-fallback" {
                report.measure("video_codec", "vp9").unwrap();
                report.measure("audio_codec", "opus").unwrap();
                report.measure("media_duration_seconds", 10).unwrap();
            }
        }
        SpikeId::Gemini => {
            report.measure("observed_server_offset", 1).unwrap();
            report.measure("offset_mismatch", false).unwrap();
            report.measure("analysis_nonempty", true).unwrap();
            report.measure("remote_cleanup_state", "deleted").unwrap();
            report
                .measure("checkpoint_cleanup_state", "deleted")
                .unwrap();
            report.measure("model", "gemini-3.1-flash-lite").unwrap();
            report.measure("http_status", 200).unwrap();
        }
        SpikeId::Platform => {
            report.measure("write_status", "pass").unwrap();
            report.measure("read_status", "pass").unwrap();
            report.measure("cleanup_status", "pass").unwrap();
            report.measure("tray_status", "pass").unwrap();
            report.measure("process_group_status", "pass").unwrap();
            report.measure("child_tree_elapsed_ms", 5_000).unwrap();
            if required.target.as_str().starts_with("linux-") {
                report
                    .measure("forced_no_tray_status", "window-accessible")
                    .unwrap();
                report.measure("no_tray_warning_shown", true).unwrap();
            }
        }
        SpikeId::Distribution => {
            report.measure("bundle_validation", "pass").unwrap();
            report.measure("license_policy", "LGPL-only").unwrap();
            report.measure("source_correspondence", "pass").unwrap();
            report.measure("sbom_status", "pass").unwrap();
            report.measure("ffmpeg_keyring_status", "pass").unwrap();
            report.measure("native_double_build", "pass").unwrap();
            report.measure("platform_signature", "pass").unwrap();
            report.measure("update_tamper_rejected", true).unwrap();
            report
                .measure("package_formats", package_formats(required.target.as_str()))
                .unwrap();
            if required.target.as_str() == "macos-arm64-vt" {
                report.measure("notarization", "pass").unwrap();
            }
        }
    }
    report
}

fn package_formats(target: &str) -> Vec<&'static str> {
    match target {
        "macos-arm64-vt" => vec!["app", "dmg"],
        "windows-x64-mf" => vec!["msi"],
        "linux-x64-vaapi-wayland" => vec!["appimage", "deb"],
        _ => unreachable!("distribution is only required for package targets"),
    }
}
