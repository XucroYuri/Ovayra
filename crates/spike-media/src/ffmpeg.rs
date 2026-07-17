use std::{ffi::OsString, fmt::Write as _, path::PathBuf, process::Stdio, time::Duration};

use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
};

use crate::capability::{Inventory, InventoryCommand, InventoryError, InventoryOutput};

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
    #[error("FFmpeg child exceeded its bounded execution time")]
    TimedOut,
}

/// Errors while gathering the complete `FFmpeg` capability inventory.
#[derive(Debug, Error)]
pub enum InventoryCollectionError {
    #[error("failed to execute required inventory command")]
    Runner(#[source] FfmpegError),
    #[error("required inventory command did not exit successfully: {command:?}")]
    FailedCommand { command: InventoryCommand },
    #[error("invalid complete inventory")]
    Inventory(#[source] InventoryError),
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
        self.run_os_with_timeout(
            args.iter().map(OsString::from).collect(),
            Duration::from_secs(30),
        )
        .await
    }

    /// Executes a path-safe argument list with a bounded timeout and child cleanup.
    ///
    /// # Errors
    ///
    /// Returns a typed redacted child-process error, including timeout after kill-and-reap.
    pub async fn run_os_with_timeout(
        &self,
        args: Vec<OsString>,
        timeout: Duration,
    ) -> Result<(Vec<u8>, FfmpegEvidence), FfmpegError> {
        let mut child = Command::new(&self.executable)
            .args(COMMON_ARGS)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(FfmpegError::Spawn)?;
        let Some(stdout) = child.stdout.take() else {
            terminate_and_reap(&mut child).await;
            return Err(FfmpegError::MissingOutputPipe);
        };
        let Some(stderr) = child.stderr.take() else {
            terminate_and_reap(&mut child).await;
            return Err(FfmpegError::MissingOutputPipe);
        };
        let joined = Box::pin(tokio::time::timeout(timeout, async {
            tokio::try_join!(read_all(stdout), read_stderr_capped(stderr), child.wait(),)
        }))
        .await;
        let (stdout, stderr, status) = match joined {
            Ok(Ok(output)) => output,
            Ok(Err(error)) => {
                terminate_and_reap(&mut child).await;
                return Err(FfmpegError::Output(error));
            }
            Err(_) => {
                terminate_and_reap(&mut child).await;
                return Err(FfmpegError::TimedOut);
            }
        };
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

    /// Executes each required inventory command exactly once and accepts only six successful results.
    ///
    /// # Errors
    ///
    /// Returns an error for child-process failures, non-zero inventory exits, or incomplete inventory.
    pub async fn collect_inventory(&self) -> Result<Inventory, InventoryCollectionError> {
        let mut outputs = Vec::with_capacity(InventoryCommand::ALL.len());
        for command in InventoryCommand::ALL {
            let args = command.args();
            let (stdout, evidence) = self
                .run(&args)
                .await
                .map_err(InventoryCollectionError::Runner)?;
            if evidence.exit_code != Some(0) {
                return Err(InventoryCollectionError::FailedCommand { command });
            }
            outputs.push(InventoryOutput::success(
                command,
                String::from_utf8_lossy(&stdout),
            ));
        }
        Inventory::from_command_outputs(&outputs).map_err(InventoryCollectionError::Inventory)
    }
}

async fn terminate_and_reap(child: &mut tokio::process::Child) {
    let _ = child.start_kill();
    let _ = child.wait().await;
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
    // Diagnostics commonly contain paths, URLs, and user names; retain only line count.
    String::from_utf8_lossy(stderr)
        .lines()
        .map(|_| "[redacted-diagnostic]")
        .collect::<Vec<_>>()
        .join("\n")
        .into_bytes()
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};
    use tokio::io::{AsyncWriteExt, duplex};

    use super::{COMMON_ARGS, STDERR_LIMIT, read_all, read_stderr_capped, redact_stderr};

    #[test]
    fn common_arguments_are_exact_and_precede_operation_arguments() {
        let operation = ["-version"];
        let actual: Vec<_> = COMMON_ARGS
            .iter()
            .chain(operation.iter())
            .copied()
            .collect();
        assert_eq!(
            actual,
            [
                "-hide_banner",
                "-nostdin",
                "-nostats",
                "-progress",
                "pipe:1",
                "-version"
            ]
        );
    }

    #[tokio::test]
    async fn stdout_and_capped_stderr_are_read_as_separate_streams_without_deadlock() {
        let (mut stderr_writer, stderr_reader) = duplex(1024);
        let stderr = vec![b'e'; STDERR_LIMIT + 8 * 1024];
        let writer = tokio::spawn(async move {
            stderr_writer.write_all(&stderr).await.unwrap();
            stderr_writer.shutdown().await.unwrap();
        });
        let retained = read_stderr_capped(stderr_reader).await.unwrap();
        writer.await.unwrap();
        assert_eq!(retained.len(), STDERR_LIMIT);

        let (mut stdout_writer, stdout_reader) = duplex(64);
        stdout_writer
            .write_all(b"progress=continue\n")
            .await
            .unwrap();
        stdout_writer.shutdown().await.unwrap();
        assert_eq!(
            read_all(stdout_reader).await.unwrap(),
            b"progress=continue\n"
        );
    }

    #[test]
    fn stderr_is_redacted_before_its_evidence_hash_is_computed() {
        let redacted = redact_stderr(b"ordinary diagnostic\n/path/that/must/not/escape\n");
        assert_eq!(redacted, b"[redacted-diagnostic]\n[redacted-diagnostic]");
        assert_ne!(
            Sha256::digest(&redacted),
            Sha256::digest(b"ordinary diagnostic\n/path/that/must/not/escape\n")
        );
    }

    #[test]
    fn stderr_redaction_removes_private_names_paths_and_urls_before_hashing() {
        let first = redact_stderr(b"private-video.mp4: No such file\n'https://example.test/a?token=one'\nC:\\Users\\me\\secret.mov\n~/hidden.wav\n");
        let second = redact_stderr(b"other-video.mp4: No such file\n'https://example.test/a?token=two'\nD:\\Users\\you\\other.mov\n~/different.wav\n");
        for forbidden in [
            b"private-video.mp4".as_slice(),
            b"example.test",
            b"token=one",
            b"C:\\Users",
            b"hidden.wav",
        ] {
            assert!(
                !first
                    .windows(forbidden.len())
                    .any(|window| window == forbidden)
            );
        }
        assert_eq!(first, second);
    }

    #[cfg(unix)]
    fn script(
        log: &std::path::Path,
        fail_decoders: bool,
    ) -> (tempfile::TempDir, std::path::PathBuf) {
        use std::{fs, os::unix::fs::PermissionsExt};

        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("fake-ffmpeg");
        let failure = if fail_decoders { "exit 7" } else { "exit 0" };
        fs::write(
            &executable,
            format!("#!/bin/sh\nprintf '%s ' \"$@\" >> {}\nprintf '\\n' >> {}\nlast=\"\"\nfor value in \"$@\"; do last=\"$value\"; done\nprintf 'stdout:%s\\n' \"$last\"\nprintf 'private-video.mp4\\n' >&2\nif [ \"$last\" = \"-decoders\" ]; then {}; fi\nif [ \"$last\" = \"--emit-large\" ]; then dd if=/dev/zero bs=1024 count=1100 1>&2 2>/dev/null; fi\n", log.display(), log.display(), failure),
        ).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        (directory, executable)
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn runner_child_process_keeps_streams_separate_and_collects_all_inventory_commands() {
        use std::fs;

        let log_directory = tempfile::tempdir().unwrap();
        let log = log_directory.path().join("arguments.log");
        let (_script_directory, executable) = script(&log, false);
        let runner = super::FfmpegRunner::new(executable);
        let (stdout, evidence) = runner.run(&["--emit-large"]).await.unwrap();
        assert_eq!(stdout, b"stdout:--emit-large\n");
        assert_eq!(evidence.exit_code, Some(0));
        assert_eq!(evidence.stderr_sha256.len(), 64);
        runner.collect_inventory().await.unwrap();

        let log_contents = fs::read_to_string(log).unwrap();
        let lines: Vec<_> = log_contents.lines().collect();
        assert_eq!(
            lines[0],
            "-hide_banner -nostdin -nostats -progress pipe:1 --emit-large "
        );
        let inventory: Vec<_> = lines.into_iter().skip(1).collect();
        assert_eq!(inventory.len(), 6);
        for command in [
            "-version",
            "-buildconf",
            "-hwaccels",
            "-decoders",
            "-encoders",
            "-filters",
        ] {
            assert_eq!(
                inventory
                    .iter()
                    .filter(|line| line.ends_with(&format!("{command} ")))
                    .count(),
                1
            );
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn nonzero_inventory_child_output_cannot_produce_inventory() {
        let log_directory = tempfile::tempdir().unwrap();
        let (_script_directory, executable) =
            script(&log_directory.path().join("arguments.log"), true);
        let runner = super::FfmpegRunner::new(executable);
        assert!(runner.collect_inventory().await.is_err());
    }
}
