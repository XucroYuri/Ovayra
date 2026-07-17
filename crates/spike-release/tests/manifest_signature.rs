use std::fs;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use semver::Version;
use spike_release::{PackageRelease, ReleaseManifest, ReleaseVerifier};

const INSTALLED_VERSION: &str = "0.0.1";

fn signed_fixture() -> (Vec<u8>, String, String) {
    (
        fs::read("tests/fixtures/update-test.bin").unwrap(),
        fs::read_to_string("tests/fixtures/update-test.bin.minisig").unwrap(),
        fs::read_to_string("tests/fixtures/update-test.pub").unwrap(),
    )
}

#[test]
fn manifest_accepts_only_the_three_supported_update_targets() {
    let manifest = ReleaseManifest::parse_for_installed(
        include_str!("fixtures/valid-manifest.json"),
        &Version::parse(INSTALLED_VERSION).unwrap(),
    )
    .unwrap();

    assert_eq!(manifest.version().to_string(), "0.0.2");
    assert_eq!(manifest.platform_count(), 3);
}

#[test]
fn manifest_rejects_unknown_fields_and_ambiguous_versions() {
    let unknown = r#"{
      "version":"0.0.2", "pub_date":"2026-07-17T00:00:00Z", "notes":"x",
      "platforms":{}, "unreviewed":true
    }"#;
    assert!(
        ReleaseManifest::parse_for_installed(unknown, &Version::parse(INSTALLED_VERSION).unwrap())
            .is_err()
    );

    for version in ["0.0.1", "0.0.0", "0.0.2-alpha.1"] {
        let json = format!(
            r#"{{"version":"{version}","pub_date":"2026-07-17T00:00:00Z","notes":"x","platforms":{{}}}}"#
        );
        assert!(
            ReleaseManifest::parse_for_installed(
                &json,
                &Version::parse(INSTALLED_VERSION).unwrap()
            )
            .is_err()
        );
    }
}

#[test]
fn manifest_rejects_noncanonical_update_urls_and_target_format_mismatches() {
    let json = r#"{
      "version":"0.0.2", "pub_date":"2026-07-17T00:00:00Z", "notes":"release",
      "platforms": {
        "darwin-aarch64": {
          "url":"https://updates.ovayra.com:443/phase-0/Ovayra.dmg",
          "signature":"untrusted comment: x", "format":"dmg",
          "sha256":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        }
      }
    }"#;
    assert!(
        ReleaseManifest::parse_for_installed(json, &Version::parse(INSTALLED_VERSION).unwrap())
            .is_err()
    );
}

#[test]
fn one_byte_package_signature_key_or_hash_tampering_is_rejected() {
    let (package, signature, public_key) = signed_fixture();
    let verifier = ReleaseVerifier::new(&public_key).unwrap();
    verifier.verify(&package, &signature).unwrap();

    let mut changed_package = package.clone();
    changed_package[0] ^= 1;
    assert!(verifier.verify(&changed_package, &signature).is_err());

    let mut changed_signature = signature.clone().into_bytes();
    let index = changed_signature
        .iter()
        .position(|byte| *byte == b'A')
        .unwrap();
    changed_signature[index] = b'B';
    assert!(
        verifier
            .verify(&package, std::str::from_utf8(&changed_signature).unwrap())
            .is_err()
    );

    let mut changed_key = public_key.into_bytes();
    let index = changed_key.iter().rposition(|byte| *byte == b'A').unwrap();
    changed_key[index] = b'B';
    assert!(
        ReleaseVerifier::new(std::str::from_utf8(&changed_key).unwrap())
            .unwrap()
            .verify(&package, &signature)
            .is_err()
    );
}

#[test]
fn package_manifest_uses_only_signed_updater_artifacts_and_keeps_deb_downloads_separate() {
    let packages = tempfile::tempdir().unwrap();
    let output = tempfile::tempdir().unwrap();
    for name in [
        "ovayra-phase-0_0.0.2_darwin-aarch64.app.tar.gz",
        "ovayra-phase-0_0.0.2_windows-x86_64.msi",
        "ovayra-phase-0_0.0.2_linux-x86_64.AppImage",
        "ovayra-phase-0_0.0.2_darwin-aarch64.dmg",
        "ovayra-phase-0_0.0.2_linux-x86_64.deb",
    ] {
        fs::copy("tests/fixtures/update-test.bin", packages.path().join(name)).unwrap();
        let raw = fs::read_to_string("tests/fixtures/update-test.bin.minisig").unwrap();
        fs::write(
            packages.path().join(format!("{name}.sig")),
            STANDARD.encode(raw),
        )
        .unwrap();
    }

    let latest = output.path().join("latest.json");
    PackageRelease::generate_manifest(
        packages.path(),
        "https://updates.ovayra.com/phase-0/",
        &latest,
        &Version::parse("0.0.2").unwrap(),
        "2026-07-17T00:00:00Z",
        "release",
    )
    .unwrap();

    let manifest = ReleaseManifest::parse_for_installed(
        &fs::read_to_string(&latest).unwrap(),
        &Version::parse(INSTALLED_VERSION).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest.platform_count(), 3);
    let downloads = fs::read_to_string(output.path().join("downloads.json")).unwrap();
    assert!(downloads.contains(".deb"));
    assert!(!fs::read_to_string(&latest).unwrap().contains(".deb"));
}

#[test]
fn package_manifest_verification_rejects_a_tampered_real_file_without_touching_the_source() {
    let packages = tempfile::tempdir().unwrap();
    let output = tempfile::tempdir().unwrap();
    for name in [
        "ovayra-phase-0_0.0.2_darwin-aarch64.app.tar.gz",
        "ovayra-phase-0_0.0.2_windows-x86_64.msi",
        "ovayra-phase-0_0.0.2_linux-x86_64.AppImage",
    ] {
        fs::copy("tests/fixtures/update-test.bin", packages.path().join(name)).unwrap();
        fs::copy(
            "tests/fixtures/update-test.bin.minisig",
            packages.path().join(format!("{name}.minisig")),
        )
        .unwrap();
    }
    let latest = output.path().join("latest.json");
    PackageRelease::generate_manifest(
        packages.path(),
        "https://updates.ovayra.com/phase-0/",
        &latest,
        &Version::parse("0.0.2").unwrap(),
        "2026-07-17T00:00:00Z",
        "release",
    )
    .unwrap();
    let public_key = fs::read_to_string("tests/fixtures/update-test.pub").unwrap();
    PackageRelease::verify_manifest(
        &latest,
        packages.path(),
        &public_key,
        &Version::parse(INSTALLED_VERSION).unwrap(),
    )
    .unwrap();

    let source = packages
        .path()
        .join("ovayra-phase-0_0.0.2_windows-x86_64.msi");
    let original = fs::read(&source).unwrap();
    assert!(
        PackageRelease::verify_tamper_rejection(
            &latest,
            packages.path(),
            &public_key,
            &Version::parse(INSTALLED_VERSION).unwrap()
        )
        .is_ok()
    );
    assert_eq!(fs::read(source).unwrap(), original);
}

#[test]
fn package_rejects_a_malformed_double_encoded_or_ambiguous_cargo_packager_signature() {
    let packages = tempfile::tempdir().unwrap();
    let output = tempfile::tempdir().unwrap();
    let names = [
        "ovayra-phase-0_0.0.2_darwin-aarch64.app.tar.gz",
        "ovayra-phase-0_0.0.2_windows-x86_64.msi",
        "ovayra-phase-0_0.0.2_linux-x86_64.AppImage",
    ];
    for name in names {
        fs::copy("tests/fixtures/update-test.bin", packages.path().join(name)).unwrap();
        let raw = fs::read_to_string("tests/fixtures/update-test.bin.minisig").unwrap();
        fs::write(
            packages.path().join(format!("{name}.sig")),
            STANDARD.encode(&raw),
        )
        .unwrap();
    }
    let first = packages.path().join(names[0]);
    let once = fs::read_to_string(first.with_file_name(format!("{}.sig", names[0]))).unwrap();
    fs::write(
        first.with_file_name(format!("{}.minisig", names[0])),
        STANDARD.encode(once),
    )
    .unwrap();
    assert!(
        PackageRelease::generate_manifest(
            packages.path(),
            "https://updates.ovayra.com/phase-0/",
            &output.path().join("latest.json"),
            &Version::parse("0.0.2").unwrap(),
            "2026-07-17T00:00:00Z",
            "release"
        )
        .is_err()
    );
}
