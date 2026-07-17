use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
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

const SUPPORTED_TARGETS: &[&str] = &[
    "macos-arm64-vt",
    "windows-x64-mf",
    "windows-x64-nvidia",
    "linux-x64-vaapi-wayland",
    "linux-x64-vaapi-x11",
    "linux-x64-nvidia",
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetId(String);

#[derive(Debug, Error)]
pub enum TargetIdError {
    #[error("unsupported phase 0 target: {0}")]
    Unsupported(String),
}

impl TargetId {
    /// Creates a supported Phase 0 target ID.
    ///
    /// # Errors
    ///
    /// Returns an unsupported-target error unless the input is one of the six
    /// real-device targets in the approved Phase 0 matrix.
    pub fn new(value: impl Into<String>) -> Result<Self, TargetIdError> {
        let value = value.into();
        if SUPPORTED_TARGETS.contains(&value.as_str()) {
            Ok(Self(value))
        } else {
            Err(TargetIdError::Unsupported(value))
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for TargetId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for TargetId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// A finished, redacted evidence record.
///
/// ~~~compile_fail
/// let evidence: spike_contracts::Evidence = todo!();
/// serde_json::to_string(&evidence).unwrap();
/// ~~~
#[derive(Debug)]
pub struct Evidence {
    schema_version: u32,
    spike: SpikeId,
    target: TargetId,
    verdict: Option<Verdict>,
    duration_ms: Option<u64>,
    measurements: BTreeMap<String, Value>,
    observations: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceDocument {
    schema_version: u32,
    spike: SpikeId,
    target: TargetId,
    verdict: Option<Verdict>,
    duration_ms: Option<u64>,
    measurements: BTreeMap<String, Value>,
    observations: Vec<String>,
}

#[derive(Debug, Error)]
pub enum EvidenceError {
    #[error("sensitive evidence field is forbidden: {0}")]
    SensitiveField(String),
    #[error("sensitive evidence observation is forbidden")]
    SensitiveObservation(String),
    #[error("evidence has not been finished")]
    Unfinished,
    #[error("unsupported evidence schema version: {0}")]
    UnsupportedSchemaVersion(u32),
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

    #[must_use]
    pub fn spike(&self) -> SpikeId {
        self.spike
    }

    #[must_use]
    pub fn target(&self) -> &TargetId {
        &self.target
    }

    #[must_use]
    pub fn verdict(&self) -> Option<Verdict> {
        self.verdict
    }

    #[must_use]
    pub fn duration_ms(&self) -> Option<u64> {
        self.duration_ms
    }

    #[must_use]
    pub fn measurements(&self) -> &BTreeMap<String, Value> {
        &self.measurements
    }

    #[must_use]
    pub fn observations(&self) -> &[String] {
        &self.observations
    }

    /// # Errors
    ///
    /// Returns a sensitive-field error when the name, or any object key within
    /// the serialized value, contains a forbidden marker. Returns a JSON error
    /// when the value cannot be serialized.
    pub fn measure(&mut self, name: &str, value: impl Serialize) -> Result<(), EvidenceError> {
        validate_field_name(name)?;
        let value = serde_json::to_value(value)?;
        validate_measurement_value(&value)?;
        self.measurements.insert(name.to_owned(), value);
        Ok(())
    }

    /// # Errors
    ///
    /// Returns a sensitive-observation error when the observation contains a
    /// forbidden marker.
    pub fn observe(&mut self, observation: impl Into<String>) -> Result<(), EvidenceError> {
        let observation = observation.into();
        if contains_forbidden_marker(&observation) {
            return Err(EvidenceError::SensitiveObservation(observation));
        }
        self.observations.push(observation);
        Ok(())
    }

    pub fn finish(&mut self, verdict: Verdict, duration_ms: u64) {
        self.verdict = Some(verdict);
        self.duration_ms = Some(duration_ms);
    }

    /// # Errors
    ///
    /// Returns an unfinished-evidence error until both a verdict and duration
    /// have been supplied, or a validation or JSON error if the record is invalid.
    pub fn to_pretty_json(&self) -> Result<String, EvidenceError> {
        let document = self.document()?;
        Ok(serde_json::to_string_pretty(&document)?)
    }

    /// Parses a complete redacted evidence document.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed or unknown JSON fields, unsupported schema
    /// versions or targets, unfinished evidence, or forbidden measurement and
    /// observation content.
    pub fn from_json(input: &str) -> Result<Self, EvidenceError> {
        let document = serde_json::from_str(input)?;
        Self::from_document(document)
    }

    fn document(&self) -> Result<EvidenceDocument, EvidenceError> {
        Self::from_parts(
            self.schema_version,
            self.spike,
            self.target.clone(),
            self.verdict,
            self.duration_ms,
            self.measurements.clone(),
            self.observations.clone(),
        )
    }

    fn from_document(document: EvidenceDocument) -> Result<Self, EvidenceError> {
        let document = Self::from_parts(
            document.schema_version,
            document.spike,
            document.target,
            document.verdict,
            document.duration_ms,
            document.measurements,
            document.observations,
        )?;
        Ok(Self {
            schema_version: document.schema_version,
            spike: document.spike,
            target: document.target,
            verdict: document.verdict,
            duration_ms: document.duration_ms,
            measurements: document.measurements,
            observations: document.observations,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn from_parts(
        schema_version: u32,
        spike: SpikeId,
        target: TargetId,
        verdict: Option<Verdict>,
        duration_ms: Option<u64>,
        measurements: BTreeMap<String, Value>,
        observations: Vec<String>,
    ) -> Result<EvidenceDocument, EvidenceError> {
        if schema_version != 1 {
            return Err(EvidenceError::UnsupportedSchemaVersion(schema_version));
        }
        if verdict.is_none() || duration_ms.is_none() {
            return Err(EvidenceError::Unfinished);
        }
        for (name, value) in &measurements {
            validate_field_name(name)?;
            validate_measurement_value(value)?;
        }
        for observation in &observations {
            if contains_forbidden_marker(observation) {
                return Err(EvidenceError::SensitiveObservation(observation.clone()));
            }
        }
        Ok(EvidenceDocument {
            schema_version,
            spike,
            target,
            verdict,
            duration_ms,
            measurements,
            observations,
        })
    }
}

fn validate_field_name(name: &str) -> Result<(), EvidenceError> {
    if contains_forbidden_marker(name) {
        Err(EvidenceError::SensitiveField(name.to_owned()))
    } else {
        Ok(())
    }
}

fn validate_measurement_value(value: &Value) -> Result<(), EvidenceError> {
    match value {
        Value::Array(values) => {
            for value in values {
                validate_measurement_value(value)?;
            }
        }
        Value::Object(values) => {
            for (name, value) in values {
                validate_field_name(name)?;
                validate_measurement_value(value)?;
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
    Ok(())
}

fn contains_forbidden_marker(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase();
    FORBIDDEN.iter().any(|marker| normalized.contains(marker))
}
