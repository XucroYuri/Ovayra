use std::{fmt::Write as _, fs, path::Path};

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
fn accepts_the_official_multiline_buildconf_format_and_rejects_an_empty_record() {
    let multiline = format!(
        "\n  configuration:\n    {}\n    --extra-cflags='-MD -ID:/a path/include'\n    --extra-libs=opus.lib\\ vpx.lib\n\nExiting with exit code 0\n",
        BUILD_CONF
            .trim_start_matches("configuration: ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join("\n    ")
    );
    FfmpegBundle::validate_buildconf(&multiline).unwrap();

    let error =
        FfmpegBundle::validate_buildconf("\n  configuration:\n\nExiting with exit code 0\n")
            .unwrap_err();
    assert!(matches!(error, FfmpegPolicyError::InvalidBuildconf(_)));

    for malformed in [
        "configuration: --disable-gpl '--disable-nonfree",
        "configuration: --disable-gpl --disable-nonfree\\",
    ] {
        let error = FfmpegBundle::validate_buildconf(malformed).unwrap_err();
        assert!(matches!(error, FfmpegPolicyError::InvalidBuildconf(_)));
    }
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
fn accepts_additional_installed_files_only_when_the_manifest_covers_them() {
    let bundle = valid_layout();
    let relative = "include/libavcodec/avcodec.h";
    write_file(bundle.path(), relative, "installed header");

    let error =
        FfmpegBundle::validate_layout_with_lock(bundle.path(), &trusted_lock(bundle.path()))
            .unwrap_err();
    assert!(matches!(
        error,
        FfmpegPolicyError::InvalidChecksumManifest(message)
            if message == format!("missing {relative}")
    ));

    let mut manifest = fs::read_to_string(bundle.path().join("provenance/SHA256SUMS")).unwrap();
    writeln!(
        manifest,
        "{}  {relative}",
        sha256(bundle.path().join(relative))
    )
    .unwrap();
    fs::write(bundle.path().join("provenance/SHA256SUMS"), manifest).unwrap();
    FfmpegBundle::validate_layout_with_lock(bundle.path(), &trusted_lock(bundle.path())).unwrap();
}

#[cfg(unix)]
#[test]
fn rejects_an_executable_symlink_that_escapes_the_bundle() {
    use std::os::unix::fs::symlink;

    let bundle = valid_layout();
    let outside = tempfile::NamedTempFile::new().unwrap();
    fs::remove_file(bundle.path().join("bin/ffmpeg")).unwrap();
    symlink(outside.path(), bundle.path().join("bin/ffmpeg")).unwrap();
    let error = FfmpegBundle::validate_layout(bundle.path()).unwrap_err();
    assert!(matches!(error, FfmpegPolicyError::UnsafePath(_)));
}

#[test]
fn accepts_a_complete_lgpl_only_layout_with_cyclonedx_hash_and_license_evidence() {
    let bundle = valid_layout();
    FfmpegBundle::validate_layout_with_lock(bundle.path(), &trusted_lock(bundle.path())).unwrap();
}

#[test]
fn rejects_a_minimal_or_unknown_field_lock_even_with_a_matching_manifest() {
    let bundle = valid_layout();
    write_file(
        bundle.path(),
        "provenance/ffmpeg.lock",
        "[ffmpeg]\nversion = \"8.1.2\"\n",
    );
    rewrite_sums(bundle.path());
    assert!(matches!(
        FfmpegBundle::validate_layout_with_lock(bundle.path(), &trusted_lock(bundle.path())),
        Err(FfmpegPolicyError::InvalidBuildconf(_))
    ));
}

#[test]
fn rejects_empty_signature_wrong_signer_and_unsafe_archive_paths() {
    let empty = valid_layout();
    fs::write(empty.path().join("provenance/ffmpeg-8.1.2.tar.xz.asc"), "").unwrap();
    rewrite_sums(empty.path());
    assert!(matches!(
        FfmpegBundle::validate_layout_with_lock(empty.path(), &trusted_lock(empty.path())),
        Err(FfmpegPolicyError::InvalidBuildconf(_))
    ));

    let signer = valid_layout();
    let attestation = fs::read_to_string(
        signer
            .path()
            .join("provenance/ffmpeg-signature-attestation.json"),
    )
    .unwrap();
    fs::write(
        signer
            .path()
            .join("provenance/ffmpeg-signature-attestation.json"),
        attestation.replace(
            "FCF986EA15E6E293A5644F10B4322F04D67658D8",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        ),
    )
    .unwrap();
    rewrite_sums(signer.path());
    assert!(matches!(
        FfmpegBundle::validate_layout_with_lock(signer.path(), &trusted_lock(signer.path())),
        Err(FfmpegPolicyError::InvalidBuildconf(_))
    ));

    for unsafe_entry in ["outside/COPYING", "ffmpeg-8.1.2/../escape"] {
        let archive = valid_layout();
        write_ffmpeg_archive(archive.path(), unsafe_entry);
        refresh_provenance(archive.path());
        assert!(matches!(
            FfmpegBundle::validate_layout_with_lock(archive.path(), &trusted_lock(archive.path())),
            Err(FfmpegPolicyError::InvalidBuildconf(_))
        ));
    }
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

#[cfg(unix)]
#[test]
fn accepts_official_release_version_banners_with_copyright_suffixes() {
    let bundle = executable_layout(
        "ffmpeg version 8.1.2 Copyright (c) 2000-2026 the FFmpeg developers",
        "ffprobe version 8.1.2 Copyright (c) 2007-2026 the FFmpeg developers",
    );
    FfmpegBundle::validate_with_lock(bundle.path(), &trusted_lock(bundle.path())).unwrap();
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
    write_ffmpeg_archive(bundle.path(), "ffmpeg-8.1.2/COPYING.LGPLv2.1");
    let ffmpeg_hash = sha256(bundle.path().join("provenance/ffmpeg-8.1.2.tar.xz"));
    write_file(
        bundle.path(),
        "provenance/ffmpeg.lock",
        &fixture_lock(&ffmpeg_hash),
    );
    write_file(
        bundle.path(),
        "provenance/ffmpeg-signature-attestation.json",
        &format!(
            "{{\"schema_version\":1,\"verified\":true,\"signer_fingerprint\":\"FCF986EA15E6E293A5644F10B4322F04D67658D8\",\"primary_fingerprint\":\"FCF986EA15E6E293A5644F10B4322F04D67658D8\",\"sha256\":\"{ffmpeg_hash}\"}}"
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

fn write_ffmpeg_archive(root: &Path, entry: &str) {
    use std::io::Write;
    let destination = fs::File::create(root.join("provenance/ffmpeg-8.1.2.tar.xz")).unwrap();
    let mut archive = tar::Builder::new(Vec::new());
    let bytes = b"LGPL";
    let mut header = tar::Header::new_gnu();
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    archive
        .append_data(&mut header, "ffmpeg-8.1.2/safe", &bytes[..])
        .unwrap();
    let mut raw = archive.into_inner().unwrap();
    raw[..100].fill(0);
    raw[..entry.len()].copy_from_slice(entry.as_bytes());
    raw[148..156].fill(b' ');
    let checksum: u32 = raw[..512].iter().map(|byte| u32::from(*byte)).sum();
    let text = format!("{checksum:06o}\0 ");
    raw[148..156].copy_from_slice(text.as_bytes());
    let mut encoder = xz2::write::XzEncoder::new(destination, 6);
    encoder.write_all(&raw).unwrap();
    encoder.flush().unwrap();
    encoder.finish().unwrap();
}

fn refresh_provenance(root: &Path) {
    let hash = sha256(root.join("provenance/ffmpeg-8.1.2.tar.xz"));
    write_file(root, "provenance/ffmpeg.lock", &fixture_lock(&hash));
    write_file(
        root,
        "provenance/ffmpeg-signature-attestation.json",
        &format!(
            "{{\"schema_version\":1,\"verified\":true,\"signer_fingerprint\":\"FCF986EA15E6E293A5644F10B4322F04D67658D8\",\"primary_fingerprint\":\"FCF986EA15E6E293A5644F10B4322F04D67658D8\",\"sha256\":\"{hash}\"}}"
        ),
    );
    let sbom = fs::read_to_string(root.join("sbom/ffmpeg.cdx.json")).unwrap();
    let previous = sha256(root.join("provenance/ffmpeg-8.1.2.tar.xz"));
    fs::write(
        root.join("sbom/ffmpeg.cdx.json"),
        sbom.replace(&previous, &hash),
    )
    .unwrap();
    rewrite_sums(root);
}

fn fixture_lock(hash: &str) -> String {
    format!(
        "[ffmpeg]\nversion = \"8.1.2\"\nsource_url = \"https://ffmpeg.org/releases/ffmpeg-8.1.2.tar.xz\"\nsignature_url = \"https://ffmpeg.org/releases/ffmpeg-8.1.2.tar.xz.asc\"\nsha256 = \"{hash}\"\nrelease_key_fingerprint = \"FCF986EA15E6E293A5644F10B4322F04D67658D8\"\nrelease_tag = \"n8.1.2\"\nrelease_commit = \"38b88335f99e76ed89ff3c93f877fdefce736c13\"\nsource_date_epoch = 1\n\n[libvpx]\ntag = \"v1.16.0\"\npeeled_commit = \"1024874c5919305883187e2953de8fcb4c3d7fa6\"\nsource_url = \"https://github.com/webmproject/libvpx.git\"\nlicense = \"BSD-3-Clause\"\n\n[opus]\ntag = \"v1.6.1\"\npeeled_commit = \"22244de5a79bd1d6d623c32e72bf1954b56235be\"\nsource_url = \"https://github.com/xiph/opus.git\"\nlicense = \"BSD-3-Clause\"\n\n[nv_codec_headers]\ntag = \"n13.0.19.0\"\npeeled_commit = \"e844e5b26f46bb77479f063029595293aa8f812d\"\nsource_url = \"https://github.com/FFmpeg/nv-codec-headers.git\"\nlicense = \"MIT\"\n\n[[target]]\nid = \"macos-arm64-vt\"\ntriple = \"aarch64-apple-darwin\"\nbuilder_image = \"macos-14\"\nbuilder_os = \"macos\"\n\n[[target]]\nid = \"windows-x64-mf\"\ntriple = \"x86_64-pc-windows-msvc\"\nbuilder_image = \"windows-2025\"\nbuilder_os = \"windows\"\n\n[[target]]\nid = \"linux-x64-vaapi-wayland\"\ntriple = \"x86_64-unknown-linux-gnu\"\nbuilder_image = \"ubuntu-24.04\"\nbuilder_os = \"linux\"\n"
    )
}

fn sha256(path: impl AsRef<Path>) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(fs::read(path).unwrap()))
}

fn trusted_lock(root: &Path) -> String {
    fs::read_to_string(root.join("provenance/ffmpeg.lock")).unwrap()
}
