//! Fail-closed verification for a redistributable `FFmpeg` bundle.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

const FFMPEG_VERSION: &str = "8.1.2";
const REQUIRED_ARTIFACTS: &[&str] = &[
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
        let root = canonical_directory(root)?;
        for artifact in REQUIRED_ARTIFACTS {
            let path = root.join(artifact);
            require_regular(&root, &path, artifact)?;
        }
        validate_no_symlink_or_unlisted_files(&root)?;
        validate_checksums(&root)?;
        let buildconf = fs::read_to_string(root.join("provenance/buildconf.txt"))?;
        Self::validate_buildconf(&buildconf)?;
        validate_sbom(&root.join("sbom/ffmpeg.cdx.json"))
    }

    /// Runs only verified in-root executables with a bounded wall-clock deadline.
    ///
    /// # Errors
    ///
    /// Returns an error if layout validation fails, an executable is unsafe, times out, exits
    /// unsuccessfully, or reports a source/configuration that violates policy.
    pub fn validate(root: &Path) -> Result<(), FfmpegPolicyError> {
        Self::validate_layout(root)?;
        let root = canonical_directory(root)?;
        for program in ["ffmpeg", "ffprobe"] {
            let executable = executable_path(&root, program)?;
            let output = run_checked(&executable, "-version")?;
            let stdout = String::from_utf8_lossy(&output);
            if !stdout.contains(&format!("ffmpeg version {FFMPEG_VERSION}"))
                && !stdout.contains(&format!("ffprobe version {FFMPEG_VERSION}"))
            {
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
        Self::validate_buildconf(&buildconf)?;
        Ok(())
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

fn validate_no_symlink_or_unlisted_files(root: &Path) -> Result<(), FfmpegPolicyError> {
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
    let manifest = "provenance/SHA256SUMS";
    for file in files.iter().filter(|file| file.as_str() != manifest) {
        if !REQUIRED_ARTIFACTS.contains(&file.as_str()) {
            return Err(FfmpegPolicyError::InvalidChecksumManifest(format!(
                "unlisted regular file {file}"
            )));
        }
    }
    Ok(())
}

fn validate_checksums(root: &Path) -> Result<(), FfmpegPolicyError> {
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
    for required in REQUIRED_ARTIFACTS
        .iter()
        .filter(|path| **path != "provenance/SHA256SUMS")
    {
        let expected = sums.remove(*required).ok_or_else(|| {
            FfmpegPolicyError::InvalidChecksumManifest(format!("missing {required}"))
        })?;
        let actual = sha256_file(&root.join(required))?;
        if actual != expected {
            return Err(FfmpegPolicyError::ChecksumMismatch((*required).into()));
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

fn validate_sbom(path: &Path) -> Result<(), FfmpegPolicyError> {
    let bom: Bom = serde_json::from_slice(&fs::read(path)?)
        .map_err(|error| FfmpegPolicyError::InvalidSbom(error.to_string()))?;
    for (name, version, license) in [
        ("FFmpeg", FFMPEG_VERSION, "LGPL-2.1-or-later"),
        ("libvpx", "1.16.0", "BSD-3-Clause"),
        ("opus", "1.6.1", "BSD-3-Clause"),
    ] {
        let component = bom
            .components
            .iter()
            .find(|component| component.name == name && component.version == version)
            .ok_or_else(|| FfmpegPolicyError::InvalidSbom(format!("missing {name}@{version}")))?;
        let hash_ok = component.hashes.as_ref().is_some_and(|hashes| {
            hashes.iter().any(|hash| {
                hash.alg.eq_ignore_ascii_case("SHA-256")
                    && hash.content.len() == 64
                    && hash.content.bytes().all(|byte| byte.is_ascii_hexdigit())
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
    let file = if cfg!(windows) {
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
