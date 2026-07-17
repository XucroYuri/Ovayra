use std::{fs, path::Path};

use spike_release::{FfmpegBundle, FfmpegPolicyError};

const BUILD_CONF: &str = "configuration: --disable-autodetect --disable-debug --disable-doc --disable-ffplay --disable-network --enable-ffmpeg --enable-ffprobe --enable-libopus --enable-libvpx --enable-version3 --disable-gpl --disable-nonfree\n";

#[test]
fn rejects_gpl_and_nonfree_configurations_even_when_quoted_or_whitespace_separated() {
    for forbidden in [
        "--enable-gpl",
        "  --enable-nonfree",
        "'--enable-gpl'",
        "\"--enable-nonfree\"",
        "--disable-gpl --enable-gpl",
    ] {
        let error =
            FfmpegBundle::validate_buildconf(&format!("configuration: {forbidden}")).unwrap_err();
        assert!(matches!(
            error,
            FfmpegPolicyError::ForbiddenConfigureFlag(_)
        ));
    }
}

#[test]
fn rejects_duplicate_configure_tokens_to_prevent_ambiguous_effective_configuration() {
    let error = FfmpegBundle::validate_buildconf(&format!(
        "configuration: {} --disable-gpl",
        BUILD_CONF.trim_start_matches("configuration: ").trim()
    ))
    .unwrap_err();
    assert!(matches!(error, FfmpegPolicyError::InvalidBuildconf(_)));
}

#[test]
fn requires_corresponding_source_and_release_material() {
    let bundle = tempfile::tempdir().unwrap();
    let error = FfmpegBundle::validate_layout(bundle.path()).unwrap_err();
    assert!(matches!(error, FfmpegPolicyError::MissingArtifact(_)));
}

#[test]
fn rejects_duplicate_checksum_entries_and_unlisted_regular_files() {
    let bundle = valid_layout();
    fs::write(
        bundle.path().join("provenance/SHA256SUMS"),
        "00  provenance/buildconf.txt\n00  provenance/buildconf.txt\n",
    )
    .unwrap();
    let error =
        FfmpegBundle::validate_layout_with_lock(bundle.path(), &trusted_lock(bundle.path()))
            .unwrap_err();
    assert!(matches!(
        error,
        FfmpegPolicyError::InvalidChecksumManifest(_)
    ));
}

#[test]
fn rejects_an_executable_symlink_that_escapes_the_bundle() {
    let bundle = valid_layout();
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let outside = tempfile::NamedTempFile::new().unwrap();
        fs::remove_file(bundle.path().join("bin/ffmpeg")).unwrap();
        symlink(outside.path(), bundle.path().join("bin/ffmpeg")).unwrap();
        let error = FfmpegBundle::validate_layout(bundle.path()).unwrap_err();
        assert!(matches!(error, FfmpegPolicyError::UnsafePath(_)));
    }
}

#[test]
fn accepts_a_complete_lgpl_only_layout_with_cyclonedx_hash_and_license_evidence() {
    let bundle = valid_layout();
    FfmpegBundle::validate_layout_with_lock(bundle.path(), &trusted_lock(bundle.path())).unwrap();
}

#[test]
fn default_validator_rejects_a_bundle_with_a_coordinately_replaced_lock() {
    let bundle = valid_layout();
    let error = FfmpegBundle::validate_layout(bundle.path()).unwrap_err();
    assert!(matches!(error, FfmpegPolicyError::InvalidBuildconf(_)));
}

#[test]
fn windows_marker_requires_exe_paths_instead_of_host_platform_paths() {
    let bundle = valid_layout();
    write_file(bundle.path(), ".ovayra-target", "windows-x64-mf\n");
    let error = FfmpegBundle::validate_layout(bundle.path()).unwrap_err();
    assert!(matches!(error, FfmpegPolicyError::MissingArtifact(path) if path == "bin/ffmpeg.exe"));
}

#[test]
fn rejects_missing_or_unrecognized_target_marker() {
    let missing = valid_layout();
    fs::remove_file(missing.path().join(".ovayra-target")).unwrap();
    assert!(matches!(
        FfmpegBundle::validate_layout(missing.path()),
        Err(FfmpegPolicyError::MissingArtifact(_))
    ));
    let wrong = valid_layout();
    write_file(wrong.path(), ".ovayra-target", "windows-x64-unknown\n");
    rewrite_sums(wrong.path());
    assert!(matches!(
        FfmpegBundle::validate_layout(wrong.path()),
        Err(FfmpegPolicyError::UnsafePath(_))
    ));
}

#[test]
fn rejects_inexact_or_swapped_executable_version_banners() {
    let bundle = executable_layout("ffprobe version 8.1.2", "ffmpeg version 8.1.20");
    let error =
        FfmpegBundle::validate_with_lock(bundle.path(), &trusted_lock(bundle.path())).unwrap_err();
    assert!(matches!(error, FfmpegPolicyError::ExecutableCheck(_)));
}

#[test]
fn rejects_regenerated_manifest_when_sbom_archive_hash_is_replaced() {
    let bundle = valid_layout();
    let sbom = fs::read_to_string(bundle.path().join("sbom/ffmpeg.cdx.json")).unwrap();
    let hash = sha256(bundle.path().join("provenance/ffmpeg-8.1.2.tar.xz"));
    fs::write(
        bundle.path().join("sbom/ffmpeg.cdx.json"),
        sbom.replace(&hash, &"0".repeat(64)),
    )
    .unwrap();
    rewrite_sums(bundle.path());
    let error =
        FfmpegBundle::validate_layout_with_lock(bundle.path(), &trusted_lock(bundle.path()))
            .unwrap_err();
    assert!(matches!(error, FfmpegPolicyError::InvalidSbom(_)));
}

#[test]
fn rejects_replaced_source_after_checksum_manifest_is_regenerated() {
    let bundle = valid_layout();
    fs::write(
        bundle.path().join("provenance/ffmpeg-8.1.2.tar.xz"),
        "replacement",
    )
    .unwrap();
    rewrite_sums(bundle.path());
    let error =
        FfmpegBundle::validate_layout_with_lock(bundle.path(), &trusted_lock(bundle.path()))
            .unwrap_err();
    assert!(matches!(error, FfmpegPolicyError::ChecksumMismatch(_)));
}

fn valid_layout() -> tempfile::TempDir {
    let bundle = tempfile::tempdir().unwrap();
    write_file(bundle.path(), ".ovayra-target", "macos-arm64-vt\n");
    for relative in [
        "bin/ffmpeg",
        "bin/ffprobe",
        "provenance/ffmpeg-8.1.2.tar.xz",
        "provenance/ffmpeg-8.1.2.tar.xz.asc",
        "provenance/libvpx-source.tar.zst",
        "provenance/opus-source.tar.zst",
        "provenance/buildconf.txt",
        "provenance/changes.diff",
        "provenance/ffmpeg.lock",
        "provenance/ffmpeg-signature-attestation.json",
        "LICENSES/FFmpeg-LGPL-2.1-or-later.txt",
        "LICENSES/libvpx-BSD-3-Clause.txt",
        "LICENSES/Opus-BSD-3-Clause.txt",
        "NOTICE.txt",
        "sbom/ffmpeg.cdx.json",
    ] {
        write_file(
            bundle.path(),
            relative,
            if relative.ends_with("buildconf.txt") {
                BUILD_CONF
            } else {
                "fixture\n"
            },
        );
    }
    let ffmpeg_hash = sha256(bundle.path().join("provenance/ffmpeg-8.1.2.tar.xz"));
    write_file(
        bundle.path(),
        "provenance/ffmpeg.lock",
        &format!(
            "[ffmpeg]\nversion = \"8.1.2\"\nsha256 = \"{ffmpeg_hash}\"\nrelease_key_fingerprint = \"FCF986EA15E6E293A5644F10B4322F04D67658D8\"\n"
        ),
    );
    write_file(
        bundle.path(),
        "provenance/ffmpeg-signature-attestation.json",
        &format!(
            "{{\"verified\":true,\"fingerprint\":\"FCF986EA15E6E293A5644F10B4322F04D67658D8\",\"sha256\":\"{ffmpeg_hash}\"}}"
        ),
    );
    write_file(
        bundle.path(),
        "sbom/ffmpeg.cdx.json",
        &format!(
            r#"{{"bomFormat":"CycloneDX","specVersion":"1.5","components":[{{"name":"FFmpeg","version":"8.1.2","hashes":[{{"alg":"SHA-256","content":"{}"}}],"licenses":[{{"license":{{"id":"LGPL-2.1-or-later"}}}}]}},{{"name":"libvpx","version":"1.16.0","hashes":[{{"alg":"SHA-256","content":"{}"}}],"licenses":[{{"license":{{"id":"BSD-3-Clause"}}}}]}},{{"name":"opus","version":"1.6.1","hashes":[{{"alg":"SHA-256","content":"{}"}}],"licenses":[{{"license":{{"id":"BSD-3-Clause"}}}}]}}]}}"#,
            sha256(bundle.path().join("provenance/ffmpeg-8.1.2.tar.xz")),
            sha256(bundle.path().join("provenance/libvpx-source.tar.zst")),
            sha256(bundle.path().join("provenance/opus-source.tar.zst")),
        ),
    );
    let mut sums = String::new();
    for relative in required_files() {
        sums.push_str(&sha256(bundle.path().join(relative)));
        sums.push_str("  ");
        sums.push_str(relative);
        sums.push('\n');
    }
    write_file(bundle.path(), "provenance/SHA256SUMS", &sums);
    bundle
}

#[cfg(unix)]
fn executable_layout(ffmpeg_banner: &str, ffprobe_banner: &str) -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;
    let bundle = valid_layout();
    for (program, banner) in [("ffmpeg", ffmpeg_banner), ("ffprobe", ffprobe_banner)] {
        let file = bundle.path().join("bin").join(program);
        fs::write(&file, format!("#!/bin/sh\nif [ \"$1\" = -buildconf ]; then echo '{BUILD_CONF}'; else echo '{banner}'; fi\n")).unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
    }
    rewrite_sums(bundle.path());
    bundle
}

#[cfg(not(unix))]
fn executable_layout(_ffmpeg_banner: &str, _ffprobe_banner: &str) -> tempfile::TempDir {
    valid_layout()
}

fn rewrite_sums(root: &Path) {
    let mut sums = String::new();
    for relative in required_files() {
        sums.push_str(&sha256(root.join(relative)));
        sums.push_str("  ");
        sums.push_str(relative);
        sums.push('\n');
    }
    write_file(root, "provenance/SHA256SUMS", &sums);
}

fn required_files() -> [&'static str; 16] {
    [
        ".ovayra-target",
        "bin/ffmpeg",
        "bin/ffprobe",
        "provenance/ffmpeg-8.1.2.tar.xz",
        "provenance/ffmpeg-8.1.2.tar.xz.asc",
        "provenance/libvpx-source.tar.zst",
        "provenance/opus-source.tar.zst",
        "provenance/buildconf.txt",
        "provenance/changes.diff",
        "provenance/ffmpeg.lock",
        "provenance/ffmpeg-signature-attestation.json",
        "LICENSES/FFmpeg-LGPL-2.1-or-later.txt",
        "LICENSES/libvpx-BSD-3-Clause.txt",
        "LICENSES/Opus-BSD-3-Clause.txt",
        "NOTICE.txt",
        "sbom/ffmpeg.cdx.json",
    ]
}

fn write_file(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn sha256(path: impl AsRef<Path>) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(fs::read(path).unwrap()))
}

fn trusted_lock(root: &Path) -> String {
    fs::read_to_string(root.join("provenance/ffmpeg.lock")).unwrap()
}
