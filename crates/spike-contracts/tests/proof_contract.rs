use spike_contracts::{PhaseZeroProof, ProofComponent};

#[test]
fn typed_phase_zero_proof_rejects_generic_measurements_and_unknown_fields() {
    let generic = r#"{"schema_version":1,"spike":"preview","target":"macos-arm64-vt","verdict":"pass","duration_ms":120000,"measurements":{},"observations":[]}"#;
    assert!(PhaseZeroProof::from_json(generic).is_err());
    let unknown = r#"{"schema_version":2,"component":"preview","row":{"spike":"preview","target":"macos-arm64-vt","session":"aqua","backend":null},"proof":{"kind":"preview","requested_duration_ms":120000,"measured_duration_ms":120000,"milli_fps":24000,"p95_ms":1,"rss_growth_mib":1,"frames_read":1,"frames_applied":1,"frames_dropped":0,"hidden":true,"restored":true,"event_loop_errors":0,"stream_errors":0,"renderer":"software","extra":true}}"#;
    assert!(PhaseZeroProof::from_json(unknown).is_err());
    assert_eq!(ProofComponent::Preview.as_str(), "preview");
}

#[test]
fn distribution_proofs_require_hashes_and_platform_specific_attestation_fields() {
    let missing_hash = r#"{"schema_version":2,"component":"distribution_package","row":{"spike":"distribution","target":"macos-arm64-vt","session":null,"backend":null},"proof":{"kind":"distribution_package","artifacts":[],"inspection_sha256":"0000000000000000000000000000000000000000000000000000000000000000","platform_verification":"codesign_notary_staple","notarization":"accepted"}}"#;
    assert!(PhaseZeroProof::from_json(missing_hash).is_err());

    let unknown_tamper = r#"{"schema_version":2,"component":"distribution_update","row":{"spike":"distribution","target":"linux-x64-vaapi-wayland","session":null,"backend":null},"proof":{"kind":"distribution_update","manifest_sha256":"0000000000000000000000000000000000000000000000000000000000000000","artifacts":[],"updater_format":"appimage","signature_verification":"pinned_minisign","tamper_rejection":"updater_and_download","extra":true}}"#;
    assert!(PhaseZeroProof::from_json(unknown_tamper).is_err());
}
