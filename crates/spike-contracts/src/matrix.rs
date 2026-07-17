use std::{collections::BTreeSet, fs, path::Path};

use serde::Deserialize;
use thiserror::Error;

use crate::{Evidence, PhaseZeroProof, ProofComponent, ProofPayload, SpikeId, TargetId, Verdict};

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
    #[error("missing required evidence: {0}")]
    MissingEvidence(String),
    #[error("unmatched evidence record")]
    UnmatchedEvidence,
    #[error("duplicate evidence record for required matrix row")]
    DuplicateEvidence,
    #[error("required media evidence is missing actual_backend")]
    MissingActualBackend,
    #[error("evidence backend does not match its required matrix row")]
    WrongBackend,
    #[error("required evidence is missing or violates {0}")]
    Requirement(String),
    #[error("matrix rows must be in the canonical Phase 0 order")]
    CanonicalOrder,
    #[error("duplicate proof component for a required matrix row")]
    DuplicateProofComponent,
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

    /// Evaluates the exact Phase 0 evidence set without any waiver path.
    ///
    /// The caller is responsible for parsing each source document through the
    /// strict [`Evidence`] schema first.  This method then rejects records that
    /// do not map one-to-one to the frozen matrix, any non-passing verdict, and
    /// evidence that does not satisfy the documented acceptance thresholds.
    ///
    /// # Errors
    ///
    /// Returns the first deterministic fail-closed reason found while matching
    /// records in input order, or a missing matrix row in canonical order.
    pub fn evaluate(&self, reports: &[Evidence]) -> Result<(), MatrixError> {
        let mut seen = BTreeSet::new();
        let mut matched = Vec::with_capacity(reports.len());
        for report in reports {
            let required = self.match_report(report)?;
            let key = required_key(required);
            if !seen.insert(key) {
                return Err(MatrixError::DuplicateEvidence);
            }
            let verdict = report
                .verdict()
                .ok_or_else(|| MatrixError::Requirement("finished verdict".to_owned()))?;
            self.validate_required_verdict(required, verdict)?;
            matched.push((required, report));
        }
        for (required, report) in matched {
            validate_requirements(required, report)?;
        }

        for required in &self.required {
            if !seen.contains(&required_key(required)) {
                return Err(MatrixError::MissingEvidence(required_key(required)));
            }
        }
        Ok(())
    }

    /// Evaluates only schema-v2 tagged proof components. Generic measurements
    /// are intentionally outside this API and cannot satisfy acceptance.
    ///
    /// # Errors
    ///
    /// Returns a deterministic error for an unmatched, duplicate, missing, or
    /// threshold-violating required proof component.
    pub fn evaluate_proofs(&self, proofs: &[PhaseZeroProof]) -> Result<(), MatrixError> {
        let mut seen = BTreeSet::new();
        for proof in proofs {
            let required = self
                .required
                .iter()
                .find(|required| {
                    required.id == proof.row.spike
                        && required.target == proof.row.target
                        && required.session == proof.row.session
                        && required.backend == proof.row.backend
                })
                .ok_or(MatrixError::UnmatchedEvidence)?;
            let component = proof.component.as_str();
            let key = format!("{}|{component}", required_key(required));
            if !seen.insert(key) {
                return Err(MatrixError::DuplicateProofComponent);
            }
            validate_typed_proof(required, proof)?;
        }
        for required in &self.required {
            for component in required_components(required) {
                if !seen.contains(&format!(
                    "{}|{}",
                    required_key(required),
                    component.as_str()
                )) {
                    return Err(MatrixError::MissingEvidence(format!(
                        "{}|{}",
                        required_key(required),
                        component.as_str()
                    )));
                }
            }
        }
        validate_distribution_relationships(proofs, &self.required)?;
        validate_gemini_relationships(proofs, &self.required)?;
        Ok(())
    }

    fn match_report<'a>(&'a self, report: &Evidence) -> Result<&'a RequiredEvidence, MatrixError> {
        let measurements = report.measurements();
        let requested_backend = measurement_str(measurements, "requested_backend");
        let session = measurement_str(measurements, "session");
        let candidates = self
            .required
            .iter()
            .filter(|entry| entry.id == report.spike() && entry.target == *report.target())
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            return Err(MatrixError::UnmatchedEvidence);
        }

        let candidate = candidates.into_iter().find(|entry| {
            entry.session.as_deref() == session && entry.backend.as_deref() == requested_backend
        });
        let Some(candidate) = candidate else {
            if report.spike() == SpikeId::Media && requested_backend.is_some() {
                return Err(MatrixError::WrongBackend);
            }
            return Err(MatrixError::UnmatchedEvidence);
        };
        if candidate.backend.is_some() && measurement_str(measurements, "actual_backend").is_none()
        {
            return Err(MatrixError::MissingActualBackend);
        }
        Ok(candidate)
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
            .collect::<Vec<_>>();
        let actual = self.required.iter().map(required_key).collect::<Vec<_>>();
        let expected_set = expected.iter().cloned().collect::<BTreeSet<_>>();
        let actual_set = actual.iter().cloned().collect::<BTreeSet<_>>();
        let missing = expected_set
            .difference(&actual_set)
            .cloned()
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(MatrixError::MissingRequiredEntries(missing));
        }
        let unsupported = actual_set
            .difference(&expected_set)
            .cloned()
            .collect::<Vec<_>>();
        if !unsupported.is_empty() {
            return Err(MatrixError::UnsupportedRequiredEntries(unsupported));
        }
        if actual != expected {
            return Err(MatrixError::CanonicalOrder);
        }
        Ok(())
    }
}

fn validate_gemini_relationships(
    proofs: &[PhaseZeroProof],
    required: &[RequiredEvidence],
) -> Result<(), MatrixError> {
    for row in required.iter().filter(|row| row.id == SpikeId::Gemini) {
        let stage = proofs.iter().find(|proof| {
            proof.component == ProofComponent::GeminiStage && proof.row.target == row.target
        });
        let resume = proofs.iter().find(|proof| {
            proof.component == ProofComponent::GeminiResume && proof.row.target == row.target
        });
        let (
            Some(PhaseZeroProof {
                proof: ProofPayload::GeminiStage(stage),
                ..
            }),
            Some(PhaseZeroProof {
                proof: ProofPayload::GeminiResume(resume),
                ..
            }),
        ) = (stage, resume)
        else {
            continue;
        };
        if stage.checkpoint_id != resume.checkpoint_id
            || resume.resumed_offset < stage.staged_offset
            || resume.server_offset < resume.resumed_offset
            || !resume.server_authoritative
        {
            return Err(MatrixError::Requirement(
                "gemini stage/resume relationship".to_owned(),
            ));
        }
    }
    Ok(())
}

fn required_components(required: &RequiredEvidence) -> &'static [ProofComponent] {
    match required.id {
        SpikeId::Preview => &[ProofComponent::Preview],
        SpikeId::Media if required.backend.as_deref() == Some("cpu-fallback") => {
            &[ProofComponent::MediaCpu]
        }
        SpikeId::Media => &[
            ProofComponent::MediaHardware,
            ProofComponent::MediaForcedFallback,
        ],
        SpikeId::Gemini => &[ProofComponent::GeminiStage, ProofComponent::GeminiResume],
        SpikeId::Platform => &[
            ProofComponent::PlatformKeyring,
            ProofComponent::PlatformTray,
            ProofComponent::PlatformNoTray,
            ProofComponent::PlatformProcess,
            ProofComponent::PlatformCheckpoint,
        ],
        SpikeId::Distribution => &[
            ProofComponent::DistributionFfmpeg,
            ProofComponent::DistributionPackage,
            ProofComponent::DistributionUpdate,
        ],
    }
}

#[allow(clippy::too_many_lines)]
fn validate_typed_proof(
    required: &RequiredEvidence,
    proof: &PhaseZeroProof,
) -> Result<(), MatrixError> {
    let ok = match (&proof.component, &proof.proof) {
        (ProofComponent::Preview, ProofPayload::Preview(value)) => {
            value.requested_duration_ms == 120_000
                && value.measured_duration_ms >= 120_000
                && (23_000..=25_000).contains(&value.milli_fps)
                && value.p95_ms <= 100
                && value.rss_growth_mib <= 64
                && value.frames_read > 0
                && value.frames_applied > 0
                && value.frames_applied + value.frames_dropped <= value.frames_read
                && value.hidden
                && value.restored
                && value.event_loop_errors == 0
                && value.stream_errors == 0
                && !value.renderer.is_empty()
        }
        (ProofComponent::MediaCpu, ProofPayload::MediaCpu(value)) => {
            value.actual_backend == "cpu"
                && value.output_duration_seconds >= 10
                && value.video_codec == "vp9"
                && value.audio_codec == "opus"
                && value.progress_complete
                && sha256(Some(&value.output_sha256))
        }
        (ProofComponent::MediaHardware, ProofPayload::MediaHardware(value)) => {
            required.backend.as_deref() == Some(value.requested_backend.as_str())
                && value.actual_backend == value.requested_backend
                && value.output_duration_seconds >= 10
                && sha256(Some(&value.output_sha256))
        }
        (ProofComponent::MediaForcedFallback, ProofPayload::MediaForcedFallback(value)) => {
            required.backend.as_deref() == Some(value.requested_backend.as_str())
                && value.cpu_restarts == 1
                && value.session_quarantined
                && value.video_codec == "vp9"
                && value.audio_codec == "opus"
                && sha256(Some(&value.output_sha256))
        }
        (ProofComponent::GeminiStage, ProofPayload::GeminiStage(value)) => {
            !value.checkpoint_id.is_empty()
                && value.staged_offset > 0
                && value.server_offset == value.staged_offset
                && value.retry_policy_observed
                && value.chunk_granularity > 0
                && value.encrypted
                && value.plaintext_absent
        }
        (ProofComponent::GeminiResume, ProofPayload::GeminiResume(value)) => {
            !value.checkpoint_id.is_empty()
                && value.resumed_offset > 0
                && value.server_offset >= value.resumed_offset
                && value.server_authoritative
                && value.remote_state == "ACTIVE"
                && value.analysis_nonempty
                && value.model == "gemini-3.1-flash-lite"
                && value.http_status == 200
                && value.remote_deleted
                && value.checkpoint_deleted
                && value.retry_policy_observed
        }
        (ProofComponent::PlatformKeyring, ProofPayload::PlatformKeyring(value)) => {
            value.set_ok && value.get_ok && value.delete_ok && value.missing_after_delete
        }
        (ProofComponent::PlatformTray, ProofPayload::PlatformTray(value)) => {
            value.hidden && value.restored && value.quit
        }
        (ProofComponent::PlatformNoTray, ProofPayload::PlatformNoTray(value)) => {
            value.accessible && value.warning_shown && value.quit
        }
        (ProofComponent::PlatformProcess, ProofPayload::PlatformProcess(value)) => {
            value.parent_dead && value.grandchild_dead && value.elapsed_ms <= 5_000
        }
        (ProofComponent::PlatformCheckpoint, ProofPayload::PlatformCheckpoint(value)) => {
            value.encrypted && value.plaintext_absent
        }
        (ProofComponent::DistributionFfmpeg, ProofPayload::DistributionFfmpeg(value)) => {
            value.immutable_lock
                && value.source_signature
                && value.sbom
                && value.reproducible
                && value.lgpl_only
                && value.source_correspondence
                && sha256(Some(&value.source_lock_sha256))
                && sha256(Some(&value.bundle_tree_sha256))
        }
        (ProofComponent::DistributionPackage, ProofPayload::DistributionPackage(value)) => {
            expected_formats(required.target.as_str())
                .is_some_and(|expected| exact_artifact_formats(&value.artifacts, expected))
                && sha256(Some(&value.source_lock_sha256))
                && sha256(Some(&value.inspection_sha256))
                && package_platform_verified(required.target.as_str(), value)
        }
        (ProofComponent::DistributionUpdate, ProofPayload::DistributionUpdate(value)) => {
            expected_formats(required.target.as_str())
                .is_some_and(|expected| exact_artifact_formats(&value.artifacts, expected))
                && sha256(Some(&value.manifest_sha256))
                && expected_updater_format(required.target.as_str())
                    == Some(value.updater_format.as_str())
                && value.signature_verification == "pinned_minisign"
                && value.tamper_rejection == "updater_and_download"
        }
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        Err(MatrixError::Requirement(format!(
            "typed {} proof",
            proof.component.as_str()
        )))
    }
}

fn validate_distribution_relationships(
    proofs: &[PhaseZeroProof],
    required: &[RequiredEvidence],
) -> Result<(), MatrixError> {
    for row in required
        .iter()
        .filter(|row| row.id == SpikeId::Distribution)
    {
        let ffmpeg = proofs.iter().find(|proof| {
            proof.component == ProofComponent::DistributionFfmpeg && proof.row.target == row.target
        });
        let package = proofs.iter().find(|proof| {
            proof.component == ProofComponent::DistributionPackage && proof.row.target == row.target
        });
        let update = proofs.iter().find(|proof| {
            proof.component == ProofComponent::DistributionUpdate && proof.row.target == row.target
        });
        let (
            Some(PhaseZeroProof {
                proof: ProofPayload::DistributionFfmpeg(ffmpeg),
                ..
            }),
            Some(PhaseZeroProof {
                proof: ProofPayload::DistributionPackage(package),
                ..
            }),
            Some(PhaseZeroProof {
                proof: ProofPayload::DistributionUpdate(update),
                ..
            }),
        ) = (ffmpeg, package, update)
        else {
            continue;
        };
        if ffmpeg.source_lock_sha256 != package.source_lock_sha256
            || package.artifacts != update.artifacts
            || !sha256(Some(&update.manifest_sha256))
        {
            return Err(MatrixError::Requirement(
                "distribution proof relationships".to_owned(),
            ));
        }
    }
    Ok(())
}

fn exact_artifact_formats(artifacts: &[crate::ArtifactDigestProof], expected: &[&str]) -> bool {
    let mut actual: Vec<_> = artifacts
        .iter()
        .map(|artifact| artifact.format.as_str())
        .collect();
    actual.sort_unstable();
    let mut expected = expected.to_vec();
    expected.sort_unstable();
    actual == expected
        && artifacts
            .iter()
            .all(|artifact| artifact.length > 0 && sha256(Some(&artifact.sha256)))
}

fn package_platform_verified(target: &str, value: &crate::DistributionPackageProof) -> bool {
    match target {
        "macos-arm64-vt" => {
            value.platform_verification == "codesign_notary_staple"
                && value.notarization.as_deref() == Some("accepted")
        }
        "windows-x64-mf" => {
            value.platform_verification == "authenticode" && value.notarization.is_none()
        }
        "linux-x64-vaapi-wayland" => {
            value.platform_verification == "minisign" && value.notarization.is_none()
        }
        _ => false,
    }
}

fn expected_updater_format(target: &str) -> Option<&'static str> {
    match target {
        "macos-arm64-vt" => Some("app"),
        "windows-x64-mf" => Some("wix"),
        "linux-x64-vaapi-wayland" => Some("appimage"),
        _ => None,
    }
}

fn expected_formats(target: &str) -> Option<&'static [&'static str]> {
    match target {
        "macos-arm64-vt" => Some(&["app", "dmg"]),
        "windows-x64-mf" => Some(&["wix"]),
        "linux-x64-vaapi-wayland" => Some(&["appimage", "deb"]),
        _ => None,
    }
}

fn validate_requirements(
    required: &RequiredEvidence,
    report: &Evidence,
) -> Result<(), MatrixError> {
    match required.id {
        SpikeId::Preview => validate_preview(report),
        SpikeId::Media => validate_media(required, report),
        SpikeId::Gemini => validate_gemini(report),
        SpikeId::Platform => validate_platform(required, report),
        SpikeId::Distribution => validate_distribution(required, report),
    }
}

fn validate_preview(report: &Evidence) -> Result<(), MatrixError> {
    let measurements = report.measurements();
    let valid = measurement_u64(measurements, "observed_milli_fps")
        .is_some_and(|value| (23_000..=25_000).contains(&value))
        && measurement_u64(measurements, "requested_duration_seconds") == Some(120)
        && measurement_u64(measurements, "measured_elapsed_ms")
            .is_some_and(|value| value >= 120_000)
        && report.duration_ms().is_some_and(|value| value >= 120_000)
        && measurement_u64(measurements, "frames_read").is_some_and(|value| value > 0)
        && measurement_u64(measurements, "frames_applied").is_some_and(|value| value > 0)
        && measurement_bool(measurements, "automation_hide") == Some(true)
        && measurement_bool(measurements, "automation_restore") == Some(true)
        && measurement_u64(measurements, "p95_ms").is_some_and(|value| value <= 100)
        && measurement_u64(measurements, "rss_growth_mib").is_some_and(|value| value <= 64)
        && measurement_bool(measurements, "rss_samples_complete") == Some(true)
        && measurement_u64(measurements, "event_loop_errors") == Some(0)
        && measurement_u64(measurements, "preview_stream_errors") == Some(0);
    if valid {
        Ok(())
    } else {
        Err(MatrixError::Requirement("preview thresholds".to_owned()))
    }
}

fn validate_media(required: &RequiredEvidence, report: &Evidence) -> Result<(), MatrixError> {
    let measurements = report.measurements();
    let backend = required
        .backend
        .as_deref()
        .expect("media rows have a backend");
    let expected_actual = if backend == "cpu-fallback" {
        "cpu"
    } else {
        backend
    };
    if measurement_str(measurements, "actual_backend") != Some(expected_actual) {
        return Err(MatrixError::WrongBackend);
    }
    if !sha256(measurement_str(measurements, "content_sha256")) {
        return Err(MatrixError::Requirement("media content SHA-256".to_owned()));
    }
    if backend == "cpu-fallback"
        && !(measurement_str(measurements, "video_codec") == Some("vp9")
            && measurement_str(measurements, "audio_codec") == Some("opus")
            && measurement_u64(measurements, "media_duration_seconds")
                .is_some_and(|value| value >= 10))
    {
        return Err(MatrixError::Requirement(
            "CPU fallback VP9/Opus WebM".to_owned(),
        ));
    }
    Ok(())
}

fn validate_gemini(report: &Evidence) -> Result<(), MatrixError> {
    let measurements = report.measurements();
    let valid = measurement_u64(measurements, "observed_server_offset")
        .is_some_and(|value| value > 0)
        && measurement_bool(measurements, "offset_mismatch") == Some(false)
        && measurement_bool(measurements, "analysis_nonempty") == Some(true)
        && measurement_str(measurements, "remote_cleanup_state") == Some("deleted")
        && measurement_str(measurements, "checkpoint_cleanup_state") == Some("deleted")
        && measurement_str(measurements, "model") == Some("gemini-3.1-flash-lite")
        && measurement_u64(measurements, "http_status") == Some(200);
    if valid {
        Ok(())
    } else {
        Err(MatrixError::Requirement(
            "Gemini resume, ACTIVE analysis, and cleanup".to_owned(),
        ))
    }
}

fn validate_platform(required: &RequiredEvidence, report: &Evidence) -> Result<(), MatrixError> {
    let measurements = report.measurements();
    let common = [
        "write_status",
        "read_status",
        "cleanup_status",
        "tray_status",
        "process_group_status",
    ]
    .iter()
    .all(|name| measurement_str(measurements, name) == Some("pass"))
        && measurement_u64(measurements, "child_tree_elapsed_ms")
            .is_some_and(|value| value <= 5_000);
    let linux_fallback = !required.target.as_str().starts_with("linux-")
        || (measurement_str(measurements, "forced_no_tray_status") == Some("window-accessible")
            && measurement_bool(measurements, "no_tray_warning_shown") == Some(true));
    if common && linux_fallback {
        Ok(())
    } else {
        Err(MatrixError::Requirement(
            "keyring, tray, or process-group proof".to_owned(),
        ))
    }
}

fn validate_distribution(
    required: &RequiredEvidence,
    report: &Evidence,
) -> Result<(), MatrixError> {
    let measurements = report.measurements();
    let common = [
        ("bundle_validation", "pass"),
        ("license_policy", "LGPL-only"),
        ("source_correspondence", "pass"),
        ("sbom_status", "pass"),
        ("ffmpeg_keyring_status", "pass"),
        ("native_double_build", "pass"),
        ("platform_signature", "pass"),
    ]
    .iter()
    .all(|(name, value)| measurement_str(measurements, name) == Some(*value))
        && measurement_bool(measurements, "update_tamper_rejected") == Some(true)
        && package_formats(measurements, required.target.as_str());
    let notarized = required.target.as_str() != "macos-arm64-vt"
        || measurement_str(measurements, "notarization") == Some("pass");
    if common && notarized {
        Ok(())
    } else {
        Err(MatrixError::Requirement(
            "native package, signature, source, SBOM, or double-build proof".to_owned(),
        ))
    }
}

fn package_formats(
    measurements: &std::collections::BTreeMap<String, serde_json::Value>,
    target: &str,
) -> bool {
    let expected: &[&str] = match target {
        "macos-arm64-vt" => &["app", "dmg"],
        "windows-x64-mf" => &["msi"],
        "linux-x64-vaapi-wayland" => &["appimage", "deb"],
        _ => return false,
    };
    let Some(values) = measurements
        .get("package_formats")
        .and_then(serde_json::Value::as_array)
    else {
        return false;
    };
    let actual = values
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect::<BTreeSet<_>>();
    actual.len() == expected.len() && expected.iter().all(|value| actual.contains(value))
}

fn measurement_str<'a>(
    measurements: &'a std::collections::BTreeMap<String, serde_json::Value>,
    name: &str,
) -> Option<&'a str> {
    measurements.get(name).and_then(serde_json::Value::as_str)
}

fn measurement_u64(
    measurements: &std::collections::BTreeMap<String, serde_json::Value>,
    name: &str,
) -> Option<u64> {
    measurements.get(name).and_then(serde_json::Value::as_u64)
}

fn measurement_bool(
    measurements: &std::collections::BTreeMap<String, serde_json::Value>,
    name: &str,
) -> Option<bool> {
    measurements.get(name).and_then(serde_json::Value::as_bool)
}

fn sha256(value: Option<&str>) -> bool {
    value.is_some_and(|value| {
        value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
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
