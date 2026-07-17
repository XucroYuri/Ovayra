use std::{collections::BTreeSet, fs, path::Path};

use serde::Deserialize;
use thiserror::Error;

use crate::{SpikeId, TargetId, Verdict};

const SUPPORTED_TARGETS: &[&str] = &[
    "macos-arm64-vt",
    "windows-x64-mf",
    "windows-x64-nvidia",
    "linux-x64-vaapi-wayland",
    "linux-x64-vaapi-x11",
    "linux-x64-nvidia",
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
    #[error("unsupported phase 0 target: {0}")]
    UnsupportedTarget(String),
    #[error("required evidence qualifiers must not be empty")]
    EmptyQualifier,
    #[error("duplicate required evidence entry")]
    Duplicate,
    #[error("required real-device evidence must pass, got {0:?}")]
    RequiredVerdict(Verdict),
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

    /// Rejects every required real-device outcome except [`Verdict::Pass`].
    ///
    /// # Errors
    ///
    /// Returns [`MatrixError::RequiredVerdict`] for `Conditional`, `Fail`, or
    /// `Skipped` outcomes.
    pub fn validate_required_verdict(&self, verdict: Verdict) -> Result<(), MatrixError> {
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
            let target = evidence.target.as_str();
            if !SUPPORTED_TARGETS.contains(&target) {
                return Err(MatrixError::UnsupportedTarget(target.to_owned()));
            }
            if evidence.session.as_deref().is_some_and(str::is_empty)
                || evidence.backend.as_deref().is_some_and(str::is_empty)
            {
                return Err(MatrixError::EmptyQualifier);
            }
            if !entries.insert((
                evidence.id,
                target,
                evidence.session.as_deref(),
                evidence.backend.as_deref(),
            )) {
                return Err(MatrixError::Duplicate);
            }
        }
        Ok(())
    }
}
