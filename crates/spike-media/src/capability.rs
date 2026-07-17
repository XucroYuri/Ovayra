/// Stable names for the hardware paths evaluated in the feasibility spike.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    VideoToolbox,
    D3d11vaMf,
    NvencNvdec,
    Vaapi,
}

impl Backend {
    pub const ALL: [Self; 4] = [
        Self::VideoToolbox,
        Self::D3d11vaMf,
        Self::NvencNvdec,
        Self::Vaapi,
    ];
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
        };
        Self { backend, args }
    }

    #[must_use]
    pub fn args(&self) -> &[&str] {
        &self.args
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
        self.required_components()
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
        }
    }
}
