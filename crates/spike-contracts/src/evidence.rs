use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

const FORBIDDEN: &[&str] = &[
    "api_key",
    "token",
    "secret",
    "password",
    "upload_url",
    "prompt",
    "result",
    "media_path",
    "file_name",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpikeId {
    Preview,
    Media,
    Gemini,
    Platform,
    Distribution,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Pass,
    Conditional,
    Fail,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetId(String);

impl TargetId {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Evidence {
    pub schema_version: u32,
    pub spike: SpikeId,
    pub target: TargetId,
    pub verdict: Option<Verdict>,
    pub duration_ms: Option<u64>,
    pub measurements: BTreeMap<String, Value>,
    pub observations: Vec<String>,
}

#[derive(Debug, Error)]
pub enum EvidenceError {
    #[error("sensitive evidence field is forbidden: {0}")]
    SensitiveField(String),
    #[error("evidence has not been finished")]
    Unfinished,
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl Evidence {
    #[must_use]
    pub fn new(spike: SpikeId, target: TargetId) -> Self {
        Self {
            schema_version: 1,
            spike,
            target,
            verdict: None,
            duration_ms: None,
            measurements: BTreeMap::new(),
            observations: Vec::new(),
        }
    }

    /// # Errors
    ///
    /// Returns [`EvidenceError::SensitiveField`] when `name` contains a forbidden
    /// value, or [`EvidenceError::Json`] when `value` cannot be serialized.
    pub fn measure(&mut self, name: &str, value: impl Serialize) -> Result<(), EvidenceError> {
        let normalized = name.to_ascii_lowercase();
        if FORBIDDEN.iter().any(|part| normalized.contains(part)) {
            return Err(EvidenceError::SensitiveField(name.to_owned()));
        }
        self.measurements
            .insert(name.to_owned(), serde_json::to_value(value)?);
        Ok(())
    }

    pub fn finish(&mut self, verdict: Verdict, duration_ms: u64) {
        self.verdict = Some(verdict);
        self.duration_ms = Some(duration_ms);
    }

    /// # Errors
    ///
    /// Returns [`EvidenceError::Unfinished`] until both a verdict and duration
    /// have been supplied, or [`EvidenceError::Json`] if serialization fails.
    pub fn to_pretty_json(&self) -> Result<String, EvidenceError> {
        if self.verdict.is_none() || self.duration_ms.is_none() {
            return Err(EvidenceError::Unfinished);
        }
        Ok(serde_json::to_string_pretty(self)?)
    }
}
