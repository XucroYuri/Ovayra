#![forbid(unsafe_code)]

//! Media-processing spike support.

mod capability;
mod cpu_fallback;
mod ffmpeg;
mod preview;
mod progress;

pub use capability::{
    AttemptOutcome, Backend, DowngradeCode, ExecutionPolicy, ExecutionPolicyError,
    FORCED_FAILURE_DEVICE, HardwarePlan, Inventory, InventoryCommand, InventoryError,
    InventoryOutput,
};
pub use cpu_fallback::{
    CpuFallback, CpuFallbackError, CpuFallbackOutput, FfprobeError, FfprobeReport,
    content_sha256_bytes, redacted_process_detail,
};
pub use ffmpeg::{
    COMMON_ARGS, FfmpegError, FfmpegEvidence, FfmpegRunner, InventoryCollectionError,
};
pub use preview::{
    FfmpegPreview, Frame, FrameError, LatestFrame, PREVIEW_FRAME_BYTES, PREVIEW_HEIGHT,
    PREVIEW_WIDTH, PreviewError, PreviewRun,
};
pub use progress::{ProgressError, ProgressEvent, ProgressParser};
