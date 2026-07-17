//! Strict parsing and verification for Phase 0 update manifests.

use std::collections::{BTreeMap, BTreeSet};

use minisign_verify::{PublicKey, Signature};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use time::format_description::well_known::Rfc3339;
use url::Url;

const MAX_MANIFEST_BYTES: usize = 64 * 1024;
const MAX_NOTES_BYTES: usize = 4 * 1024;
const MAX_SIGNATURE_BYTES: usize = 8 * 1024;
const MAX_URL_BYTES: usize = 1024;
const SUPPORTED_TARGETS: [(&str, UpdateFormat); 3] = [
    ("darwin-aarch64", UpdateFormat::App),
    ("windows-x86_64", UpdateFormat::Wix),
    ("linux-x86_64", UpdateFormat::Appimage),
];

#[derive(Debug, Error)]
pub enum ReleaseManifestError {
    #[error("update manifest is too large")]
    TooLarge,
    #[error("update manifest is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("update manifest policy violation: {0}")]
    Policy(String),
    #[error("minisign verification failed: {0}")]
    Minisign(#[from] minisign_verify::Error),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateFormat {
    App,
    Wix,
    Appimage,
}

impl PartialEq for UpdateFormat {
    fn eq(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (Self::App, Self::App) | (Self::Wix, Self::Wix) | (Self::Appimage, Self::Appimage)
        )
    }
}

impl Eq for UpdateFormat {}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformRelease {
    pub(crate) url: Url,
    pub(crate) signature: String,
    pub(crate) format: UpdateFormat,
    pub(crate) sha256: String,
}

impl PlatformRelease {
    #[must_use]
    pub fn url(&self) -> &Url {
        &self.url
    }
    #[must_use]
    pub fn signature(&self) -> &str {
        &self.signature
    }
    #[must_use]
    pub fn format(&self) -> &UpdateFormat {
        &self.format
    }
    #[must_use]
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseManifest {
    version: Version,
    pub_date: String,
    notes: String,
    platforms: BTreeMap<String, PlatformRelease>,
}

impl ReleaseManifest {
    /// Parses a bounded manifest and proves it advances the supplied stable version.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed JSON, unknown fields, unsupported update targets, an
    /// invalid download policy, or an equal/downgrade/prerelease version.
    pub fn parse_for_installed(
        input: &str,
        installed: &Version,
    ) -> Result<Self, ReleaseManifestError> {
        if input.len() > MAX_MANIFEST_BYTES {
            return Err(ReleaseManifestError::TooLarge);
        }
        let manifest: Self = serde_json::from_str(input)?;
        manifest.validate_structure()?;
        if !manifest.version.pre.is_empty()
            || !installed.pre.is_empty()
            || manifest.version <= *installed
        {
            return Err(policy("version must be a stable upgrade"));
        }
        Ok(manifest)
    }

    #[must_use]
    pub fn version(&self) -> &Version {
        &self.version
    }
    #[must_use]
    pub fn platform_count(&self) -> usize {
        self.platforms.len()
    }
    #[must_use]
    pub fn platforms(&self) -> &BTreeMap<String, PlatformRelease> {
        &self.platforms
    }

    pub(crate) fn from_release_parts(
        version: Version,
        pub_date: String,
        notes: String,
        platforms: BTreeMap<String, PlatformRelease>,
    ) -> Result<Self, ReleaseManifestError> {
        let manifest = Self {
            version,
            pub_date,
            notes,
            platforms,
        };
        manifest.validate_structure()?;
        if !manifest.version.pre.is_empty() {
            return Err(policy("release version must be stable"));
        }
        Ok(manifest)
    }

    fn validate_structure(&self) -> Result<(), ReleaseManifestError> {
        time::OffsetDateTime::parse(&self.pub_date, &Rfc3339)
            .map_err(|_| policy("pub_date must be RFC3339"))?;
        if self.notes.is_empty() || self.notes.len() > MAX_NOTES_BYTES {
            return Err(policy("notes are empty or too large"));
        }
        if self.platforms.len() != SUPPORTED_TARGETS.len() {
            return Err(policy("platform target count is not exact"));
        }

        let mut urls = BTreeSet::new();
        for (target, expected_format) in SUPPORTED_TARGETS {
            let platform = self
                .platforms
                .get(target)
                .ok_or_else(|| policy("missing supported target"))?;
            if platform.format != expected_format {
                return Err(policy("target has an unsupported format"));
            }
            validate_platform(platform)?;
            let canonical = platform.url.as_str();
            if !urls.insert(canonical) {
                return Err(policy("duplicate canonical update URL"));
            }
        }
        if self.platforms.keys().any(|target| {
            !SUPPORTED_TARGETS
                .iter()
                .any(|(allowed, _)| target == allowed)
        }) {
            return Err(policy("unsupported update target"));
        }
        Ok(())
    }
}

/// A verifier constructed from an official two-line Minisign public-key document.
pub struct ReleaseVerifier(PublicKey);

impl ReleaseVerifier {
    /// # Errors
    ///
    /// Fails closed if the supplied document is not a Minisign public key.
    pub fn new(public_key: &str) -> Result<Self, ReleaseManifestError> {
        Ok(Self(PublicKey::decode(public_key)?))
    }

    /// # Errors
    ///
    /// Fails closed for malformed signatures, key-ID mismatch, unsupported legacy signatures,
    /// and every payload/signature verification failure.
    pub fn verify(&self, package: &[u8], signature: &str) -> Result<(), ReleaseManifestError> {
        let signature = Signature::decode(signature)?;
        self.0.verify(package, &signature, false)?;
        Ok(())
    }

    /// Verifies the manifest's exact byte length, SHA-256, and detached signature.
    ///
    /// # Errors
    ///
    /// Fails before signature verification for a length or checksum mismatch.
    pub fn verify_expected(
        &self,
        package: &[u8],
        signature: &str,
        expected_length: u64,
        expected_sha256: &str,
    ) -> Result<(), ReleaseManifestError> {
        if u64::try_from(package.len()).map_err(|_| policy("package length overflow"))?
            != expected_length
        {
            return Err(policy("package length mismatch"));
        }
        let actual = hex::encode(Sha256::digest(package));
        if actual != expected_sha256 {
            return Err(policy("package SHA-256 mismatch"));
        }
        self.verify(package, signature)
    }
}

fn validate_platform(platform: &PlatformRelease) -> Result<(), ReleaseManifestError> {
    if platform.url.as_str().len() > MAX_URL_BYTES
        || platform.signature.is_empty()
        || platform.signature.len() > MAX_SIGNATURE_BYTES
    {
        return Err(policy("platform field is empty or too large"));
    }
    let url = &platform.url;
    if url.scheme() != "https"
        || url.host_str() != Some("updates.ovayra.com")
        || url.port_or_known_default() != Some(443)
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.path().starts_with("/phase-0/")
        || url.path().contains("//")
        || url.path().contains("..")
    {
        return Err(policy("update URL is outside the canonical HTTPS origin"));
    }
    if !is_lower_hex(&platform.sha256, 64) {
        return Err(policy("SHA-256 is not lowercase hexadecimal"));
    }
    Ok(())
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (byte as char).is_ascii_lowercase())
}

fn policy(message: impl Into<String>) -> ReleaseManifestError {
    ReleaseManifestError::Policy(message.into())
}
