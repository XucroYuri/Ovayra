#![forbid(unsafe_code)]

//! Media-processing spike support.

mod capability;
mod ffmpeg;
mod progress;

pub use capability::{Backend, HardwarePlan, Inventory};
pub use ffmpeg::{COMMON_ARGS, FfmpegError, FfmpegEvidence, FfmpegRunner};
pub use progress::{ProgressError, ProgressEvent, ProgressParser};
