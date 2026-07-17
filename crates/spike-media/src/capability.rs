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
    hwaccels: String,
    decoders: String,
    encoders: String,
    filters: String,
}

impl Inventory {
    #[must_use]
    pub fn from_outputs(outputs: &[(&str, &str)]) -> Self {
        let mut inventory = Self::default();
        for (kind, output) in outputs {
            match *kind {
                "-hwaccels" => (*output).clone_into(&mut inventory.hwaccels),
                "-decoders" => (*output).clone_into(&mut inventory.decoders),
                "-encoders" => (*output).clone_into(&mut inventory.encoders),
                "-filters" => (*output).clone_into(&mut inventory.filters),
                _ => {}
            }
        }
        inventory
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
