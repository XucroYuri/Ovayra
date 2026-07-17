use std::{collections::BTreeSet, fs, path::Path};

use serde::Deserialize;
use thiserror::Error;

use crate::{SpikeId, TargetId, Verdict};

const CANONICAL_REQUIRED: &[(&str, &str, Option<&str>, Option<&str>)] = &[
    ("preview", "macos-arm64-vt", Some("aqua"), None),
    ("media", "macos-arm64-vt", None, Some("videotoolbox")),
    ("media", "macos-arm64-vt", None, Some("cpu-fallback")),
    ("platform", "macos-arm64-vt", Some("aqua"), None),
    ("gemini", "macos-arm64-vt", None, None),
    ("distribution", "macos-arm64-vt", None, None),
    ("preview", "windows-x64-mf", Some("windows"), None),
    ("media", "windows-x64-mf", None, Some("d3d11va-mf")),
    ("media", "windows-x64-mf", None, Some("cpu-fallback")),
    ("platform", "windows-x64-mf", Some("windows"), None),
    ("gemini", "windows-x64-mf", None, None),
    ("distribution", "windows-x64-mf", None, None),
    ("preview", "windows-x64-nvidia", Some("windows"), None),
    ("media", "windows-x64-nvidia", None, Some("nvenc-nvdec")),
    ("media", "windows-x64-nvidia", None, Some("cpu-fallback")),
    ("platform", "windows-x64-nvidia", Some("windows"), None),
    ("gemini", "windows-x64-nvidia", None, None),
    ("preview", "linux-x64-vaapi-wayland", Some("wayland"), None),
    ("media", "linux-x64-vaapi-wayland", None, Some("vaapi")),
    (
        "media",
        "linux-x64-vaapi-wayland",
        None,
        Some("cpu-fallback"),
    ),
    ("platform", "linux-x64-vaapi-wayland", Some("wayland"), None),
    ("gemini", "linux-x64-vaapi-wayland", None, None),
    ("distribution", "linux-x64-vaapi-wayland", None, None),
    ("preview", "linux-x64-vaapi-x11", Some("x11"), None),
    ("media", "linux-x64-vaapi-x11", None, Some("vaapi")),
    ("media", "linux-x64-vaapi-x11", None, Some("cpu-fallback")),
    ("platform", "linux-x64-vaapi-x11", Some("x11"), None),
    ("gemini", "linux-x64-vaapi-x11", None, None),
    ("preview", "linux-x64-nvidia", None, None),
    ("media", "linux-x64-nvidia", None, Some("nvenc-nvdec")),
    ("media", "linux-x64-nvidia", None, Some("cpu-fallback")),
    ("platform", "linux-x64-nvidia", None, None),
    ("gemini", "linux-x64-nvidia", None, None),
];

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequiredEvidence {
    pub id: SpikeId,
    pub target: TargetId,
    pub session: Option<String>,
    pub backend: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhaseZeroMatrix {
    pub required: Vec<RequiredEvidence>,
}

#[derive(Debug, Error)]
pub enum MatrixError {
    #[error(transparent)]
    Toml(#[from] toml::de::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("phase 0 evidence matrix must contain at least one required entry")]
    Empty,
    #[error("required evidence qualifiers must not be empty")]
    EmptyQualifier,
    #[error("duplicate required evidence entry")]
    Duplicate,
    #[error("matrix is missing required entries: {0:?}")]
    MissingRequiredEntries(Vec<String>),
    #[error("matrix contains unsupported required entries: {0:?}")]
    UnsupportedRequiredEntries(Vec<String>),
    #[error("required real-device evidence must pass, got {0:?}")]
    RequiredVerdict(Verdict),
    #[error("evidence entry is not required by this matrix")]
    MissingRequiredEvidence,
}

impl PhaseZeroMatrix {
    /// Parses and validates a Phase 0 evidence matrix.
    ///
    /// # Errors
    ///
    /// Returns an error when the TOML cannot be parsed, contains unsupported or
    /// duplicate entries, or has an empty required-evidence list.
    pub fn from_toml(input: &str) -> Result<Self, MatrixError> {
        let matrix: Self = toml::from_str(input)?;
        matrix.validate()?;
        Ok(matrix)
    }

    /// Loads and validates a Phase 0 evidence matrix from disk.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be read or its contents do not form
    /// a valid Phase 0 evidence matrix.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, MatrixError> {
        Self::from_toml(&fs::read_to_string(path)?)
    }

    /// Rejects a missing entry and every required outcome except [`Verdict::Pass`].
    ///
    /// # Errors
    ///
    /// Returns [`MatrixError::MissingRequiredEvidence`] when `entry` is absent,
    /// or [`MatrixError::RequiredVerdict`] for `Conditional`, `Fail`, or
    /// `Skipped` outcomes.
    pub fn validate_required_verdict(
        &self,
        entry: &RequiredEvidence,
        verdict: Verdict,
    ) -> Result<(), MatrixError> {
        if !self.required.contains(entry) {
            return Err(MatrixError::MissingRequiredEvidence);
        }
        if verdict == Verdict::Pass {
            Ok(())
        } else {
            Err(MatrixError::RequiredVerdict(verdict))
        }
    }

    fn validate(&self) -> Result<(), MatrixError> {
        if self.required.is_empty() {
            return Err(MatrixError::Empty);
        }

        let mut entries = BTreeSet::new();
        for evidence in &self.required {
            if evidence.session.as_deref().is_some_and(str::is_empty)
                || evidence.backend.as_deref().is_some_and(str::is_empty)
            {
                return Err(MatrixError::EmptyQualifier);
            }
            if !entries.insert((
                evidence.id,
                evidence.target.as_str(),
                evidence.session.as_deref(),
                evidence.backend.as_deref(),
            )) {
                return Err(MatrixError::Duplicate);
            }
        }

        let expected = CANONICAL_REQUIRED
            .iter()
            .map(|(id, target, session, backend)| matrix_key(id, target, *session, *backend))
            .collect::<BTreeSet<_>>();
        let actual = self
            .required
            .iter()
            .map(required_key)
            .collect::<BTreeSet<_>>();
        let missing = expected.difference(&actual).cloned().collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(MatrixError::MissingRequiredEntries(missing));
        }
        let unsupported = actual.difference(&expected).cloned().collect::<Vec<_>>();
        if !unsupported.is_empty() {
            return Err(MatrixError::UnsupportedRequiredEntries(unsupported));
        }
        Ok(())
    }
}

fn required_key(entry: &RequiredEvidence) -> String {
    matrix_key(
        spike_id_name(entry.id),
        entry.target.as_str(),
        entry.session.as_deref(),
        entry.backend.as_deref(),
    )
}

fn matrix_key(id: &str, target: &str, session: Option<&str>, backend: Option<&str>) -> String {
    format!(
        "{id}|{target}|{}|{}",
        session.unwrap_or(""),
        backend.unwrap_or("")
    )
}

const fn spike_id_name(id: SpikeId) -> &'static str {
    match id {
        SpikeId::Preview => "preview",
        SpikeId::Media => "media",
        SpikeId::Gemini => "gemini",
        SpikeId::Platform => "platform",
        SpikeId::Distribution => "distribution",
    }
}
