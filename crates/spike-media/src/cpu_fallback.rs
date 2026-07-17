use std::{
    ffi::OsString,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt};

use crate::{ProgressError, ProgressParser};

/// CPU-only synthetic `WebM` generator backed exclusively by `FFmpeg` child processes.
#[derive(Debug, Clone)]
pub struct CpuFallback {
    ffmpeg: PathBuf,
    ffprobe: PathBuf,
    timeouts: ProcessTimeouts,
}

#[derive(Debug, Clone, Copy)]
struct ProcessTimeouts {
    generation: Duration,
    utility: Duration,
}

/// Successful `FFmpeg` execution details safe to include in evidence.
#[derive(Debug, Clone, PartialEq)]
pub struct CpuFallbackOutput {
    pub average_speed: Option<f64>,
    pub ffmpeg_build_id: String,
}

#[derive(Debug, Error)]
pub enum CpuFallbackError {
    #[error("requested duration must be positive")]
    InvalidRequestedDuration,
    #[error("unable to start FFmpeg")]
    Spawn,
    #[error("FFmpeg failed")]
    Failed,
    #[error("FFmpeg timed out")]
    TimedOut,
    #[error("FFmpeg output exceeded its bounded limit")]
    OutputLimit,
    #[error("unable to collect FFmpeg output")]
    Collect,
    #[error("FFmpeg build identity was empty")]
    EmptyBuildId,
    #[error("FFmpeg build identity command failed")]
    BuildIdFailed,
    #[error(transparent)]
    Progress(#[from] ProgressError),
}

impl CpuFallbackError {
    fn from_collection(error: &ChildCollectionError) -> Self {
        match error {
            ChildCollectionError::Spawn(_) => Self::Spawn,
            ChildCollectionError::Read(_) | ChildCollectionError::Runtime(_) => Self::Collect,
            ChildCollectionError::TimedOut => Self::TimedOut,
            ChildCollectionError::StdoutLimit => Self::OutputLimit,
        }
    }
}

impl CpuFallback {
    #[must_use]
    pub fn new(ffmpeg: impl Into<PathBuf>, ffprobe: impl Into<PathBuf>) -> Self {
        Self {
            ffmpeg: ffmpeg.into(),
            ffprobe: ffprobe.into(),
            timeouts: ProcessTimeouts {
                generation: Duration::from_secs(600),
                utility: Duration::from_secs(5),
            },
        }
    }

    /// Overrides bounded process timeouts for controlled child-process tests.
    #[must_use]
    pub fn with_timeouts(mut self, generation: Duration, utility: Duration) -> Self {
        self.timeouts = ProcessTimeouts {
            generation,
            utility,
        };
        self
    }

    #[must_use]
    pub fn ffmpeg_arguments(&self, output: &Path, seconds: u64) -> Vec<OsString> {
        [
            "-y",
            "-hide_banner",
            "-nostdin",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=640x360:rate=24",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=1000:sample_rate=48000",
            "-t",
        ]
        .into_iter()
        .map(OsString::from)
        .chain(std::iter::once(seconds.to_string().into()))
        .chain(
            [
                "-map",
                "0:v:0",
                "-map",
                "1:a:0",
                "-c:v",
                "libvpx-vp9",
                "-deadline",
                "realtime",
                "-cpu-used",
                "4",
                "-b:v",
                "600k",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "libopus",
                "-b:a",
                "64k",
                "-ac",
                "1",
                "-f",
                "webm",
                "-progress",
                "pipe:1",
                "-nostats",
            ]
            .into_iter()
            .map(OsString::from),
        )
        .chain(std::iter::once(output.as_os_str().to_owned()))
        .collect()
    }

    /// Generates VP9/Opus `WebM` using only lavfi sources and drains both pipes via `output`.
    ///
    /// # Errors
    ///
    /// Returns a redacted process error when `FFmpeg` cannot start or exits unsuccessfully.
    pub fn generate_synthetic(
        &self,
        output: &Path,
        seconds: u64,
    ) -> Result<CpuFallbackOutput, CpuFallbackError> {
        if seconds == 0 {
            return Err(CpuFallbackError::InvalidRequestedDuration);
        }
        let headroom = Duration::from_secs(15);
        let generation_timeout = Duration::from_secs(seconds)
            .saturating_add(headroom)
            .min(Duration::from_secs(600))
            .min(self.timeouts.generation);
        let process = collect_child(
            &self.ffmpeg,
            self.ffmpeg_arguments(output, seconds),
            generation_timeout,
            64 * 1024,
            16 * 1024,
        )
        .map_err(|error| CpuFallbackError::from_collection(&error))?;
        if !process.status.success() {
            return Err(CpuFallbackError::Failed);
        }
        Ok(CpuFallbackOutput {
            average_speed: Self::average_speed_from_progress(&process.stdout)?,
            ffmpeg_build_id: self.ffmpeg_build_id()?,
        })
    }

    /// Parses every complete `-progress pipe:1` event and averages its reported speeds.
    ///
    /// # Errors
    ///
    /// Returns a progress error for malformed `FFmpeg` progress output.
    ///
    /// # Panics
    ///
    /// Panics only if a bounded progress buffer somehow contains more than `u32::MAX` events.
    pub fn average_speed_from_progress(progress: &[u8]) -> Result<Option<f64>, ProgressError> {
        let events = ProgressParser::default().push(progress)?;
        let speeds = events
            .iter()
            .filter_map(|event| event.speed)
            .collect::<Vec<_>>();
        let count = u32::try_from(speeds.len()).expect("progress events are bounded by input size");
        Ok((count != 0).then(|| speeds.iter().sum::<f64>() / f64::from(count)))
    }

    /// Returns the first `FFmpeg` version line, which is the permitted build identifier.
    ///
    /// # Errors
    ///
    /// Returns a redacted error if the executable cannot run or prints no identity line.
    pub fn ffmpeg_build_id(&self) -> Result<String, CpuFallbackError> {
        let output = collect_child(
            &self.ffmpeg,
            [OsString::from("-version")],
            self.timeouts.utility,
            8 * 1024,
            8 * 1024,
        )
        .map_err(|error| CpuFallbackError::from_collection(&error))?;
        if !output.status.success() {
            return Err(CpuFallbackError::BuildIdFailed);
        }
        let line = String::from_utf8_lossy(&output.stdout)
            .lines()
            .find(|line| !line.is_empty())
            .map(str::to_owned)
            .filter(|line| line.len() <= 256)
            .ok_or(CpuFallbackError::EmptyBuildId)?;
        Ok(line)
    }

    #[must_use]
    pub fn ffprobe_path(&self) -> &Path {
        &self.ffprobe
    }
}

/// Validated, Gemini-compatible stream information reported by ffprobe.
#[derive(Debug, Clone, PartialEq)]
pub struct FfprobeReport {
    pub container: String,
    pub duration_seconds: f64,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub video_pixel_format: Option<String>,
}

#[derive(Debug, Error)]
pub enum FfprobeError {
    #[error("unable to start ffprobe")]
    Spawn,
    #[error("ffprobe timed out")]
    TimedOut,
    #[error("ffprobe output exceeded its bounded limit")]
    OutputLimit,
    #[error("unable to collect ffprobe output")]
    Collect,
    #[error("ffprobe failed")]
    Failed,
    #[error("ffprobe returned malformed JSON")]
    MalformedJson,
    #[error("output has zero bytes")]
    ZeroBytes,
    #[error("output is not a WebM container")]
    NotWebm,
    #[error("output is missing a required stream")]
    MissingStream,
    #[error("output codecs or pixel format are incompatible")]
    IncompatibleStreams,
    #[error("output duration is not positive")]
    InvalidDuration,
}

impl FfprobeError {
    fn from_collection(error: &ChildCollectionError) -> Self {
        match error {
            ChildCollectionError::Spawn(_) => Self::Spawn,
            ChildCollectionError::Read(_) | ChildCollectionError::Runtime(_) => Self::Collect,
            ChildCollectionError::TimedOut => Self::TimedOut,
            ChildCollectionError::StdoutLimit => Self::OutputLimit,
        }
    }
}

impl FfprobeReport {
    /// Runs the exact bounded ffprobe query and validates its result without retaining logs.
    ///
    /// # Errors
    ///
    /// Returns a redacted error for child failure or invalid media metadata.
    pub fn read(ffprobe: impl AsRef<Path>, output: &Path) -> Result<Self, FfprobeError> {
        let bytes = fs::metadata(output).map_err(|_| FfprobeError::Spawn)?.len();
        let process = collect_child(
            ffprobe.as_ref(),
            [
                "-v",
                "error",
                "-show_entries",
                "format=format_name,duration:stream=codec_name,codec_type,pix_fmt",
                "-of",
                "json",
            ]
            .into_iter()
            .map(OsString::from)
            .chain(std::iter::once(output.as_os_str().to_owned())),
            Duration::from_secs(5),
            64 * 1024,
            16 * 1024,
        )
        .map_err(|error| FfprobeError::from_collection(&error))?;
        Self::from_child_output(process.status.success(), &process.stdout, bytes)
    }

    /// Validates a fully-drained ffprobe child result without exposing child logs.
    ///
    /// # Errors
    ///
    /// Rejects a nonzero child before considering its stdout.
    pub fn from_child_output(
        succeeded: bool,
        stdout: &[u8],
        output_bytes: u64,
    ) -> Result<Self, FfprobeError> {
        if !succeeded {
            return Err(FfprobeError::Failed);
        }
        let json = std::str::from_utf8(stdout).map_err(|_| FfprobeError::MalformedJson)?;
        Self::from_json(json, output_bytes)
    }

    /// Validates an ffprobe JSON reply and a separately observed output size.
    ///
    /// # Errors
    ///
    /// Rejects malformed JSON, invalid container/streams/duration, and zero-byte output.
    pub fn from_json(input: &str, output_bytes: u64) -> Result<Self, FfprobeError> {
        if output_bytes == 0 {
            return Err(FfprobeError::ZeroBytes);
        }
        let value: ProbeDocument =
            serde_json::from_str(input).map_err(|_| FfprobeError::MalformedJson)?;
        if value.format.format_name != "matroska,webm" {
            return Err(FfprobeError::NotWebm);
        }
        let duration_seconds = value
            .format
            .duration
            .parse::<f64>()
            .ok()
            .filter(|duration| duration.is_finite() && *duration > 0.0)
            .ok_or(FfprobeError::InvalidDuration)?;
        let video = value
            .streams
            .iter()
            .find(|stream| stream.codec_type == "video");
        let audio = value
            .streams
            .iter()
            .find(|stream| stream.codec_type == "audio");
        let (Some(video), Some(audio)) = (video, audio) else {
            return Err(FfprobeError::MissingStream);
        };
        if video.codec_name != "vp9"
            || video.pix_fmt.as_deref() != Some("yuv420p")
            || audio.codec_name != "opus"
        {
            return Err(FfprobeError::IncompatibleStreams);
        }
        Ok(Self {
            container: value.format.format_name,
            duration_seconds,
            video_codec: Some(video.codec_name.clone()),
            audio_codec: Some(audio.codec_name.clone()),
            video_pixel_format: video.pix_fmt.clone(),
        })
    }
}

#[derive(Debug, Error)]
enum ChildCollectionError {
    #[error("unable to spawn child")]
    Spawn(#[source] std::io::Error),
    #[error("unable to drain child output")]
    Read(#[source] std::io::Error),
    #[error("child timed out")]
    TimedOut,
    #[error("child stdout exceeded its limit")]
    StdoutLimit,
    #[error("unable to create child collector runtime")]
    Runtime(#[source] std::io::Error),
}

struct ChildOutput {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
}

fn collect_child<I>(
    program: &Path,
    arguments: I,
    timeout: Duration,
    stdout_limit: usize,
    stderr_limit: usize,
) -> Result<ChildOutput, ChildCollectionError>
where
    I: IntoIterator<Item = OsString>,
{
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(ChildCollectionError::Runtime)?;
    runtime.block_on(collect_child_async(
        program.to_owned(),
        arguments.into_iter().collect(),
        timeout,
        stdout_limit,
        stderr_limit,
    ))
}

async fn collect_child_async(
    program: PathBuf,
    arguments: Vec<OsString>,
    timeout: Duration,
    stdout_limit: usize,
    stderr_limit: usize,
) -> Result<ChildOutput, ChildCollectionError> {
    let mut child = tokio::process::Command::new(program)
        .args(arguments)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(ChildCollectionError::Spawn)?;
    let stdout = child.stdout.take().ok_or_else(missing_pipe)?;
    let stderr = child.stderr.take().ok_or_else(missing_pipe)?;
    let stdout_task = tokio::spawn(drain_pipe(stdout, stdout_limit, true));
    let stderr_task = tokio::spawn(drain_pipe(stderr, stderr_limit, false));
    let status = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(error)) => return Err(ChildCollectionError::Read(error)),
        Err(_) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            stdout_task.abort();
            stderr_task.abort();
            let _ = stdout_task.await;
            let _ = stderr_task.await;
            return Err(ChildCollectionError::TimedOut);
        }
    };
    let stdout = stdout_task
        .await
        .map_err(|error| join_error(&error))?
        .map_err(ChildCollectionError::Read)?;
    let _stderr = stderr_task
        .await
        .map_err(|error| join_error(&error))?
        .map_err(ChildCollectionError::Read)?;
    if stdout.truncated {
        return Err(ChildCollectionError::StdoutLimit);
    }
    Ok(ChildOutput {
        status,
        stdout: stdout.bytes,
    })
}

fn missing_pipe() -> ChildCollectionError {
    ChildCollectionError::Read(std::io::Error::other("child pipe was unavailable"))
}

fn join_error(error: &tokio::task::JoinError) -> ChildCollectionError {
    ChildCollectionError::Read(std::io::Error::other(error.to_string()))
}

struct DrainedPipe {
    bytes: Vec<u8>,
    truncated: bool,
}

async fn drain_pipe<R>(mut reader: R, limit: usize, retain: bool) -> std::io::Result<DrainedPipe>
where
    R: AsyncRead + Unpin,
{
    let mut bytes = Vec::with_capacity(limit);
    let mut buffer = [0_u8; 8 * 1024];
    let mut truncated = false;
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        if retain {
            let remaining = limit.saturating_sub(bytes.len());
            let copied = remaining.min(read);
            bytes.extend_from_slice(&buffer[..copied]);
            truncated |= copied != read;
        }
    }
    Ok(DrainedPipe { bytes, truncated })
}

#[derive(serde::Deserialize)]
struct ProbeDocument {
    format: ProbeFormat,
    streams: Vec<ProbeStream>,
}

#[derive(serde::Deserialize)]
struct ProbeFormat {
    format_name: String,
    duration: String,
}

#[derive(serde::Deserialize)]
struct ProbeStream {
    codec_name: String,
    codec_type: String,
    pix_fmt: Option<String>,
}

#[must_use]
pub fn content_sha256_bytes(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        write!(result, "{byte:02x}").expect("writing into a String cannot fail");
    }
    result
}

/// Prevents callers from accidentally carrying child paths or raw process output into evidence.
#[must_use]
pub fn redacted_process_detail(_raw: &str) -> Option<String> {
    None
}
