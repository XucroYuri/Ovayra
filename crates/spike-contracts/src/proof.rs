use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{SpikeId, TargetId};

/// The versioned, tagged records consumed by the Phase 0 acceptance gate.
/// Generic `Evidence.measurements` are deliberately not accepted here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhaseZeroProof {
    pub schema_version: u32,
    pub component: ProofComponent,
    pub row: ProofRow,
    pub proof: ProofPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProofRow {
    pub spike: SpikeId,
    pub target: TargetId,
    pub session: Option<String>,
    pub backend: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProofComponent {
    Preview,
    MediaCpu,
    MediaHardware,
    MediaForcedFallback,
    GeminiStage,
    GeminiResume,
    PlatformKeyring,
    PlatformTray,
    PlatformNoTray,
    PlatformProcess,
    PlatformCheckpoint,
    DistributionFfmpeg,
    DistributionPackage,
    DistributionUpdate,
}

impl ProofComponent {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::MediaCpu => "media_cpu",
            Self::MediaHardware => "media_hardware",
            Self::MediaForcedFallback => "media_forced_fallback",
            Self::GeminiStage => "gemini_stage",
            Self::GeminiResume => "gemini_resume",
            Self::PlatformKeyring => "platform_keyring",
            Self::PlatformTray => "platform_tray",
            Self::PlatformNoTray => "platform_no_tray",
            Self::PlatformProcess => "platform_process",
            Self::PlatformCheckpoint => "platform_checkpoint",
            Self::DistributionFfmpeg => "distribution_ffmpeg",
            Self::DistributionPackage => "distribution_package",
            Self::DistributionUpdate => "distribution_update",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProofPayload {
    Preview(PreviewProof),
    MediaCpu(MediaCpuProof),
    MediaHardware(MediaHardwareProof),
    MediaForcedFallback(MediaForcedFallbackProof),
    GeminiStage(GeminiStageProof),
    GeminiResume(GeminiResumeProof),
    PlatformKeyring(PlatformKeyringProof),
    PlatformTray(PlatformTrayProof),
    PlatformNoTray(PlatformNoTrayProof),
    PlatformProcess(PlatformProcessProof),
    PlatformCheckpoint(PlatformCheckpointProof),
    DistributionFfmpeg(DistributionFfmpegProof),
    DistributionPackage(DistributionPackageProof),
    DistributionUpdate(DistributionUpdateProof),
}

macro_rules! strict {
    ($name:ident { $($field:ident : $ty:ty),+ $(,)? }) => {
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct $name { $(pub $field: $ty),+ }
    };
}

strict!(PreviewProof {
    requested_duration_ms: u64,
    measured_duration_ms: u64,
    milli_fps: u64,
    p95_ms: u64,
    rss_growth_mib: u64,
    frames_read: u64,
    frames_applied: u64,
    frames_dropped: u64,
    hidden: bool,
    restored: bool,
    event_loop_errors: u64,
    stream_errors: u64,
    renderer: String
});
strict!(MediaCpuProof {
    actual_backend: String,
    output_duration_seconds: u64,
    video_codec: String,
    audio_codec: String,
    progress_complete: bool,
    output_sha256: String
});
strict!(MediaHardwareProof {
    requested_backend: String,
    actual_backend: String,
    output_duration_seconds: u64,
    output_sha256: String
});
strict!(MediaForcedFallbackProof {
    requested_backend: String,
    cpu_restarts: u8,
    session_quarantined: bool,
    video_codec: String,
    audio_codec: String,
    output_sha256: String
});
strict!(GeminiStageProof {
    checkpoint_id: String,
    staged_offset: u64,
    server_offset: u64,
    retry_policy_observed: bool,
    chunk_granularity: u64,
    encrypted: bool,
    plaintext_absent: bool
});
strict!(GeminiResumeProof {
    checkpoint_id: String,
    resumed_offset: u64,
    server_offset: u64,
    server_authoritative: bool,
    remote_state: String,
    analysis_nonempty: bool,
    model: String,
    http_status: u16,
    remote_deleted: bool,
    checkpoint_deleted: bool,
    retry_policy_observed: bool
});
strict!(PlatformKeyringProof {
    set_ok: bool,
    get_ok: bool,
    delete_ok: bool,
    missing_after_delete: bool
});
strict!(PlatformTrayProof {
    hidden: bool,
    restored: bool,
    quit: bool
});
strict!(PlatformNoTrayProof {
    accessible: bool,
    warning_shown: bool,
    quit: bool
});
strict!(PlatformProcessProof {
    parent_dead: bool,
    grandchild_dead: bool,
    elapsed_ms: u64
});
strict!(PlatformCheckpointProof {
    encrypted: bool,
    plaintext_absent: bool
});
strict!(DistributionFfmpegProof {
    immutable_lock: bool,
    source_signature: bool,
    sbom: bool,
    reproducible: bool,
    lgpl_only: bool,
    source_correspondence: bool,
    source_lock_sha256: String,
    bundle_tree_sha256: String
});
strict!(ArtifactDigestProof {
    format: String,
    sha256: String,
    length: u64
});
strict!(DistributionPackageProof {
    artifacts: Vec<ArtifactDigestProof>,
    source_lock_sha256: String,
    inspection_sha256: String,
    platform_verification: String,
    notarization: Option<String>
});
strict!(DistributionUpdateProof {
    manifest_sha256: String,
    artifacts: Vec<ArtifactDigestProof>,
    updater_format: String,
    signature_verification: String,
    tamper_rejection: String
});

#[derive(Debug, Error)]
pub enum ProofError {
    #[error("unsupported phase 0 proof schema version: {0}")]
    Schema(u32),
    #[error("proof component does not match its tagged payload")]
    Component,
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl PhaseZeroProof {
    /// Builds one schema-v2 record from typed values. Producers must serialize
    /// it through [`Self::to_pretty_json`] before it is admitted to the gate.
    #[must_use]
    pub fn record(component: ProofComponent, row: ProofRow, proof: ProofPayload) -> Self {
        Self {
            schema_version: 2,
            component,
            row,
            proof,
        }
    }

    #[must_use]
    pub fn preview(target: &TargetId, renderer: &str, value: &PreviewProof) -> Self {
        Self {
            schema_version: 2,
            component: ProofComponent::Preview,
            row: ProofRow {
                spike: SpikeId::Preview,
                target: target.clone(),
                session: phase_zero_session(target).map(str::to_owned),
                backend: None,
            },
            proof: ProofPayload::Preview(PreviewProof {
                renderer: renderer.to_owned(),
                ..value.clone()
            }),
        }
    }

    #[must_use]
    pub fn media_cpu(target: TargetId, value: MediaCpuProof) -> Self {
        Self {
            schema_version: 2,
            component: ProofComponent::MediaCpu,
            row: ProofRow {
                spike: SpikeId::Media,
                target,
                session: None,
                backend: Some("cpu-fallback".to_owned()),
            },
            proof: ProofPayload::MediaCpu(value),
        }
    }

    #[must_use]
    pub fn media_hardware(target: TargetId, backend: String, value: MediaHardwareProof) -> Self {
        Self {
            schema_version: 2,
            component: ProofComponent::MediaHardware,
            row: ProofRow {
                spike: SpikeId::Media,
                target,
                session: None,
                backend: Some(backend),
            },
            proof: ProofPayload::MediaHardware(value),
        }
    }

    #[must_use]
    pub fn media_forced_fallback(
        target: TargetId,
        backend: String,
        value: MediaForcedFallbackProof,
    ) -> Self {
        Self {
            schema_version: 2,
            component: ProofComponent::MediaForcedFallback,
            row: ProofRow {
                spike: SpikeId::Media,
                target,
                session: None,
                backend: Some(backend),
            },
            proof: ProofPayload::MediaForcedFallback(value),
        }
    }
    /// Parses one exact schema-v2 proof document.
    ///
    /// # Errors
    ///
    /// Returns an error when the JSON is malformed, has unknown fields, or is
    /// not a schema-v2 component/payload pairing.
    pub fn from_json(input: &str) -> Result<Self, ProofError> {
        let proof: Self = serde_json::from_str(input)?;
        proof.validate()?;
        Ok(proof)
    }

    /// Serializes one validated proof document.
    ///
    /// # Errors
    ///
    /// Returns an error if the proof is not schema-v2 or its component and
    /// payload tags disagree.
    pub fn to_pretty_json(&self) -> Result<String, ProofError> {
        self.validate()?;
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Validates the schema version and component/payload pairing.
    ///
    /// # Errors
    ///
    /// Returns an error for unsupported schemas or mismatched variant tags.
    pub fn validate(&self) -> Result<(), ProofError> {
        if self.schema_version != 2 {
            return Err(ProofError::Schema(self.schema_version));
        }
        let correct = matches!(
            (&self.component, &self.proof),
            (ProofComponent::Preview, ProofPayload::Preview(_))
                | (ProofComponent::MediaCpu, ProofPayload::MediaCpu(_))
                | (
                    ProofComponent::MediaHardware,
                    ProofPayload::MediaHardware(_)
                )
                | (
                    ProofComponent::MediaForcedFallback,
                    ProofPayload::MediaForcedFallback(_)
                )
                | (ProofComponent::GeminiStage, ProofPayload::GeminiStage(_))
                | (ProofComponent::GeminiResume, ProofPayload::GeminiResume(_))
                | (
                    ProofComponent::PlatformKeyring,
                    ProofPayload::PlatformKeyring(_)
                )
                | (ProofComponent::PlatformTray, ProofPayload::PlatformTray(_))
                | (
                    ProofComponent::PlatformNoTray,
                    ProofPayload::PlatformNoTray(_)
                )
                | (
                    ProofComponent::PlatformProcess,
                    ProofPayload::PlatformProcess(_)
                )
                | (
                    ProofComponent::PlatformCheckpoint,
                    ProofPayload::PlatformCheckpoint(_)
                )
                | (
                    ProofComponent::DistributionFfmpeg,
                    ProofPayload::DistributionFfmpeg(_)
                )
                | (
                    ProofComponent::DistributionPackage,
                    ProofPayload::DistributionPackage(_)
                )
                | (
                    ProofComponent::DistributionUpdate,
                    ProofPayload::DistributionUpdate(_)
                )
        );
        if correct {
            Ok(())
        } else {
            Err(ProofError::Component)
        }
    }
}

#[must_use]
pub fn phase_zero_session(target: &TargetId) -> Option<&str> {
    match target.as_str() {
        "macos-arm64-vt" => Some("aqua"),
        "windows-x64-mf" | "windows-x64-nvidia" => Some("windows"),
        "linux-x64-vaapi-wayland" => Some("wayland"),
        "linux-x64-vaapi-x11" => Some("x11"),
        _ => None,
    }
}
