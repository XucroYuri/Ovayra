use std::{fmt::Write as _, path::PathBuf, process::Stdio};

use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
};

/// Arguments attached to every `FFmpeg` invocation, including probes and self-tests.
pub const COMMON_ARGS: &[&str] = &[
    "-hide_banner",
    "-nostdin",
    "-nostats",
    "-progress",
    "pipe:1",
];
const STDERR_LIMIT: usize = 1024 * 1024;

/// Minimal evidence retained from a child invocation. It intentionally holds no command or path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfmpegEvidence {
    pub exit_code: Option<i32>,
    pub stderr_sha256: String,
}

#[derive(Debug, Error)]
pub enum FfmpegError {
    #[error("failed to spawn FFmpeg child process")]
    Spawn(#[source] std::io::Error),
    #[error("failed to read FFmpeg child output")]
    Output(#[source] std::io::Error),
    #[error("FFmpeg child did not provide its configured output pipe")]
    MissingOutputPipe,
}

/// Runs only the `FFmpeg` CLI; this crate never links `FFmpeg` libraries.
#[derive(Debug, Clone)]
pub struct FfmpegRunner {
    executable: PathBuf,
}

impl Default for FfmpegRunner {
    fn default() -> Self {
        Self::new("ffmpeg")
    }
}

impl FfmpegRunner {
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
        }
    }

    /// Executes `FFmpeg` with stdout isolated for progress parsing and bounded stderr evidence.
    ///
    /// # Errors
    ///
    /// Returns an error if the child process cannot be spawned or its output cannot be collected.
    pub async fn run(&self, args: &[&str]) -> Result<(Vec<u8>, FfmpegEvidence), FfmpegError> {
        let mut child = Command::new(&self.executable)
            .args(COMMON_ARGS)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(FfmpegError::Spawn)?;
        let stdout = child.stdout.take().ok_or(FfmpegError::MissingOutputPipe)?;
        let stderr = child.stderr.take().ok_or(FfmpegError::MissingOutputPipe)?;
        let (stdout, stderr, status) =
            tokio::try_join!(read_all(stdout), read_stderr_capped(stderr), child.wait(),)
                .map_err(FfmpegError::Output)?;
        let redacted = redact_stderr(&stderr);
        let digest = Sha256::digest(redacted);
        let mut stderr_sha256 = String::with_capacity(64);
        for byte in digest {
            write!(&mut stderr_sha256, "{byte:02x}").expect("writing into a String cannot fail");
        }
        let evidence = FfmpegEvidence {
            exit_code: status.code(),
            stderr_sha256,
        };
        Ok((stdout, evidence))
    }
}

async fn read_all(mut reader: impl AsyncRead + Unpin) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).await?;
    Ok(bytes)
}

async fn read_stderr_capped(mut reader: impl AsyncRead + Unpin) -> std::io::Result<Vec<u8>> {
    let mut retained = Vec::with_capacity(STDERR_LIMIT);
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            return Ok(retained);
        }
        let remaining = STDERR_LIMIT.saturating_sub(retained.len());
        retained.extend_from_slice(&buffer[..read.min(remaining)]);
    }
}

fn redact_stderr(stderr: &[u8]) -> Vec<u8> {
    // Hash only a conservative normalized representation, never retain raw diagnostics.
    String::from_utf8_lossy(stderr)
        .lines()
        .map(|line| {
            if line.contains('/') || line.contains('\\') {
                "[redacted-path]"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        .into_bytes()
}
