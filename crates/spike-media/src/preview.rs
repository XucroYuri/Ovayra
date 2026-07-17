//! A bounded raw-RGBA preview reader and latest-frame transport.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use thiserror::Error;
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::{Child, Command},
};

/// The fixed preview dimensions requested from `FFmpeg`.
pub const PREVIEW_WIDTH: usize = 640;
/// The fixed preview dimensions requested from `FFmpeg`.
pub const PREVIEW_HEIGHT: usize = 360;
/// The exact number of bytes in one preview frame.
pub const PREVIEW_FRAME_BYTES: usize = PREVIEW_WIDTH * PREVIEW_HEIGHT * 4;
const STDERR_RETAIN_LIMIT: usize = 64 * 1024;

/// A validated RGBA frame, stamped when it enters the preview bridge.
#[derive(Debug)]
pub struct Frame {
    width: usize,
    height: usize,
    bytes: Vec<u8>,
    sequence: u64,
    enqueued_at: Instant,
}

impl Frame {
    /// Builds a frame only when `bytes` is exactly `width * height * 4` bytes.
    ///
    /// # Errors
    ///
    /// Returns an error for overflowing dimensions or an incorrect byte length.
    pub fn rgba(
        width: usize,
        height: usize,
        bytes: Vec<u8>,
        sequence: u64,
    ) -> Result<Self, FrameError> {
        let expected = width
            .checked_mul(height)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or(FrameError::DimensionOverflow { width, height })?;
        if bytes.len() != expected {
            return Err(FrameError::IncorrectByteCount {
                expected,
                actual: bytes.len(),
            });
        }
        Ok(Self {
            width,
            height,
            bytes,
            sequence,
            enqueued_at: Instant::now(),
        })
    }

    #[must_use]
    pub fn width(&self) -> usize {
        self.width
    }

    #[must_use]
    pub fn height(&self) -> usize {
        self.height
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[must_use]
    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    #[must_use]
    pub fn enqueued_at(&self) -> Instant {
        self.enqueued_at
    }

    #[must_use]
    pub fn enqueue_latency(&self) -> Duration {
        self.enqueued_at.elapsed()
    }
}

/// Validation errors for a [`Frame`].
#[derive(Debug, Error, PartialEq, Eq)]
pub enum FrameError {
    #[error("RGBA dimensions overflow byte-count calculation: {width}x{height}")]
    DimensionOverflow { width: usize, height: usize },
    #[error("RGBA byte count must be exactly {expected}, received {actual}")]
    IncorrectByteCount { expected: usize, actual: usize },
}

/// A coalescing transport which holds at most one unapplied frame.
///
/// The frame storage is deliberately exactly an `Arc<Mutex<Option<Frame>>>`: publishing a
/// replacement never grows a queue. The separate atomic records each replacement.
#[derive(Debug, Clone)]
pub struct LatestFrame {
    frame: Arc<Mutex<Option<Frame>>>,
    dropped: Arc<AtomicU64>,
}

impl Default for LatestFrame {
    fn default() -> Self {
        Self {
            frame: Arc::new(Mutex::new(None)),
            dropped: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl LatestFrame {
    /// Replaces a pending frame and records a drop if one was waiting.
    ///
    /// # Panics
    ///
    /// Panics if another thread poisoned the frame mutex.
    pub fn publish(&self, frame: Frame) {
        let mut pending = self.frame.lock().expect("latest-frame mutex poisoned");
        if pending.replace(frame).is_some() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Removes the newest pending frame, if any.
    ///
    /// # Panics
    ///
    /// Panics if another thread poisoned the frame mutex.
    #[must_use]
    pub fn take(&self) -> Option<Frame> {
        self.frame
            .lock()
            .expect("latest-frame mutex poisoned")
            .take()
    }

    /// Reports whether the single slot has no pending frame.
    ///
    /// # Panics
    ///
    /// Panics if another thread poisoned the frame mutex.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.frame
            .lock()
            .expect("latest-frame mutex poisoned")
            .is_none()
    }

    #[must_use]
    pub fn dropped_frames(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

/// Runs `FFmpeg` in raw-RGBA preview mode.
#[derive(Debug, Clone)]
pub struct FfmpegPreview {
    executable: PathBuf,
}

impl FfmpegPreview {
    #[must_use]
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
        }
    }

    /// The fixed command line used for preview decoding.
    #[must_use]
    pub fn arguments(&self, input: &Path) -> Vec<OsString> {
        [
            "-hide_banner",
            "-nostdin",
            "-re",
            "-stream_loop",
            "-1",
            "-i",
        ]
        .into_iter()
        .map(OsString::from)
        .chain(std::iter::once(input.as_os_str().to_os_string()))
        .chain(
            [
                "-an",
                "-vf",
                "scale=640:360,fps=24",
                "-pix_fmt",
                "rgba",
                "-f",
                "rawvideo",
                "pipe:1",
            ]
            .into_iter()
            .map(OsString::from),
        )
        .collect()
    }

    /// Streams frames to the one-slot transport.
    ///
    /// This is a convenience wrapper around [`Self::stream_with`]. A Slint frame bridge
    /// should use `stream_with` and call its scheduling `publish` method in the callback.
    ///
    /// # Errors
    ///
    /// Returns an error when the child cannot run, is cancelled, times out, or does not emit
    /// complete raw RGBA frames.
    pub async fn stream(
        &self,
        input: &Path,
        latest: LatestFrame,
        cancelled: Arc<AtomicBool>,
        timeout: Duration,
    ) -> Result<PreviewRun, PreviewError> {
        self.stream_with(input, cancelled, timeout, move |frame| {
            latest.publish(frame);
        })
        .await
    }

    /// Streams frames to a background callback without introducing a frame queue.
    ///
    /// The callback is invoked synchronously by this worker task for every complete frame.
    /// It should return promptly; UI work belongs in a separately scheduled main-thread task.
    ///
    /// # Errors
    ///
    /// Returns an error when the child cannot run, is cancelled, times out, or does not emit
    /// complete raw RGBA frames.
    pub async fn stream_with<F>(
        &self,
        input: &Path,
        cancelled: Arc<AtomicBool>,
        timeout: Duration,
        mut on_frame: F,
    ) -> Result<PreviewRun, PreviewError>
    where
        F: FnMut(Frame) + Send,
    {
        let mut child = Command::new(&self.executable)
            .args(self.arguments(input))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(PreviewError::Spawn)?;
        let Some(stdout) = child.stdout.take() else {
            terminate_and_reap(&mut child).await;
            return Err(PreviewError::MissingOutputPipe);
        };
        let Some(stderr) = child.stderr.take() else {
            terminate_and_reap(&mut child).await;
            return Err(PreviewError::MissingErrorPipe);
        };
        let stderr_task = tokio::spawn(drain_stderr(stderr));

        let streamed = tokio::time::timeout(
            timeout,
            read_and_wait(&mut child, stdout, &cancelled, &mut on_frame),
        )
        .await;
        let frames_read = match streamed {
            Ok(Ok(frames_read)) => frames_read,
            Ok(Err(error)) => {
                terminate_and_reap(&mut child).await;
                let _ = stderr_task.await;
                return Err(error);
            }
            Err(_) => {
                terminate_and_reap(&mut child).await;
                let _ = stderr_task.await;
                return Err(PreviewError::TimedOut);
            }
        };
        let stderr_bytes = stderr_task.await.map_err(|_| PreviewError::StderrTask)??;
        Ok(PreviewRun {
            frames_read,
            stderr_bytes,
        })
    }
}

/// Counters collected while a preview stream runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewRun {
    pub frames_read: u64,
    pub stderr_bytes: usize,
}

/// A preview stream error. It intentionally contains no raw stderr or source path.
#[derive(Debug, Error)]
pub enum PreviewError {
    #[error("failed to spawn FFmpeg preview process")]
    Spawn(#[source] std::io::Error),
    #[error("FFmpeg preview did not provide stdout")]
    MissingOutputPipe,
    #[error("FFmpeg preview did not provide stderr")]
    MissingErrorPipe,
    #[error("failed to read FFmpeg preview output")]
    Output(#[source] std::io::Error),
    #[error("FFmpeg preview ended with a partial frame of {received} bytes")]
    PartialFrame { received: usize },
    #[error("FFmpeg preview exceeded its bounded execution time")]
    TimedOut,
    #[error("FFmpeg preview was cancelled")]
    Cancelled,
    #[error("FFmpeg preview exited unsuccessfully")]
    Failed,
    #[error("FFmpeg preview stderr task failed")]
    StderrTask,
    #[error(transparent)]
    Frame(#[from] FrameError),
}

async fn read_and_wait<F>(
    child: &mut Child,
    mut stdout: impl AsyncRead + Unpin,
    cancelled: &AtomicBool,
    on_frame: &mut F,
) -> Result<u64, PreviewError>
where
    F: FnMut(Frame),
{
    let mut sequence = 0_u64;
    loop {
        let Some(bytes) = read_frame(&mut stdout, cancelled).await? else {
            break;
        };
        sequence = sequence.saturating_add(1);
        on_frame(Frame::rgba(PREVIEW_WIDTH, PREVIEW_HEIGHT, bytes, sequence)?);
    }
    drop(stdout);
    let status = tokio::select! {
        status = child.wait() => status.map_err(PreviewError::Output)?,
        () = wait_for_cancellation(cancelled) => return Err(PreviewError::Cancelled),
    };
    if status.success() {
        Ok(sequence)
    } else {
        Err(PreviewError::Failed)
    }
}

async fn read_frame(
    reader: &mut (impl AsyncRead + Unpin),
    cancelled: &AtomicBool,
) -> Result<Option<Vec<u8>>, PreviewError> {
    let mut frame = vec![0_u8; PREVIEW_FRAME_BYTES];
    let mut received = 0_usize;
    while received < frame.len() {
        let read = tokio::select! {
            read = reader.read(&mut frame[received..]) => read.map_err(PreviewError::Output)?,
            () = wait_for_cancellation(cancelled) => return Err(PreviewError::Cancelled),
        };
        if read == 0 {
            return if received == 0 {
                Ok(None)
            } else {
                Err(PreviewError::PartialFrame { received })
            };
        }
        received += read;
    }
    Ok(Some(frame))
}

async fn wait_for_cancellation(cancelled: &AtomicBool) {
    while !cancelled.load(Ordering::Acquire) {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn drain_stderr(mut stderr: impl AsyncRead + Unpin) -> Result<usize, PreviewError> {
    let mut retained = 0_usize;
    let mut buffer = [0_u8; 8192];
    loop {
        let read = stderr
            .read(&mut buffer)
            .await
            .map_err(PreviewError::Output)?;
        if read == 0 {
            return Ok(retained);
        }
        retained = retained.saturating_add(read).min(STDERR_RETAIN_LIMIT);
    }
}

async fn terminate_and_reap(child: &mut Child) {
    let _ = child.start_kill();
    let _ = child.wait().await;
}
