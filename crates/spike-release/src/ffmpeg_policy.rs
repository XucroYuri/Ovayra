//! Fail-closed verification for a redistributable `FFmpeg` bundle.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Component as PathComponent, Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;
use url::Url;

const FFMPEG_VERSION: &str = "8.1.2";
const PROJECT_LOCK: &str = include_str!("../../../packaging/ffmpeg.lock");
const REQUIRED_ARTIFACTS: &[&str] = &[
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
    "provenance/SHA256SUMS",
];
const BASELINE_FLAGS: &[&str] = &[
    "--disable-autodetect",
    "--disable-debug",
    "--disable-doc",
    "--disable-ffplay",
    "--disable-network",
    "--enable-ffmpeg",
    "--enable-ffprobe",
    "--enable-libopus",
    "--enable-libvpx",
    "--enable-version3",
    "--disable-gpl",
    "--disable-nonfree",
];

#[derive(Debug, Error)]
pub enum FfmpegPolicyError {
    #[error("missing required bundle artifact: {0}")]
    MissingArtifact(String),
    #[error("unsafe bundle path: {0}")]
    UnsafePath(String),
    #[error("invalid SHA256SUMS manifest: {0}")]
    InvalidChecksumManifest(String),
    #[error("checksum mismatch: {0}")]
    ChecksumMismatch(String),
    #[error("forbidden configure flag: {0}")]
    ForbiddenConfigureFlag(String),
    #[error("missing required configure flag: {0}")]
    MissingConfigureFlag(String),
    #[error("invalid build configuration: {0}")]
    InvalidBuildconf(String),
    #[error("invalid CycloneDX SBOM: {0}")]
    InvalidSbom(String),
    #[error("FFmpeg executable check failed: {0}")]
    ExecutableCheck(String),
    #[error("I/O failure: {0}")]
    Io(#[from] std::io::Error),
}

/// A policy namespace; the bundle is deliberately checked from disk, never trusted in memory.
pub struct FfmpegBundle;

impl FfmpegBundle {
    /// Verifies all non-executable material. This is usable in tests and before execution.
    ///
    /// # Errors
    ///
    /// Returns an error for missing, unsafe, unchecked, modified, unlicensed, or malformed
    /// bundle material.
    pub fn validate_layout(root: &Path) -> Result<(), FfmpegPolicyError> {
        Self::validate_layout_with_lock(root, PROJECT_LOCK)
    }

    /// Verifies a bundle against a caller-supplied, authenticated project lock.
    ///
    /// # Errors
    ///
    /// Returns an error unless the bundled lock is byte-identical to `trusted_lock` and every
    /// required material check succeeds. Production callers should use [`Self::validate_layout`].
    pub fn validate_layout_with_lock(
        root: &Path,
        trusted_lock: &str,
    ) -> Result<(), FfmpegPolicyError> {
        let root = canonical_directory(root)?;
        let artifacts = required_artifacts(&root)?;
        for artifact in &artifacts {
            let path = root.join(artifact);
            require_regular(&root, &path, artifact)?;
        }
        let files = collect_bundle_files(&root)?;
        validate_checksums(&root, &artifacts, &files)?;
        let lock = validate_locked_source(&root, trusted_lock)?;
        let buildconf = fs::read_to_string(root.join("provenance/buildconf.txt"))?;
        Self::validate_buildconf(&buildconf)?;
        validate_sbom(&root, &lock)
    }

    /// Runs only verified in-root executables with a bounded wall-clock deadline.
    ///
    /// # Errors
    ///
    /// Returns an error if layout validation fails, an executable is unsafe, times out, exits
    /// unsuccessfully, or reports a source/configuration that violates policy.
    pub fn validate(root: &Path) -> Result<(), FfmpegPolicyError> {
        Self::validate_layout(root)?;
        validate_executables(root)
    }

    /// Executes a bundle after verifying it against a caller-supplied authenticated lock.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::validate`] and fails before executing any binary if
    /// the bundle does not match `trusted_lock`.
    pub fn validate_with_lock(root: &Path, trusted_lock: &str) -> Result<(), FfmpegPolicyError> {
        Self::validate_layout_with_lock(root, trusted_lock)?;
        validate_executables(root)
    }

    /// Parses the `configuration:` record as shell-like tokens, rejecting ambiguous quoting.
    ///
    /// # Errors
    ///
    /// Returns an error for a malformed record, duplicate/forbidden token, or absent baseline
    /// policy token.
    pub fn validate_buildconf(buildconf: &str) -> Result<(), FfmpegPolicyError> {
        let line = buildconf
            .lines()
            .find_map(|line| line.trim_start().strip_prefix("configuration:"))
            .ok_or_else(|| {
                FfmpegPolicyError::InvalidBuildconf("missing configuration record".into())
            })?;
        let tokens = parse_tokens(line.trim())?;
        let values: BTreeSet<_> = tokens.iter().map(String::as_str).collect();
        if values.len() != tokens.len() {
            return Err(FfmpegPolicyError::InvalidBuildconf(
                "duplicate configure token".into(),
            ));
        }
        for forbidden in ["--enable-gpl", "--enable-nonfree"] {
            if values.contains(forbidden) {
                return Err(FfmpegPolicyError::ForbiddenConfigureFlag(forbidden.into()));
            }
        }
        for required in BASELINE_FLAGS {
            if !values.contains(required) {
                return Err(FfmpegPolicyError::MissingConfigureFlag((*required).into()));
            }
        }
        for prefix in ["--enable-gpl=", "--enable-nonfree="] {
            if let Some(flag) = values.iter().find(|flag| flag.starts_with(prefix)) {
                return Err(FfmpegPolicyError::ForbiddenConfigureFlag((*flag).into()));
            }
        }
        Ok(())
    }
}

fn validate_executables(root: &Path) -> Result<(), FfmpegPolicyError> {
    let root = canonical_directory(root)?;
    for program in ["ffmpeg", "ffprobe"] {
        let executable = executable_path(&root, program)?;
        let output = run_checked(&executable, "-version")?;
        let stdout = String::from_utf8_lossy(&output);
        let expected = format!("{program} version {FFMPEG_VERSION}");
        let suffix = stdout
            .lines()
            .next()
            .map(str::trim_start)
            .and_then(|line| line.strip_prefix(&expected));
        let valid = match suffix {
            Some("") => true,
            Some(suffix) => {
                suffix.starts_with(" Copyright (c) ") && suffix.ends_with(" the FFmpeg developers")
            }
            None => false,
        };
        if !valid {
            return Err(FfmpegPolicyError::ExecutableCheck(format!(
                "{program} did not report exact source version {FFMPEG_VERSION}"
            )));
        }
    }
    let buildconf = String::from_utf8_lossy(&run_checked(
        &executable_path(&root, "ffmpeg")?,
        "-buildconf",
    )?)
    .into_owned();
    FfmpegBundle::validate_buildconf(&buildconf)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LockFile {
    ffmpeg: LockedFfmpeg,
    libvpx: LockedDependency,
    opus: LockedDependency,
    nv_codec_headers: LockedDependency,
    target: Vec<LockedTarget>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LockedFfmpeg {
    version: String,
    source_url: String,
    signature_url: String,
    sha256: String,
    release_key_fingerprint: String,
    release_tag: String,
    release_commit: String,
    source_date_epoch: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LockedDependency {
    tag: String,
    peeled_commit: String,
    source_url: String,
    license: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LockedTarget {
    id: String,
    triple: String,
    builder_image: String,
    builder_os: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SignatureAttestation {
    schema_version: u8,
    verified: bool,
    signer_fingerprint: String,
    primary_fingerprint: String,
    sha256: String,
}

fn invalid_lock(message: impl Into<String>) -> FfmpegPolicyError {
    FfmpegPolicyError::InvalidBuildconf(format!("invalid source lock: {}", message.into()))
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (byte as char).is_ascii_lowercase())
}

#[allow(clippy::too_many_lines)] // A single fail-closed schema boundary makes all immutable fields auditable together.
fn parse_lock(value: &str) -> Result<LockFile, FfmpegPolicyError> {
    let lock: LockFile = toml::from_str(value).map_err(|error| invalid_lock(error.to_string()))?;
    let ffmpeg = &lock.ffmpeg;
    if ffmpeg.version != FFMPEG_VERSION
        || ffmpeg.release_tag != "n8.1.2"
        || !is_lower_hex(&ffmpeg.sha256, 64)
        || !is_lower_hex(&ffmpeg.release_commit, 40)
        || ffmpeg.release_key_fingerprint.len() != 40
        || !ffmpeg
            .release_key_fingerprint
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (byte as char).is_ascii_uppercase())
        || ffmpeg.source_date_epoch == 0
    {
        return Err(invalid_lock("FFmpeg value has invalid syntax"));
    }
    for (url, host, suffix) in [
        (&ffmpeg.source_url, "ffmpeg.org", "ffmpeg-8.1.2.tar.xz"),
        (
            &ffmpeg.signature_url,
            "ffmpeg.org",
            "ffmpeg-8.1.2.tar.xz.asc",
        ),
    ] {
        let parsed = Url::parse(url).map_err(|_| invalid_lock("invalid upstream URL"))?;
        if parsed.scheme() != "https"
            || parsed.host_str() != Some(host)
            || !parsed.path().ends_with(suffix)
        {
            return Err(invalid_lock("unexpected FFmpeg upstream URL"));
        }
    }
    for (dependency, tag, commit, host, path, license) in [
        (
            &lock.libvpx,
            "v1.16.0",
            "1024874c5919305883187e2953de8fcb4c3d7fa6",
            "github.com",
            "/webmproject/libvpx.git",
            "BSD-3-Clause",
        ),
        (
            &lock.opus,
            "v1.6.1",
            "22244de5a79bd1d6d623c32e72bf1954b56235be",
            "github.com",
            "/xiph/opus.git",
            "BSD-3-Clause",
        ),
        (
            &lock.nv_codec_headers,
            "n13.0.19.0",
            "e844e5b26f46bb77479f063029595293aa8f812d",
            "github.com",
            "/FFmpeg/nv-codec-headers.git",
            "MIT",
        ),
    ] {
        let parsed = Url::parse(&dependency.source_url)
            .map_err(|_| invalid_lock("invalid dependency URL"))?;
        if dependency.tag != tag
            || dependency.peeled_commit != commit
            || dependency.license != license
            || !is_lower_hex(&dependency.peeled_commit, 40)
            || parsed.scheme() != "https"
            || parsed.host_str() != Some(host)
            || parsed.path() != path
        {
            return Err(invalid_lock(
                "dependency does not match immutable provenance",
            ));
        }
    }
    let expected = [
        (
            "macos-arm64-vt",
            "aarch64-apple-darwin",
            "macos-14",
            "macos",
        ),
        (
            "windows-x64-mf",
            "x86_64-pc-windows-msvc",
            "windows-2025",
            "windows",
        ),
        (
            "linux-x64-vaapi-wayland",
            "x86_64-unknown-linux-gnu",
            "ubuntu-24.04",
            "linux",
        ),
    ];
    if lock.target.len() != expected.len()
        || expected.iter().any(|(id, triple, image, os)| {
            !lock.target.iter().any(|target| {
                (
                    target.id.as_str(),
                    target.triple.as_str(),
                    target.builder_image.as_str(),
                    target.builder_os.as_str(),
                ) == (*id, *triple, *image, *os)
            })
        })
    {
        return Err(invalid_lock(
            "target matrix must be exactly the supported immutable targets",
        ));
    }
    Ok(lock)
}

fn validate_locked_source(root: &Path, trusted_lock: &str) -> Result<LockFile, FfmpegPolicyError> {
    let bundled = fs::read_to_string(root.join("provenance/ffmpeg.lock"))?;
    if bundled != trusted_lock {
        return Err(FfmpegPolicyError::InvalidBuildconf(
            "bundle lock differs from authenticated project lock".into(),
        ));
    }
    let lock = parse_lock(&bundled)?;
    let target = fs::read_to_string(root.join(".ovayra-target"))?;
    if !lock.target.iter().any(|entry| entry.id == target.trim()) {
        return Err(invalid_lock(
            "bundle target marker is not in immutable target matrix",
        ));
    }
    let actual = sha256_file(&root.join("provenance/ffmpeg-8.1.2.tar.xz"))?;
    if lock.ffmpeg.version != FFMPEG_VERSION || lock.ffmpeg.sha256 != actual {
        return Err(FfmpegPolicyError::ChecksumMismatch(
            "provenance/ffmpeg-8.1.2.tar.xz".into(),
        ));
    }
    if fs::metadata(root.join("provenance/ffmpeg-8.1.2.tar.xz.asc"))?.len() == 0 {
        return Err(FfmpegPolicyError::InvalidBuildconf(
            "empty FFmpeg detached signature".into(),
        ));
    }
    validate_tarball(root.join("provenance/ffmpeg-8.1.2.tar.xz"))?;
    let attestation: SignatureAttestation = serde_json::from_slice(&fs::read(
        root.join("provenance/ffmpeg-signature-attestation.json"),
    )?)
    .map_err(|error| {
        FfmpegPolicyError::InvalidBuildconf(format!("invalid signature attestation: {error}"))
    })?;
    if attestation.schema_version != 1
        || !attestation.verified
        || attestation.signer_fingerprint != lock.ffmpeg.release_key_fingerprint
        || attestation.primary_fingerprint != lock.ffmpeg.release_key_fingerprint
        || attestation.sha256 != actual
    {
        return Err(FfmpegPolicyError::InvalidBuildconf(
            "signature attestation does not match lock".into(),
        ));
    }
    Ok(lock)
}

fn validate_tarball(path: PathBuf) -> Result<(), FfmpegPolicyError> {
    let file = fs::File::open(path)?;
    let mut archive = tar::Archive::new(xz2::read::XzDecoder::new(file));
    let mut entries = 0_u64;
    for entry in archive
        .entries()
        .map_err(|error| invalid_lock(format!("invalid tar.xz: {error}")))?
    {
        let entry =
            entry.map_err(|error| invalid_lock(format!("invalid tar.xz entry: {error}")))?;
        let path = entry
            .path()
            .map_err(|error| invalid_lock(format!("invalid tar.xz path: {error}")))?;
        let mut components = path.components();
        if components.next() != Some(PathComponent::Normal("ffmpeg-8.1.2".as_ref()))
            || components.any(|component| !matches!(component, PathComponent::Normal(_)))
        {
            return Err(invalid_lock(
                "FFmpeg source archive contains an unsafe or unexpected top-level path",
            ));
        }
        entries += 1;
    }
    if entries == 0 {
        return Err(invalid_lock("FFmpeg source archive is empty"));
    }
    Ok(())
}

fn canonical_directory(root: &Path) -> Result<PathBuf, FfmpegPolicyError> {
    let canonical = fs::canonicalize(root)
        .map_err(|_| FfmpegPolicyError::MissingArtifact(root.display().to_string()))?;
    if !canonical.is_dir() {
        return Err(FfmpegPolicyError::UnsafePath(root.display().to_string()));
    }
    Ok(canonical)
}

fn require_regular(root: &Path, path: &Path, label: &str) -> Result<(), FfmpegPolicyError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| FfmpegPolicyError::MissingArtifact(label.to_owned()))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(FfmpegPolicyError::UnsafePath(label.to_owned()));
    }
    let canonical = fs::canonicalize(path)?;
    if !canonical.starts_with(root) {
        return Err(FfmpegPolicyError::UnsafePath(label.to_owned()));
    }
    Ok(())
}

fn required_artifacts(root: &Path) -> Result<Vec<String>, FfmpegPolicyError> {
    let mut artifacts = REQUIRED_ARTIFACTS
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<Vec<_>>();
    let target = fs::read_to_string(root.join(".ovayra-target"))
        .map_err(|_| FfmpegPolicyError::MissingArtifact(".ovayra-target".into()))?;
    match target.trim() {
        "macos-arm64-vt" => {}
        "windows-x64-mf"
        | "windows-x64-nvidia"
        | "linux-x64-vaapi-wayland"
        | "linux-x64-vaapi-x11"
        | "linux-x64-nvidia" => {
            artifacts.extend(
                [
                    "provenance/nv-codec-headers-source.tar.zst",
                    "LICENSES/nv-codec-headers-MIT.txt",
                ]
                .into_iter()
                .map(str::to_owned),
            );
        }
        other => {
            return Err(FfmpegPolicyError::UnsafePath(format!(
                "unsupported target marker {other}"
            )));
        }
    }
    if target.trim().starts_with("windows-") {
        for program in ["ffmpeg", "ffprobe"] {
            let regular = format!("bin/{program}");
            let position = artifacts
                .iter()
                .position(|artifact| artifact == &regular)
                .expect("fixed required artifact exists");
            artifacts[position] = format!("bin/{program}.exe");
        }
    }
    Ok(artifacts)
}

fn collect_bundle_files(root: &Path) -> Result<BTreeSet<String>, FfmpegPolicyError> {
    let mut stack = vec![root.to_owned()];
    let mut files = BTreeSet::new();
    while let Some(directory) = stack.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)?;
            let relative = path
                .strip_prefix(root)
                .map_err(|_| FfmpegPolicyError::UnsafePath(path.display().to_string()))?
                .to_string_lossy()
                .replace('\\', "/");
            if metadata.file_type().is_symlink() {
                return Err(FfmpegPolicyError::UnsafePath(relative));
            }
            if metadata.is_dir() {
                stack.push(path);
            } else if metadata.is_file() {
                files.insert(relative);
            } else {
                return Err(FfmpegPolicyError::UnsafePath(relative));
            }
        }
    }
    Ok(files)
}

fn validate_checksums(
    root: &Path,
    required: &[String],
    files: &BTreeSet<String>,
) -> Result<(), FfmpegPolicyError> {
    let manifest = fs::read_to_string(root.join("provenance/SHA256SUMS"))?;
    let mut sums = BTreeMap::new();
    for line in manifest.lines() {
        let (hex, relative) = line.split_once("  ").ok_or_else(|| {
            FfmpegPolicyError::InvalidChecksumManifest(
                "entries require exactly two-space separator".into(),
            )
        })?;
        if hex.len() != 64
            || !hex.bytes().all(|byte| byte.is_ascii_hexdigit())
            || relative.is_empty()
            || relative.starts_with('/')
            || relative.contains('\\')
            || relative
                .split('/')
                .any(|part| part == ".." || part.is_empty())
        {
            return Err(FfmpegPolicyError::InvalidChecksumManifest(line.into()));
        }
        if relative == "provenance/SHA256SUMS"
            || sums.insert(relative, hex.to_ascii_lowercase()).is_some()
        {
            return Err(FfmpegPolicyError::InvalidChecksumManifest(relative.into()));
        }
    }
    for required in required
        .iter()
        .filter(|path| path.as_str() != "provenance/SHA256SUMS")
    {
        if !sums.contains_key(required.as_str()) {
            return Err(FfmpegPolicyError::InvalidChecksumManifest(format!(
                "missing {required}"
            )));
        }
    }
    for file in files
        .iter()
        .filter(|path| path.as_str() != "provenance/SHA256SUMS")
    {
        let expected = sums
            .remove(file.as_str())
            .ok_or_else(|| FfmpegPolicyError::InvalidChecksumManifest(format!("missing {file}")))?;
        let actual = sha256_file(&root.join(file))?;
        if actual != expected {
            return Err(FfmpegPolicyError::ChecksumMismatch(file.clone()));
        }
    }
    if let Some((unexpected, _)) = sums.into_iter().next() {
        return Err(FfmpegPolicyError::InvalidChecksumManifest(format!(
            "unexpected {unexpected}"
        )));
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, FfmpegPolicyError> {
    let mut file = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(hex::encode(hash.finalize()))
}

fn parse_tokens(input: &str) -> Result<Vec<String>, FfmpegPolicyError> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    for character in input.chars() {
        match (quote, character) {
            (Some(delimiter), character) if character == delimiter => quote = None,
            (None, '\'' | '\"') if current.is_empty() => quote = Some(character),
            (None, character) if character.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            (Some(_) | None, character) => current.push(character),
        }
    }
    if quote.is_some() || current.contains('\'') || current.contains('\"') {
        return Err(FfmpegPolicyError::InvalidBuildconf(
            "unbalanced or embedded quoting".into(),
        ));
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    if tokens.is_empty() {
        return Err(FfmpegPolicyError::InvalidBuildconf(
            "empty configuration".into(),
        ));
    }
    Ok(tokens)
}

#[derive(Deserialize)]
struct Bom {
    components: Vec<Component>,
}
#[derive(Deserialize)]
struct Component {
    name: String,
    version: String,
    hashes: Option<Vec<Hash>>,
    licenses: Option<Vec<LicenseChoice>>,
}
#[derive(Deserialize)]
struct Hash {
    alg: String,
    content: String,
}
#[derive(Deserialize)]
struct LicenseChoice {
    license: Option<License>,
}
#[derive(Deserialize)]
struct License {
    id: Option<String>,
}

fn validate_sbom(root: &Path, lock: &LockFile) -> Result<(), FfmpegPolicyError> {
    let bom: Bom = serde_json::from_slice(&fs::read(root.join("sbom/ffmpeg.cdx.json"))?)
        .map_err(|error| FfmpegPolicyError::InvalidSbom(error.to_string()))?;
    for (name, version, license, archive) in [
        (
            "FFmpeg",
            FFMPEG_VERSION,
            "LGPL-2.1-or-later",
            "provenance/ffmpeg-8.1.2.tar.xz",
        ),
        (
            "libvpx",
            lock.libvpx.tag.trim_start_matches('v'),
            lock.libvpx.license.as_str(),
            "provenance/libvpx-source.tar.zst",
        ),
        (
            "opus",
            lock.opus.tag.trim_start_matches('v'),
            lock.opus.license.as_str(),
            "provenance/opus-source.tar.zst",
        ),
    ] {
        let component = bom
            .components
            .iter()
            .find(|component| component.name == name && component.version == version)
            .ok_or_else(|| FfmpegPolicyError::InvalidSbom(format!("missing {name}@{version}")))?;
        let archive_hash = sha256_file(&root.join(archive))?;
        let hash_ok = component.hashes.as_ref().is_some_and(|hashes| {
            hashes.iter().any(|hash| {
                hash.alg.eq_ignore_ascii_case("SHA-256") && hash.content == archive_hash
            })
        });
        let license_ok = component.licenses.as_ref().is_some_and(|licenses| {
            licenses.iter().any(|choice| {
                choice
                    .license
                    .as_ref()
                    .and_then(|license_value| license_value.id.as_deref())
                    == Some(license)
            })
        });
        if !hash_ok || !license_ok {
            return Err(FfmpegPolicyError::InvalidSbom(format!(
                "{name} lacks required hash or license evidence"
            )));
        }
    }
    Ok(())
}

fn executable_path(root: &Path, program: &str) -> Result<PathBuf, FfmpegPolicyError> {
    let file = if root.join(".ovayra-target").exists()
        && fs::read_to_string(root.join(".ovayra-target"))?
            .trim()
            .starts_with("windows-")
    {
        format!("{program}.exe")
    } else {
        program.into()
    };
    let path = root.join("bin").join(&file);
    require_regular(root, &path, &format!("bin/{file}"))?;
    Ok(fs::canonicalize(path)?)
}

fn run_checked(executable: &Path, argument: &str) -> Result<Vec<u8>, FfmpegPolicyError> {
    let mut child = Command::new(executable)
        .arg(argument)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| FfmpegPolicyError::ExecutableCheck(error.to_string()))?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while child.try_wait().map_err(FfmpegPolicyError::Io)?.is_none() {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(FfmpegPolicyError::ExecutableCheck(format!(
                "{argument} timed out"
            )));
        }
        thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().map_err(FfmpegPolicyError::Io)?;
    if !output.status.success() {
        return Err(FfmpegPolicyError::ExecutableCheck(format!(
            "{argument} returned non-zero"
        )));
    }
    let mut all = output.stdout;
    all.extend(output.stderr);
    Ok(all)
}
