//! Filesystem-facing package discovery and update-manifest verification.

use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use semver::Version;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use thiserror::Error;
use url::Url;

use crate::{
    PlatformRelease, ReleaseManifest, ReleaseManifestError, ReleaseVerifier, UpdateFormat,
};

const MAX_DISCOVERED_FILES: usize = 64;
const MAX_PACKAGE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_SIGNATURE_BYTES: u64 = 8 * 1024;

#[derive(Debug, Error)]
pub enum PackageError {
    #[error("package I/O failure: {0}")]
    Io(#[from] std::io::Error),
    #[error("package manifest failure: {0}")]
    Manifest(#[from] ReleaseManifestError),
    #[error("package policy violation: {0}")]
    Policy(String),
}

/// Package operations deliberately discover only direct regular files in a clean package output
/// directory. Installer downloads and in-place updater targets are separate documents.
pub struct PackageRelease;

impl PackageRelease {
    /// Generates `latest.json` atomically and writes `.deb`/`.dmg` download metadata separately.
    ///
    /// # Errors
    ///
    /// Fails closed for symlinks, ambiguous names, unsigned targets, unrecognised package files,
    /// invalid base URLs, or malformed package metadata.
    pub fn generate_manifest(
        packages: &Path,
        base_url: &str,
        output: &Path,
        version: &Version,
        pub_date: &str,
        notes: &str,
    ) -> Result<(), PackageError> {
        let base = canonical_base_url(base_url)?;
        let mut platforms = BTreeMap::new();
        let mut downloads = BTreeMap::new();
        for entry in safe_direct_files(packages)? {
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| policy("package name is not UTF-8"))?;
            if name.ends_with(".minisig")
                || Path::new(&name)
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("sig"))
            {
                continue;
            }
            match artifact_kind(&name) {
                Some(ArtifactKind::Update { target, format }) => {
                    if platforms.contains_key(target) {
                        return Err(policy("duplicate updater target"));
                    }
                    let bytes = read_regular_bounded(&entry.path(), MAX_PACKAGE_BYTES)?;
                    let signature = read_signature_for(&entry.path(), &name)?;
                    let url = base
                        .join(&name)
                        .map_err(|_| policy("package name cannot form update URL"))?;
                    platforms.insert(
                        target.to_owned(),
                        PlatformRelease {
                            url,
                            signature,
                            format,
                            sha256: hex::encode(Sha256::digest(bytes)),
                        },
                    );
                }
                Some(ArtifactKind::Download) => {
                    let bytes = read_regular_bounded(&entry.path(), MAX_PACKAGE_BYTES)?;
                    let url = base
                        .join(&name)
                        .map_err(|_| policy("package name cannot form download URL"))?;
                    downloads.insert(
                        name,
                        DownloadArtifact {
                            url: url.to_string(),
                            sha256: hex::encode(Sha256::digest(bytes)),
                        },
                    );
                }
                None => return Err(policy("unrecognised package artifact")),
            }
        }
        let manifest = ReleaseManifest::from_release_parts(
            version.clone(),
            pub_date.to_owned(),
            notes.to_owned(),
            platforms,
        )?;
        write_json_atomic(output, &manifest)?;
        let downloads_output = output.with_file_name("downloads.json");
        write_json_atomic(
            &downloads_output,
            &Downloads {
                version: version.to_string(),
                artifacts: downloads,
            },
        )
    }

    /// Verifies every updater payload referenced by a manifest against its exact local file,
    /// SHA-256, detached signature sidecar, and Minisign public key.
    ///
    /// # Errors
    ///
    /// Returns an error for a malformed manifest, unsafe package tree, absent or substituted
    /// sidecar, digest mismatch, or any Minisign verification failure.
    pub fn verify_manifest(
        manifest: &Path,
        packages: &Path,
        public_key: &str,
        installed: &Version,
    ) -> Result<(), PackageError> {
        let parsed = ReleaseManifest::parse_for_installed(
            &read_text_bounded(manifest, 64 * 1024)?,
            installed,
        )?;
        let verifier = ReleaseVerifier::new(public_key)?;
        for platform in parsed.platforms().values() {
            let name = local_name(platform.url())?;
            let path = packages.join(name);
            let package = read_regular_bounded(&path, MAX_PACKAGE_BYTES)?;
            let signature = read_signature_for(&path, name)?;
            if signature != platform.signature() {
                return Err(policy("detached signature does not match manifest"));
            }
            verifier.verify_expected(
                &package,
                &signature,
                package.len() as u64,
                platform.sha256(),
            )?;
        }
        Ok(())
    }

    /// Copies package files into a private temporary directory, changes one byte there, and proves
    /// verification fails while the original package tree remains untouched.
    ///
    /// # Errors
    ///
    /// Returns an error if the source manifest does not verify, copying cannot preserve a regular
    /// package tree, or the altered temporary payload unexpectedly verifies.
    pub fn verify_tamper_rejection(
        manifest: &Path,
        packages: &Path,
        public_key: &str,
        installed: &Version,
    ) -> Result<(), PackageError> {
        Self::verify_manifest(manifest, packages, public_key, installed)?;
        let copied = TempDir::new()?;
        for entry in safe_direct_files(packages)? {
            fs::copy(entry.path(), copied.path().join(entry.file_name()))?;
        }
        let parsed = ReleaseManifest::parse_for_installed(
            &read_text_bounded(manifest, 64 * 1024)?,
            installed,
        )?;
        let first = parsed
            .platforms()
            .values()
            .next()
            .ok_or_else(|| policy("manifest contains no updater targets"))?;
        let path = copied.path().join(local_name(first.url())?);
        let mut bytes = read_regular_bounded(&path, MAX_PACKAGE_BYTES)?;
        let byte = bytes
            .first_mut()
            .ok_or_else(|| policy("cannot corrupt empty package"))?;
        *byte ^= 1;
        write_regular(&path, &bytes)?;
        if Self::verify_manifest(manifest, copied.path(), public_key, installed).is_ok() {
            return Err(policy("tampered package unexpectedly verified"));
        }
        Ok(())
    }
}

enum ArtifactKind {
    Update {
        target: &'static str,
        format: UpdateFormat,
    },
    Download,
}

fn artifact_kind(name: &str) -> Option<ArtifactKind> {
    if normalized_name(name, "darwin-aarch64.app.tar.gz") {
        Some(ArtifactKind::Update {
            target: "darwin-aarch64",
            format: UpdateFormat::App,
        })
    } else if normalized_name(name, "windows-x86_64.msi") {
        Some(ArtifactKind::Update {
            target: "windows-x86_64",
            format: UpdateFormat::Wix,
        })
    } else if normalized_name(name, "linux-x86_64.AppImage") {
        Some(ArtifactKind::Update {
            target: "linux-x86_64",
            format: UpdateFormat::Appimage,
        })
    } else if normalized_name(name, "darwin-aarch64.dmg")
        || normalized_name(name, "linux-x86_64.deb")
    {
        Some(ArtifactKind::Download)
    } else {
        None
    }
}

fn normalized_name(name: &str, suffix: &str) -> bool {
    name.starts_with("ovayra-phase-0_")
        && name.ends_with(suffix)
        && name
            .strip_prefix("ovayra-phase-0_")
            .and_then(|value| value.split_once('_'))
            .is_some_and(|(version, target)| Version::parse(version).is_ok() && target == suffix)
}

fn canonical_base_url(value: &str) -> Result<Url, PackageError> {
    let url = Url::parse(value).map_err(|_| policy("invalid update base URL"))?;
    if url.scheme() != "https"
        || url.host_str() != Some("updates.ovayra.com")
        || url.port_or_known_default() != Some(443)
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.path().starts_with("/phase-0/")
        || !url.path().ends_with('/')
    {
        return Err(policy("base URL is outside the canonical update origin"));
    }
    Ok(url)
}

fn safe_direct_files(root: &Path) -> Result<Vec<fs::DirEntry>, PackageError> {
    let metadata = fs::symlink_metadata(root)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(policy("package root is not a real directory"));
    }
    let mut entries: Vec<_> = fs::read_dir(root)?.collect::<Result<_, _>>()?;
    if entries.len() > MAX_DISCOVERED_FILES {
        return Err(policy("too many package files"));
    }
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in &entries {
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(policy("package output contains a non-regular file"));
        }
    }
    Ok(entries)
}

fn read_regular_bounded(path: &Path, max: u64) -> Result<Vec<u8>, PackageError> {
    let before = fs::symlink_metadata(path)?;
    if before.file_type().is_symlink() || !before.is_file() || before.len() > max {
        return Err(policy("package file is unsafe or too large"));
    }
    let mut file = File::open(path)?;
    let mut bytes = Vec::with_capacity(usize::try_from(before.len()).unwrap_or(0));
    file.read_to_end(&mut bytes)?;
    let after = file.metadata()?;
    if after.len() != before.len() || bytes.len() as u64 != before.len() {
        return Err(policy("package file changed while being read"));
    }
    Ok(bytes)
}

fn read_signature(path: &Path) -> Result<String, PackageError> {
    let bytes = read_regular_bounded(path, MAX_SIGNATURE_BYTES)?;
    let signature = String::from_utf8(bytes).map_err(|_| policy("signature is not UTF-8"))?;
    if signature.is_empty() {
        return Err(policy("signature is empty"));
    }
    Ok(signature)
}

fn read_signature_for(package: &Path, name: &str) -> Result<String, PackageError> {
    let candidates = [
        package.with_file_name(format!("{name}.minisig")),
        package.with_file_name(format!("{name}.sig")),
    ];
    let present: Vec<_> = candidates.iter().filter(|path| path.exists()).collect();
    if present.len() != 1 {
        return Err(policy(
            "package must have exactly one detached signature sidecar",
        ));
    }
    let signature = read_signature(present[0])?;
    if present[0]
        .extension()
        .is_some_and(|extension| extension == "sig")
    {
        let decoded = STANDARD
            .decode(signature.trim())
            .map_err(|_| policy("cargo-packager signature is not a single base64 envelope"))?;
        let raw = String::from_utf8(decoded)
            .map_err(|_| policy("cargo-packager signature is not UTF-8"))?;
        if raw.lines().count() != 4
            || raw.lines().any(str::is_empty)
            || STANDARD.decode(raw.trim()).is_ok()
        {
            return Err(policy(
                "cargo-packager signature is malformed or double encoded",
            ));
        }
        return Ok(raw);
    }
    Ok(signature)
}

fn read_text_bounded(path: &Path, max: u64) -> Result<String, PackageError> {
    String::from_utf8(read_regular_bounded(path, max)?)
        .map_err(|_| policy("text file is not UTF-8"))
}

fn local_name(url: &Url) -> Result<&str, PackageError> {
    let name = url
        .path_segments()
        .and_then(Iterator::last)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| policy("update URL lacks a package name"))?;
    if name.contains('/') || name.contains('\\') || name == "." || name == ".." {
        return Err(policy("unsafe update package name"));
    }
    Ok(name)
}

fn write_regular(path: &Path, bytes: &[u8]) -> Result<(), PackageError> {
    let mut file = OpenOptions::new().write(true).truncate(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<(), PackageError> {
    let parent = path
        .parent()
        .ok_or_else(|| policy("manifest output has no parent"))?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension("tmp");
    let json = serde_json::to_vec_pretty(value).map_err(ReleaseManifestError::from)?;
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&json)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
    }
    fs::rename(temporary, path)?;
    Ok(())
}

#[derive(Serialize)]
struct DownloadArtifact {
    url: String,
    sha256: String,
}
#[derive(Serialize)]
struct Downloads {
    version: String,
    artifacts: BTreeMap<String, DownloadArtifact>,
}

fn policy(message: impl Into<String>) -> PackageError {
    PackageError::Policy(message.into())
}
