#![forbid(unsafe_code)]

//! Release-verification spike support.

mod ffmpeg_policy;
mod manifest;
mod package;

pub use ffmpeg_policy::{FfmpegBundle, FfmpegPolicyError};
pub use manifest::{
    PlatformRelease, ReleaseManifest, ReleaseManifestError, ReleaseVerifier, UpdateFormat,
};
pub use package::{PackageError, PackageRelease};
