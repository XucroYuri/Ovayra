use spike_contracts::{
    Evidence, EvidenceError, MatrixError, PhaseZeroMatrix, RequiredEvidence, SpikeId, TargetId,
    Verdict,
};

fn target(value: &str) -> TargetId {
    TargetId::new(value).unwrap()
}

fn required(
    id: SpikeId,
    target_id: &str,
    session: Option<&str>,
    backend: Option<&str>,
) -> RequiredEvidence {
    RequiredEvidence {
        id,
        target: target(target_id),
        session: session.map(str::to_owned),
        backend: backend.map(str::to_owned),
    }
}

#[test]
fn rejects_sensitive_measurement_names() {
    let mut evidence = Evidence::new(SpikeId::Gemini, target("macos-arm64-vt"));
    let error = evidence
        .measure("upload_url", "https://secret.invalid")
        .unwrap_err();
    assert!(matches!(error, EvidenceError::SensitiveField(_)));
}

#[test]
fn rejects_sensitive_measurement_keys_at_every_nesting_depth() {
    let mut evidence = Evidence::new(SpikeId::Gemini, target("macos-arm64-vt"));
    let error = evidence
        .measure(
            "metrics",
            serde_json::json!({"outer": [{"token": "redacted"}]}),
        )
        .unwrap_err();
    assert!(matches!(error, EvidenceError::SensitiveField(field) if field == "token"));

    let error = evidence
        .measure("metrics", serde_json::json!({"outer": {"secret_value": 1}}))
        .unwrap_err();
    assert!(matches!(error, EvidenceError::SensitiveField(field) if field == "secret_value"));
}

#[test]
fn observations_are_guarded() {
    let mut evidence = Evidence::new(SpikeId::Platform, target("windows-x64-mf"));
    let error = evidence.observe("token copied to clipboard").unwrap_err();
    assert!(matches!(error, EvidenceError::SensitiveObservation(_)));
}

#[test]
fn serializes_only_finished_evidence() {
    let evidence = Evidence::new(SpikeId::Preview, target("macos-arm64-vt"));
    assert!(matches!(
        evidence.to_pretty_json(),
        Err(EvidenceError::Unfinished)
    ));
}

#[test]
fn finished_report_has_stable_schema() {
    let mut evidence = Evidence::new(SpikeId::Media, target("linux-x64-vaapi-wayland"));
    evidence.measure("p95_latency_ms", 18).unwrap();
    evidence.observe("CPU fallback was not used").unwrap();
    evidence.finish(Verdict::Pass, 1_250);
    let json = evidence.to_pretty_json().unwrap();
    assert!(json.contains("\"schema_version\": 1"));
    assert!(json.contains("\"verdict\": \"pass\""));
    assert_eq!(
        Evidence::from_json(&json).unwrap().observations(),
        ["CPU fallback was not used"]
    );
}

#[test]
fn from_json_rejects_unknown_wrong_version_and_unfinished_evidence() {
    for invalid in [
        r#"{"schema_version":1,"spike":"preview","target":"macos-arm64-vt","verdict":"pass","duration_ms":1,"measurements":{},"observations":[],"extra":true}"#,
        r#"{"schema_version":2,"spike":"preview","target":"macos-arm64-vt","verdict":"pass","duration_ms":1,"measurements":{},"observations":[]}"#,
        r#"{"schema_version":1,"spike":"preview","target":"macos-arm64-vt","verdict":null,"duration_ms":null,"measurements":{},"observations":[]}"#,
        r#"{"schema_version":1,"spike":"preview","target":"macos-arm64-vt","verdict":"pass","duration_ms":1,"measurements":{"ok":[{"api_token":"no"}]},"observations":[]}"#,
        r#"{"schema_version":1,"spike":"preview","target":"macos-arm64-vt","verdict":"pass","duration_ms":1,"measurements":{},"observations":["secret copied"]}"#,
    ] {
        assert!(Evidence::from_json(invalid).is_err(), "{invalid}");
    }
}

#[test]
fn target_id_rejects_unsupported_values_in_construction_and_deserialization() {
    assert!(TargetId::new("linux-arm64").is_err());
    assert!(serde_json::from_str::<TargetId>("\"linux-arm64\"").is_err());
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
    assert!(matrix.required.contains(&required(
        SpikeId::Preview,
        "macos-arm64-vt",
        Some("aqua"),
        None
    )));
}

#[test]
fn matrix_rejects_unknown_fields_duplicates_empty_qualifiers_and_unsupported_targets() {
    for invalid in [
        r#"[[required]]
id = "preview"
target = "macos-arm64-vt"
unexpected = true"#,
        r#"[[required]]
id = "preview"
target = "macos-arm64-vt"

[[required]]
id = "preview"
target = "macos-arm64-vt""#,
        r#"[[required]]
id = "preview"
target = "macos-arm64-vt"
session = """#,
        r#"[[required]]
id = "preview"
target = "linux-arm64""#,
    ] {
        assert!(PhaseZeroMatrix::from_toml(invalid).is_err(), "{invalid}");
    }
}

#[test]
fn required_evidence_validation_requires_a_matching_passing_entry() {
    let matrix = PhaseZeroMatrix::from_toml(
        r#"[[required]]
id = "gemini"
target = "windows-x64-mf""#,
    )
    .unwrap();
    let present = required(SpikeId::Gemini, "windows-x64-mf", None, None);
    let absent = required(SpikeId::Gemini, "macos-arm64-vt", None, None);

    assert!(
        matrix
            .validate_required_verdict(&present, Verdict::Pass)
            .is_ok()
    );
    assert!(matches!(
        matrix.validate_required_verdict(&present, Verdict::Conditional),
        Err(MatrixError::RequiredVerdict(Verdict::Conditional))
    ));
    assert!(matches!(
        matrix.validate_required_verdict(&present, Verdict::Skipped),
        Err(MatrixError::RequiredVerdict(Verdict::Skipped))
    ));
    assert!(matches!(
        matrix.validate_required_verdict(&present, Verdict::Fail),
        Err(MatrixError::RequiredVerdict(Verdict::Fail))
    ));
    assert!(matches!(
        matrix.validate_required_verdict(&absent, Verdict::Pass),
        Err(MatrixError::MissingRequiredEvidence)
    ));
}

#[test]
fn checked_in_matrix_is_the_exact_required_real_device_set() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packaging/phase-0-matrix.toml");
    let matrix = PhaseZeroMatrix::load(path).unwrap();

    let expected = vec![
        required(SpikeId::Preview, "macos-arm64-vt", Some("aqua"), None),
        required(SpikeId::Media, "macos-arm64-vt", None, Some("videotoolbox")),
        required(SpikeId::Media, "macos-arm64-vt", None, Some("cpu-fallback")),
        required(SpikeId::Platform, "macos-arm64-vt", Some("aqua"), None),
        required(SpikeId::Gemini, "macos-arm64-vt", None, None),
        required(SpikeId::Distribution, "macos-arm64-vt", None, None),
        required(SpikeId::Preview, "windows-x64-mf", Some("windows"), None),
        required(SpikeId::Media, "windows-x64-mf", None, Some("d3d11va-mf")),
        required(SpikeId::Media, "windows-x64-mf", None, Some("cpu-fallback")),
        required(SpikeId::Platform, "windows-x64-mf", Some("windows"), None),
        required(SpikeId::Gemini, "windows-x64-mf", None, None),
        required(SpikeId::Distribution, "windows-x64-mf", None, None),
        required(
            SpikeId::Preview,
            "windows-x64-nvidia",
            Some("windows"),
            None,
        ),
        required(
            SpikeId::Media,
            "windows-x64-nvidia",
            None,
            Some("nvenc-nvdec"),
        ),
        required(
            SpikeId::Media,
            "windows-x64-nvidia",
            None,
            Some("cpu-fallback"),
        ),
        required(
            SpikeId::Platform,
            "windows-x64-nvidia",
            Some("windows"),
            None,
        ),
        required(SpikeId::Gemini, "windows-x64-nvidia", None, None),
        required(
            SpikeId::Preview,
            "linux-x64-vaapi-wayland",
            Some("wayland"),
            None,
        ),
        required(
            SpikeId::Media,
            "linux-x64-vaapi-wayland",
            None,
            Some("vaapi"),
        ),
        required(
            SpikeId::Media,
            "linux-x64-vaapi-wayland",
            None,
            Some("cpu-fallback"),
        ),
        required(
            SpikeId::Platform,
            "linux-x64-vaapi-wayland",
            Some("wayland"),
            None,
        ),
        required(SpikeId::Gemini, "linux-x64-vaapi-wayland", None, None),
        required(SpikeId::Distribution, "linux-x64-vaapi-wayland", None, None),
        required(SpikeId::Preview, "linux-x64-vaapi-x11", Some("x11"), None),
        required(SpikeId::Media, "linux-x64-vaapi-x11", None, Some("vaapi")),
        required(
            SpikeId::Media,
            "linux-x64-vaapi-x11",
            None,
            Some("cpu-fallback"),
        ),
        required(SpikeId::Platform, "linux-x64-vaapi-x11", Some("x11"), None),
        required(SpikeId::Gemini, "linux-x64-vaapi-x11", None, None),
        required(SpikeId::Preview, "linux-x64-nvidia", None, None),
        required(
            SpikeId::Media,
            "linux-x64-nvidia",
            None,
            Some("nvenc-nvdec"),
        ),
        required(
            SpikeId::Media,
            "linux-x64-nvidia",
            None,
            Some("cpu-fallback"),
        ),
        required(SpikeId::Platform, "linux-x64-nvidia", None, None),
        required(SpikeId::Gemini, "linux-x64-nvidia", None, None),
    ];

    assert_eq!(matrix.required, expected);
}
