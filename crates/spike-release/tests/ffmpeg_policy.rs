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
    let error = FfmpegBundle::validate_layout(bundle.path()).unwrap_err();
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
    FfmpegBundle::validate_layout(bundle.path()).unwrap();
}

fn valid_layout() -> tempfile::TempDir {
    let bundle = tempfile::tempdir().unwrap();
    for relative in [
        "bin/ffmpeg",
        "bin/ffprobe",
        "provenance/ffmpeg-8.1.2.tar.xz",
        "provenance/ffmpeg-8.1.2.tar.xz.asc",
        "provenance/libvpx-source.tar.zst",
        "provenance/opus-source.tar.zst",
        "provenance/buildconf.txt",
        "provenance/changes.diff",
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
    write_file(
        bundle.path(),
        "sbom/ffmpeg.cdx.json",
        r#"{"bomFormat":"CycloneDX","specVersion":"1.5","components":[{"name":"FFmpeg","version":"8.1.2","hashes":[{"alg":"SHA-256","content":"0000000000000000000000000000000000000000000000000000000000000000"}],"licenses":[{"license":{"id":"LGPL-2.1-or-later"}}]},{"name":"libvpx","version":"1.16.0","hashes":[{"alg":"SHA-256","content":"0000000000000000000000000000000000000000000000000000000000000000"}],"licenses":[{"license":{"id":"BSD-3-Clause"}}]},{"name":"opus","version":"1.6.1","hashes":[{"alg":"SHA-256","content":"0000000000000000000000000000000000000000000000000000000000000000"}],"licenses":[{"license":{"id":"BSD-3-Clause"}}]}]}"#,
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

fn required_files() -> [&'static str; 13] {
    [
        "bin/ffmpeg",
        "bin/ffprobe",
        "provenance/ffmpeg-8.1.2.tar.xz",
        "provenance/ffmpeg-8.1.2.tar.xz.asc",
        "provenance/libvpx-source.tar.zst",
        "provenance/opus-source.tar.zst",
        "provenance/buildconf.txt",
        "provenance/changes.diff",
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
