use spike_contracts::{
    Evidence, EvidenceError, PhaseZeroMatrix, RequiredEvidence, SpikeId, TargetId, Verdict,
};

#[test]
fn rejects_sensitive_measurement_names() {
    let mut evidence = Evidence::new(SpikeId::Gemini, TargetId::new("macos-arm64-vt"));
    let error = evidence
        .measure("upload_url", "https://secret.invalid")
        .unwrap_err();
    assert!(matches!(error, EvidenceError::SensitiveField(_)));
}

#[test]
fn serializes_only_finished_evidence() {
    let evidence = Evidence::new(SpikeId::Preview, TargetId::new("macos-arm64-vt"));
    assert!(matches!(
        evidence.to_pretty_json(),
        Err(EvidenceError::Unfinished)
    ));
}

#[test]
fn finished_report_has_stable_schema() {
    let mut evidence = Evidence::new(SpikeId::Media, TargetId::new("linux-x64-vaapi-wayland"));
    evidence.measure("p95_latency_ms", 18).unwrap();
    evidence.finish(Verdict::Pass, 1_250);
    let json = evidence.to_pretty_json().unwrap();
    assert!(json.contains("\"schema_version\": 1"));
    assert!(json.contains("\"verdict\": \"pass\""));
}

#[test]
fn parses_required_evidence_with_optional_qualifiers() {
    let matrix = PhaseZeroMatrix::from_toml(
        r#"
            [[required]]
            id = "preview"
            target = "macos-arm64-vt"
            session = "aqua"

            [[required]]
            id = "media"
            target = "linux-x64-vaapi-wayland"
            backend = "vaapi"
        "#,
    )
    .unwrap();

    assert_eq!(matrix.required.len(), 2);
    assert!(matrix.required.contains(&RequiredEvidence {
        id: SpikeId::Preview,
        target: TargetId::new("macos-arm64-vt"),
        session: Some("aqua".to_owned()),
        backend: None,
    }));
}

#[test]
fn required_evidence_accepts_only_passing_results() {
    let matrix = PhaseZeroMatrix::from_toml(
        r#"
            [[required]]
            id = "gemini"
            target = "windows-x64-mf"
        "#,
    )
    .unwrap();

    assert!(matrix.validate_required_verdict(Verdict::Pass).is_ok());
    assert!(
        matrix
            .validate_required_verdict(Verdict::Conditional)
            .is_err()
    );
    assert!(matrix.validate_required_verdict(Verdict::Skipped).is_err());
}

#[test]
fn checked_in_matrix_covers_every_required_real_device_capability() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packaging/phase-0-matrix.toml");
    let matrix = PhaseZeroMatrix::load(path).unwrap();

    assert_eq!(matrix.required.len(), 33);
    for target in [
        "macos-arm64-vt",
        "windows-x64-mf",
        "windows-x64-nvidia",
        "linux-x64-vaapi-wayland",
        "linux-x64-vaapi-x11",
        "linux-x64-nvidia",
    ] {
        assert!(
            matrix
                .required
                .iter()
                .any(|entry| entry.id == SpikeId::Gemini && entry.target == TargetId::new(target))
        );
        assert!(matrix.required.iter().any(|entry| {
            entry.id == SpikeId::Media
                && entry.target == TargetId::new(target)
                && entry.backend.as_deref() == Some("cpu-fallback")
        }));
    }

    assert!(matrix.required.contains(&RequiredEvidence {
        id: SpikeId::Media,
        target: TargetId::new("linux-x64-vaapi-x11"),
        session: None,
        backend: Some("vaapi".to_owned()),
    }));
    assert_eq!(
        matrix
            .required
            .iter()
            .filter(|entry| entry.id == SpikeId::Distribution)
            .count(),
        3
    );
}
