use std::{ffi::OsString, path::Path, str::FromStr};

/// Stable names for the hardware paths evaluated in the feasibility spike.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    VideoToolbox,
    D3d11vaMf,
    NvencNvdec,
    Vaapi,
    /// The actual backend after a bounded hardware downgrade.
    Cpu,
}

impl Backend {
    pub const ALL: [Self; 4] = [
        Self::VideoToolbox,
        Self::D3d11vaMf,
        Self::NvencNvdec,
        Self::Vaapi,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::VideoToolbox => "videotoolbox",
            Self::D3d11vaMf => "d3d11va-mf",
            Self::NvencNvdec => "nvenc-nvdec",
            Self::Vaapi => "vaapi",
            Self::Cpu => "cpu",
        }
    }

    #[must_use]
    pub const fn is_cpu(self) -> bool {
        matches!(self, Self::Cpu)
    }
}

impl FromStr for Backend {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "videotoolbox" => Ok(Self::VideoToolbox),
            "d3d11va-mf" => Ok(Self::D3d11vaMf),
            "nvenc-nvdec" => Ok(Self::NvencNvdec),
            "vaapi" => Ok(Self::Vaapi),
            "cpu" => Ok(Self::Cpu),
            _ => Err("expected videotoolbox, d3d11va-mf, nvenc-nvdec, vaapi, or cpu"),
        }
    }
}

/// A normalized, bounded reason for hardware-to-CPU downgrade evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DowngradeCode {
    ProbeFailed,
    SpawnFailed,
    TimedOut,
    NonZeroExit,
    MissingFrames,
    InvalidFfprobe,
    Failed,
}

impl DowngradeCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProbeFailed => "probe_failed",
            Self::SpawnFailed => "spawn_failed",
            Self::TimedOut => "timed_out",
            Self::NonZeroExit => "nonzero_exit",
            Self::MissingFrames => "missing_frames",
            Self::InvalidFfprobe => "invalid_ffprobe",
            Self::Failed => "failed",
        }
    }
}

/// The result of the currently scheduled attempt; no raw process output escapes this type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttemptOutcome {
    Succeeded,
    ProbeFailed,
    SpawnFailed,
    TimedOut,
    NonZeroExit,
    MissingFrames,
    InvalidFfprobe,
    /// Compatibility input for old callers. It is bounded internally and is never evidence.
    Failed(String),
}

impl AttemptOutcome {
    const fn downgrade_code(&self) -> Option<DowngradeCode> {
        match self {
            Self::Succeeded => None,
            Self::ProbeFailed => Some(DowngradeCode::ProbeFailed),
            Self::SpawnFailed => Some(DowngradeCode::SpawnFailed),
            Self::TimedOut => Some(DowngradeCode::TimedOut),
            Self::NonZeroExit => Some(DowngradeCode::NonZeroExit),
            Self::MissingFrames => Some(DowngradeCode::MissingFrames),
            Self::InvalidFfprobe => Some(DowngradeCode::InvalidFfprobe),
            Self::Failed(_) => Some(DowngradeCode::Failed),
        }
    }
}

/// A two-attempt stage policy: one preferred hardware attempt followed by CPU at most once.
#[derive(Debug, Clone)]
pub struct ExecutionPolicy {
    requested_backend: Backend,
    scheduled_backend: Option<Backend>,
    actual_backend: Option<Backend>,
    downgrade_code: Option<DowngradeCode>,
    downgrade_reason: Option<String>,
    attempts_started: u8,
    hardware_quarantined: bool,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ExecutionPolicyError {
    #[error("the two-attempt execution policy is terminal")]
    Terminal,
}

impl ExecutionPolicy {
    /// Starts a policy for a hardware backend.
    ///
    /// # Panics
    ///
    /// Panics when passed `Backend::Cpu`, which is only an actual fallback result.
    #[must_use]
    pub fn prefer(backend: Backend) -> Self {
        assert!(!backend.is_cpu(), "a preferred backend must be hardware");
        Self {
            requested_backend: backend,
            scheduled_backend: Some(backend),
            actual_backend: None,
            downgrade_code: None,
            downgrade_reason: None,
            attempts_started: 1,
            hardware_quarantined: false,
        }
    }

    /// Observes exactly the currently scheduled attempt and returns the next backend, if any.
    ///
    /// # Errors
    ///
    /// Returns terminal once the preferred hardware and CPU attempts have been consumed.
    pub fn observe(&mut self, outcome: AttemptOutcome) -> Result<Backend, ExecutionPolicyError> {
        let Some(current) = self.scheduled_backend else {
            return Err(ExecutionPolicyError::Terminal);
        };
        if matches!(outcome, AttemptOutcome::Succeeded) {
            self.actual_backend = Some(current);
            self.scheduled_backend = None;
            return Ok(current);
        }
        if current.is_cpu() {
            self.scheduled_backend = None;
            return Err(ExecutionPolicyError::Terminal);
        }
        self.hardware_quarantined = true;
        self.downgrade_code = outcome.downgrade_code();
        self.downgrade_reason = match outcome {
            AttemptOutcome::Failed(reason) => Some(bound_reason(&reason)),
            _ => None,
        };
        self.scheduled_backend = Some(Backend::Cpu);
        self.actual_backend = Some(Backend::Cpu);
        self.attempts_started = 2;
        Ok(Backend::Cpu)
    }

    #[must_use]
    pub const fn requested_backend(&self) -> Backend {
        self.requested_backend
    }

    #[must_use]
    pub const fn actual_backend(&self) -> Option<Backend> {
        self.actual_backend
    }

    #[must_use]
    pub const fn downgrade_code(&self) -> Option<DowngradeCode> {
        self.downgrade_code
    }

    /// A bounded diagnostic for compatibility only; evidence must use `downgrade_code` instead.
    #[must_use]
    pub fn downgrade_reason(&self) -> Option<&str> {
        self.downgrade_reason.as_deref()
    }

    #[must_use]
    pub const fn may_retry_hardware_in_this_session(&self) -> bool {
        !self.hardware_quarantined && self.scheduled_backend.is_some()
    }

    #[must_use]
    pub const fn attempts_started(&self) -> u8 {
        self.attempts_started
    }
}

fn bound_reason(reason: &str) -> String {
    const MAX_REASON_BYTES: usize = 512;
    if reason.len() <= MAX_REASON_BYTES {
        return reason.to_owned();
    }
    let mut end = 0;
    for (index, character) in reason.char_indices() {
        let next = index + character.len_utf8();
        if next > MAX_REASON_BYTES {
            break;
        }
        end = next;
    }
    reason[..end].to_owned()
}

/// The bounded, non-sensitive text returned by `FFmpeg` inventory commands.
#[derive(Debug, Clone, Default)]
pub struct Inventory {
    version: String,
    buildconf: String,
    hwaccels: String,
    decoders: String,
    encoders: String,
    filters: String,
}

const MAX_INVENTORY_OUTPUT_BYTES: usize = 64 * 1024;

/// The required `FFmpeg` inventory commands, each executed independently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryCommand {
    Version,
    Buildconf,
    Hwaccels,
    Decoders,
    Encoders,
    Filters,
}

impl InventoryCommand {
    pub const ALL: [Self; 6] = [
        Self::Version,
        Self::Buildconf,
        Self::Hwaccels,
        Self::Decoders,
        Self::Encoders,
        Self::Filters,
    ];

    #[must_use]
    pub const fn args(self) -> [&'static str; 1] {
        match self {
            Self::Version => ["-version"],
            Self::Buildconf => ["-buildconf"],
            Self::Hwaccels => ["-hwaccels"],
            Self::Decoders => ["-decoders"],
            Self::Encoders => ["-encoders"],
            Self::Filters => ["-filters"],
        }
    }
}

/// One bounded inventory command result. Raw paths and command lines are not retained.
#[derive(Debug, Clone)]
pub struct InventoryOutput {
    command: InventoryCommand,
    exit_code: Option<i32>,
    output: String,
}

impl InventoryOutput {
    #[must_use]
    pub fn success(command: InventoryCommand, output: impl AsRef<str>) -> Self {
        Self::new(command, Some(0), output)
    }

    #[must_use]
    pub fn failed(command: InventoryCommand, output: impl AsRef<str>) -> Self {
        Self::new(command, Some(1), output)
    }

    #[must_use]
    pub fn new(command: InventoryCommand, exit_code: Option<i32>, output: impl AsRef<str>) -> Self {
        let output = output.as_ref();
        let output = if output.len() <= MAX_INVENTORY_OUTPUT_BYTES {
            output.to_owned()
        } else {
            let mut end = 0;
            for (index, character) in output.char_indices() {
                let next = index + character.len_utf8();
                if next > MAX_INVENTORY_OUTPUT_BYTES {
                    break;
                }
                end = next;
            }
            output[..end].to_owned()
        };
        Self {
            command,
            exit_code,
            output,
        }
    }

    #[must_use]
    pub const fn byte_len(&self) -> usize {
        self.output.len()
    }
}

/// Prevents partial or failed command data from being treated as inventory.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InventoryError {
    #[error("missing required inventory command: {command:?}")]
    MissingCommand { command: InventoryCommand },
    #[error("required inventory command failed: {command:?}")]
    FailedCommand { command: InventoryCommand },
    #[error("inventory command was supplied more than once: {command:?}")]
    DuplicateCommand { command: InventoryCommand },
}

impl Inventory {
    /// Creates an inventory only from all six successful, distinct required command outputs.
    ///
    /// # Errors
    ///
    /// Returns an error if any required command is missing, duplicated, or failed.
    pub fn from_command_outputs(outputs: &[InventoryOutput]) -> Result<Self, InventoryError> {
        let mut inventory = Self::default();
        let mut seen = [false; 6];
        for output in outputs {
            let index = output.command as usize;
            if seen[index] {
                return Err(InventoryError::DuplicateCommand {
                    command: output.command,
                });
            }
            if output.exit_code != Some(0) {
                return Err(InventoryError::FailedCommand {
                    command: output.command,
                });
            }
            seen[index] = true;
            match output.command {
                InventoryCommand::Version => output.output.clone_into(&mut inventory.version),
                InventoryCommand::Buildconf => output.output.clone_into(&mut inventory.buildconf),
                InventoryCommand::Hwaccels => output.output.clone_into(&mut inventory.hwaccels),
                InventoryCommand::Decoders => output.output.clone_into(&mut inventory.decoders),
                InventoryCommand::Encoders => output.output.clone_into(&mut inventory.encoders),
                InventoryCommand::Filters => output.output.clone_into(&mut inventory.filters),
            }
        }
        for command in InventoryCommand::ALL {
            if !seen[command as usize] {
                return Err(InventoryError::MissingCommand { command });
            }
        }
        Ok(inventory)
    }

    fn has(&self, kind: InventoryKind, component: &str) -> bool {
        let output = match kind {
            InventoryKind::Hwaccel => &self.hwaccels,
            InventoryKind::Decoder => &self.decoders,
            InventoryKind::Encoder => &self.encoders,
            InventoryKind::Filter => &self.filters,
        };
        output.split_whitespace().any(|name| name == component)
    }
}

#[derive(Debug, Clone, Copy)]
enum InventoryKind {
    Hwaccel,
    Decoder,
    Encoder,
    Filter,
}

/// A self-test command plan. The input source is generated before this plan runs.
#[derive(Debug, Clone)]
pub struct HardwarePlan {
    backend: Backend,
    args: Vec<&'static str>,
}

impl HardwarePlan {
    /// Creates the static hardware self-test plan.
    ///
    /// # Panics
    ///
    /// Panics when passed `Backend::Cpu`, which has no hardware plan.
    #[must_use]
    pub fn self_test(backend: Backend) -> Self {
        let args = match backend {
            Backend::VideoToolbox => vec![
                "-hwaccel",
                "videotoolbox",
                "-i",
                "synthetic-h264-aac.mp4",
                "-vf",
                "scale=1280:720",
                "-c:v",
                "h264_videotoolbox",
                "-c:a",
                "copy",
                "-f",
                "null",
                "-",
            ],
            Backend::D3d11vaMf => vec![
                "-hwaccel",
                "d3d11va",
                "-i",
                "synthetic-h264-aac.mp4",
                "-vf",
                "scale=1280:720",
                "-c:v",
                "h264_mf",
                "-c:a",
                "copy",
                "-f",
                "null",
                "-",
            ],
            Backend::NvencNvdec => vec![
                "-hwaccel",
                "cuda",
                "-i",
                "synthetic-h264-aac.mp4",
                "-vf",
                "scale_cuda=1280:720",
                "-c:v",
                "h264_nvenc",
                "-c:a",
                "copy",
                "-f",
                "null",
                "-",
            ],
            Backend::Vaapi => vec![
                "-vaapi_device",
                "/dev/dri/renderD128",
                "-hwaccel",
                "vaapi",
                "-hwaccel_output_format",
                "vaapi",
                "-i",
                "synthetic-h264-aac.mp4",
                "-vf",
                "scale_vaapi=w=1280:h=720",
                "-c:v",
                "h264_vaapi",
                "-c:a",
                "copy",
                "-f",
                "null",
                "-",
            ],
            Backend::Cpu => panic!("CPU is a fallback output, not a hardware self-test plan"),
        };
        Self { backend, args }
    }

    #[must_use]
    pub fn args(&self) -> &[&str] {
        &self.args
    }

    /// Builds a concrete ten-second hardware transcode using supplied paths and an optional
    /// explicit render device. The returned vector contains no inferred device identifiers.
    ///
    /// # Panics
    ///
    /// Panics only if this plan was somehow constructed for `Backend::Cpu`.
    #[must_use]
    pub fn transcode_args(
        &self,
        input: &Path,
        output: &Path,
        render_device: Option<&Path>,
    ) -> Vec<OsString> {
        let mut args = vec![OsString::from("-y")];
        match self.backend {
            Backend::Vaapi => {
                args.push(OsString::from("-vaapi_device"));
                args.push(
                    render_device
                        .unwrap_or_else(|| Path::new("/dev/dri/renderD128"))
                        .as_os_str()
                        .to_owned(),
                );
            }
            Backend::VideoToolbox | Backend::D3d11vaMf | Backend::NvencNvdec => {
                if let Some(device) = render_device {
                    args.push(OsString::from("-hwaccel_device"));
                    args.push(device.as_os_str().to_owned());
                }
            }
            Backend::Cpu => panic!("CPU is a fallback output, not a hardware self-test plan"),
        }
        for argument in self.args.iter().copied() {
            if argument == "synthetic-h264-aac.mp4" {
                args.push(input.as_os_str().to_owned());
            } else if argument != "-f" && argument != "null" && argument != "-" {
                // The self-test fixture targets a null sink; the executable plan encodes output.
                args.push(OsString::from(argument));
            }
        }
        args.retain(|argument| {
            argument != "-vaapi_device" || !matches!(self.backend, Backend::Vaapi)
        });
        if matches!(self.backend, Backend::Vaapi) {
            // Drop the static default device that belonged to the fixture plan.
            args.retain(|argument| argument != "/dev/dri/renderD128");
            args.splice(
                1..1,
                [
                    OsString::from("-vaapi_device"),
                    render_device
                        .unwrap_or_else(|| Path::new("/dev/dri/renderD128"))
                        .as_os_str()
                        .to_owned(),
                ],
            );
        }
        args.push(OsString::from("-t"));
        args.push(OsString::from("10"));
        args.push(output.as_os_str().to_owned());
        args
    }

    /// The canonical generated input, shared by every backend self-test.
    #[must_use]
    pub fn source_args(&self) -> &'static [&'static str] {
        const SOURCE: &[&str] = &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=1280x720:rate=30",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=1000:sample_rate=48000",
            "-t",
            "10",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "synthetic-h264-aac.mp4",
        ];
        SOURCE
    }

    #[must_use]
    pub const fn requires_observed_output(&self) -> bool {
        true
    }

    /// A backend is available only after an inventory match and a successful, frame-producing run.
    #[must_use]
    pub fn is_available(
        &self,
        inventory: &Inventory,
        exited_successfully: bool,
        output_frames: u64,
    ) -> bool {
        !self.backend.is_cpu()
            && self
                .required_components()
                .iter()
                .all(|(kind, component)| inventory.has(*kind, component))
            && exited_successfully
            && output_frames > 0
    }

    fn required_components(&self) -> &'static [(InventoryKind, &'static str)] {
        match self.backend {
            Backend::VideoToolbox => &[
                (InventoryKind::Hwaccel, "videotoolbox"),
                (InventoryKind::Decoder, "h264"),
                (InventoryKind::Encoder, "h264_videotoolbox"),
                (InventoryKind::Filter, "scale"),
            ],
            Backend::D3d11vaMf => &[
                (InventoryKind::Hwaccel, "d3d11va"),
                (InventoryKind::Decoder, "h264"),
                (InventoryKind::Encoder, "h264_mf"),
                (InventoryKind::Filter, "scale"),
            ],
            Backend::NvencNvdec => &[
                (InventoryKind::Hwaccel, "cuda"),
                (InventoryKind::Decoder, "h264_cuvid"),
                (InventoryKind::Encoder, "h264_nvenc"),
                (InventoryKind::Filter, "scale_cuda"),
            ],
            Backend::Vaapi => &[
                (InventoryKind::Hwaccel, "vaapi"),
                (InventoryKind::Decoder, "h264"),
                (InventoryKind::Encoder, "h264_vaapi"),
                (InventoryKind::Filter, "scale_vaapi"),
            ],
            Backend::Cpu => &[],
        }
    }
}
