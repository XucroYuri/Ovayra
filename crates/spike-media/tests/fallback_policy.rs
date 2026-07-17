use spike_media::{AttemptOutcome, Backend, DowngradeCode, ExecutionPolicy, FORCED_FAILURE_DEVICE};
use std::path::Path;

#[test]
fn hardware_failure_restarts_the_stage_on_cpu_and_records_reason() {
    let mut policy = ExecutionPolicy::prefer_isolated(Backend::NvencNvdec);
    let next = policy
        .observe(AttemptOutcome::Failed("device lost".into()))
        .unwrap();
    assert!(next.is_cpu());
    assert_eq!(policy.downgrade_reason(), Some("device lost"));
    assert!(!policy.may_retry_hardware_in_this_session());
}

#[test]
fn every_hardware_failure_category_is_a_bounded_cpu_downgrade() {
    for (outcome, code) in [
        (AttemptOutcome::ProbeFailed, DowngradeCode::ProbeFailed),
        (AttemptOutcome::SpawnFailed, DowngradeCode::SpawnFailed),
        (AttemptOutcome::TimedOut, DowngradeCode::TimedOut),
        (AttemptOutcome::NonZeroExit, DowngradeCode::NonZeroExit),
        (AttemptOutcome::MissingFrames, DowngradeCode::MissingFrames),
        (
            AttemptOutcome::InvalidFfprobe,
            DowngradeCode::InvalidFfprobe,
        ),
    ] {
        let mut policy = ExecutionPolicy::prefer_isolated(Backend::Vaapi);
        assert!(policy.observe(outcome).unwrap().is_cpu());
        assert_eq!(policy.downgrade_code(), Some(code));
        assert!(!policy.may_retry_hardware_in_this_session());
    }
}

#[test]
fn cpu_failure_is_terminal_and_never_schedules_a_third_attempt() {
    let mut policy = ExecutionPolicy::prefer_isolated(Backend::VideoToolbox);
    assert!(policy.observe(AttemptOutcome::TimedOut).unwrap().is_cpu());
    assert!(policy.observe(AttemptOutcome::NonZeroExit).is_err());
    assert!(policy.observe(AttemptOutcome::NonZeroExit).is_err());
    assert_eq!(policy.attempts_started(), 2);
}

#[test]
fn evidence_values_are_stable_backend_names_not_diagnostics() {
    let mut policy = ExecutionPolicy::prefer_isolated(Backend::D3d11vaMf);
    assert!(
        policy
            .observe(AttemptOutcome::Failed(
                "device=/private/render stderr=bad".into()
            ))
            .unwrap()
            .is_cpu()
    );
    assert_eq!(policy.requested_backend(), Backend::D3d11vaMf);
    assert_eq!(policy.actual_backend(), None);
    assert_eq!(policy.downgrade_code(), Some(DowngradeCode::Failed));
    assert_eq!(
        policy.downgrade_reason(),
        Some("device=/private/render stderr=bad")
    );
    assert_eq!(Backend::D3d11vaMf.as_str(), "d3d11va-mf");
    assert_eq!(Backend::Cpu.as_str(), "cpu");
}

#[test]
fn production_policy_quarantines_a_backend_across_new_policies() {
    let mut first = ExecutionPolicy::prefer(Backend::NvencNvdec);
    assert!(first.observe(AttemptOutcome::ProbeFailed).unwrap().is_cpu());
    let later = ExecutionPolicy::prefer(Backend::NvencNvdec);
    assert_eq!(
        later.downgrade_code(),
        Some(DowngradeCode::HardwareQuarantined)
    );
    assert_eq!(later.actual_backend(), None);
    assert!(!later.may_retry_hardware_in_this_session());
    assert!(later.next_backend().unwrap().is_cpu());
}

#[test]
fn forced_cpu_fallback_transcodes_the_supplied_synthetic_input() {
    let arguments = spike_media::CpuFallback::new("ffmpeg", "ffprobe").ffmpeg_input_arguments(
        Path::new("hardware-input.mp4"),
        Path::new("fallback.webm"),
        10,
    );
    let arguments = arguments
        .iter()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert!(
        arguments
            .windows(2)
            .any(|pair| pair == ["-i", "hardware-input.mp4"])
    );
    assert!(!arguments.iter().any(|argument| argument == "lavfi"));
    assert!(
        arguments
            .windows(2)
            .any(|pair| pair == ["-c:v", "libvpx-vp9"])
    );
    assert!(arguments.windows(2).any(|pair| pair == ["-c:a", "libopus"]));
}

#[test]
fn generic_ffprobe_validation_rejects_no_video_or_invalid_duration() {
    assert!(
        spike_media::FfprobeReport::validate_any_json(
            r#"{"format":{"duration":"2"},"streams":[{"codec_type":"video"}]}"#,
            1,
        )
        .is_ok()
    );
    assert!(
        spike_media::FfprobeReport::validate_any_json(
            r#"{"format":{"duration":"0"},"streams":[{"codec_type":"video"}]}"#,
            1,
        )
        .is_err()
    );
    assert!(
        spike_media::FfprobeReport::validate_any_json(
            r#"{"format":{"duration":"2"},"streams":[]}"#,
            1,
        )
        .is_err()
    );
}

#[test]
fn forced_hardware_plan_passes_the_invalid_device_to_every_backend() {
    for backend in Backend::ALL {
        let args = spike_media::HardwarePlan::self_test(backend).transcode_args(
            Path::new("hardware-input.mp4"),
            Path::new("output.mp4"),
            Some(Path::new(FORCED_FAILURE_DEVICE)),
        );
        let args = args
            .iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(
            args.iter()
                .any(|argument| argument == FORCED_FAILURE_DEVICE)
        );
        assert!(
            args.iter()
                .any(|argument| argument == "-ovayra_forced_hardware_failure")
        );
    }
}
