use spike_contracts::{PhaseZeroProof, ProofComponent};

#[test]
fn typed_phase_zero_proof_rejects_generic_measurements_and_unknown_fields() {
    let generic = r#"{"schema_version":1,"spike":"preview","target":"macos-arm64-vt","verdict":"pass","duration_ms":120000,"measurements":{},"observations":[]}"#;
    assert!(PhaseZeroProof::from_json(generic).is_err());
    let unknown = r#"{"schema_version":2,"component":"preview","row":{"spike":"preview","target":"macos-arm64-vt","session":"aqua","backend":null},"proof":{"kind":"preview","requested_duration_ms":120000,"measured_duration_ms":120000,"milli_fps":24000,"p95_ms":1,"rss_growth_mib":1,"frames_read":1,"frames_applied":1,"frames_dropped":0,"hidden":true,"restored":true,"event_loop_errors":0,"stream_errors":0,"renderer":"software","extra":true}}"#;
    assert!(PhaseZeroProof::from_json(unknown).is_err());
    assert_eq!(ProofComponent::Preview.as_str(), "preview");
}
