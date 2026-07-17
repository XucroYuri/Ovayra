#![forbid(unsafe_code)]

//! Release-verification spike support.

mod ffmpeg_policy;

pub use ffmpeg_policy::{FfmpegBundle, FfmpegPolicyError};
