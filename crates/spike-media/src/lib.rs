#![forbid(unsafe_code)]

//! Media-processing spike support.

mod capability;
mod cpu_fallback;
mod ffmpeg;
mod progress;

pub use capability::{
    AttemptOutcome, Backend, DowngradeCode, ExecutionPolicy, ExecutionPolicyError, HardwarePlan,
    Inventory, InventoryCommand, InventoryError, InventoryOutput,
};
pub use cpu_fallback::{
    CpuFallback, CpuFallbackError, CpuFallbackOutput, FfprobeError, FfprobeReport,
    content_sha256_bytes, redacted_process_detail,
};
pub use ffmpeg::{
    COMMON_ARGS, FfmpegError, FfmpegEvidence, FfmpegRunner, InventoryCollectionError,
};
pub use progress::{ProgressError, ProgressEvent, ProgressParser};
