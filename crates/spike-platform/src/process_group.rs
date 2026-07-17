use std::{io, process::Stdio, time::Duration};

use command_group::{AsyncCommandGroup, AsyncGroupChild};
use serde::Deserialize;
use sysinfo::{Pid, ProcessesToUpdate, System};
use thiserror::Error;
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::{ChildStdout, Command},
    task::JoinHandle,
    time::timeout,
};

const MAX_REPORT_BYTES: usize = 4 * 1024;
const MAX_STDERR_BYTES: usize = 16 * 1024;
const ERROR_CLEANUP_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessIdentity {
    pid: u32,
    start_time: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessTree {
    parent: ProcessIdentity,
    grandchild: ProcessIdentity,
}

#[derive(Debug, Error)]
pub enum ProcessGroupError {
    #[error("unable to spawn process group: {0}")]
    Spawn(#[source] io::Error),
    #[error("child process did not expose a PID")]
    MissingPid,
    #[error("child process did not expose stdout")]
    MissingStdout,
    #[error("timed out waiting for child-tree JSON report")]
    ReportTimeout,
    #[error("child-tree JSON report exceeded {MAX_REPORT_BYTES} bytes")]
    ReportTooLarge,
    #[error("unable to read child-tree JSON report: {0}")]
    ReportRead(#[source] io::Error),
    #[error("invalid child-tree JSON report: {0}")]
    InvalidReport(#[source] serde_json::Error),
    #[error("reported process {0} was not observable")]
    UnobservablePid(u32),
    #[error("timed out while killing and reaping process group")]
    CleanupTimeout,
    #[error("unable to kill and reap process group: {0}")]
    Cleanup(#[source] io::Error),
    #[error("unable to capture bounded child stderr: {0}")]
    Stderr(#[source] io::Error),
}

/// A command-group child that always kills its whole process group/job object on cleanup.
pub struct GroupedProcess {
    child: Option<AsyncGroupChild>,
    stdout: Option<ChildStdout>,
    stderr_capture: Option<JoinHandle<io::Result<Vec<u8>>>>,
    leader: ProcessIdentity,
}

impl GroupedProcess {
    /// Starts `program` in a new process group (or Windows job object).
    ///
    /// # Errors
    ///
    /// Returns an error when the command cannot be spawned, lacks captured stdout, or its leader
    /// cannot be observed to establish a PID-reuse-resistant identity.
    #[allow(clippy::unused_async)] // Public lifecycle API is intentionally awaitable with its other operations.
    pub async fn spawn(program: &str, args: &[&str]) -> Result<Self, ProcessGroupError> {
        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.group_spawn().map_err(ProcessGroupError::Spawn)?;
        let pid = child.id().ok_or(ProcessGroupError::MissingPid)?;
        let leader =
            ProcessTreeProbe::identity(pid).ok_or(ProcessGroupError::UnobservablePid(pid))?;
        let stdout = child
            .inner()
            .stdout
            .take()
            .ok_or(ProcessGroupError::MissingStdout)?;
        let stderr_capture = child.inner().stderr.take().map(|stderr| {
            tokio::spawn(async move { read_bounded_and_drain(stderr, MAX_STDERR_BYTES).await })
        });

        Ok(Self {
            child: Some(child),
            stdout: Some(stdout),
            stderr_capture,
            leader,
        })
    }

    #[must_use]
    pub fn leader(&self) -> ProcessIdentity {
        self.leader.clone()
    }

    /// Reads exactly one bounded JSON report and captures identities for both live processes.
    ///
    /// # Errors
    ///
    /// Returns an error for timeout, malformed or oversized reports, or unobservable PIDs. Each
    /// such failure terminates and reaps the group before the error is returned when possible.
    pub async fn wait_for_reported_tree(
        &mut self,
        wait: Duration,
    ) -> Result<ProcessTree, ProcessGroupError> {
        let report = match self.read_report(wait).await {
            Ok(report) => report,
            Err(error) => return Err(self.with_cleanup(error).await),
        };
        let Some(parent) = ProcessTreeProbe::identity(report.parent_pid) else {
            return Err(self
                .with_cleanup(ProcessGroupError::UnobservablePid(report.parent_pid))
                .await);
        };
        let Some(grandchild) = ProcessTreeProbe::identity(report.grandchild_pid) else {
            return Err(self
                .with_cleanup(ProcessGroupError::UnobservablePid(report.grandchild_pid))
                .await);
        };
        Ok(ProcessTree { parent, grandchild })
    }

    /// Kills the entire process group/job object and waits at most `wait` for reaping.
    ///
    /// # Errors
    ///
    /// Returns an error when group termination, reaping, or bounded stderr collection fails, or
    /// when the timeout expires. The drop guard still requests termination on every error path.
    pub async fn kill_and_wait(&mut self, wait: Duration) -> Result<(), ProcessGroupError> {
        let Some(child) = self.child.as_mut() else {
            return Ok(());
        };
        let result = timeout(wait, async {
            match child.kill().await {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == io::ErrorKind::InvalidInput => {
                    child.wait().await.map(|_| ())
                }
                Err(error) => Err(error),
            }
        })
        .await;
        match result {
            Ok(Ok(())) => {
                self.child.take();
                self.collect_stderr().await
            }
            Ok(Err(error)) => Err(ProcessGroupError::Cleanup(error)),
            Err(_) => Err(ProcessGroupError::CleanupTimeout),
        }
    }

    async fn read_report(&mut self, wait: Duration) -> Result<ChildTreeReport, ProcessGroupError> {
        let stdout = self
            .stdout
            .as_mut()
            .ok_or(ProcessGroupError::MissingStdout)?;
        let bytes = timeout(wait, read_one_bounded_line(stdout))
            .await
            .map_err(|_| ProcessGroupError::ReportTimeout)??;
        serde_json::from_slice(&bytes).map_err(ProcessGroupError::InvalidReport)
    }

    async fn with_cleanup(&mut self, error: ProcessGroupError) -> ProcessGroupError {
        match self.kill_and_wait(ERROR_CLEANUP_TIMEOUT).await {
            Ok(()) => error,
            Err(cleanup) => cleanup,
        }
    }

    async fn collect_stderr(&mut self) -> Result<(), ProcessGroupError> {
        let Some(capture) = self.stderr_capture.take() else {
            return Ok(());
        };
        let _ = capture
            .await
            .map_err(|error| {
                ProcessGroupError::Stderr(io::Error::other(format!("stderr task failed: {error}")))
            })?
            .map_err(ProcessGroupError::Stderr)?;
        Ok(())
    }
}

impl Drop for GroupedProcess {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        if child.start_kill().is_err() {
            return;
        }
        // Drop cannot await. Keep the handle alive in a short-lived runtime so command-group
        // reaps every child in the Unix group or Windows job object.
        let _ = std::thread::Builder::new()
            .name("ovayra-process-reaper".to_owned())
            .spawn(move || {
                if let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    let _ = runtime.block_on(child.wait());
                }
            });
    }
}

pub struct ProcessTreeProbe;

impl ProcessTreeProbe {
    #[must_use]
    pub fn any_alive(tree: &ProcessTree) -> bool {
        Self::is_alive(&tree.parent) || Self::is_alive(&tree.grandchild)
    }

    #[must_use]
    pub fn is_alive(identity: &ProcessIdentity) -> bool {
        let Some(observed) = Self::identity(identity.pid) else {
            return false;
        };
        observed.start_time == identity.start_time
    }

    fn identity(pid: u32) -> Option<ProcessIdentity> {
        let pid = Pid::from_u32(pid);
        let mut system = System::new();
        system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
        system.process(pid).map(|process| ProcessIdentity {
            pid: pid.as_u32(),
            start_time: Some(process.start_time()),
        })
    }
}

#[derive(Deserialize)]
struct ChildTreeReport {
    parent_pid: u32,
    grandchild_pid: u32,
}

async fn read_one_bounded_line(reader: &mut ChildStdout) -> Result<Vec<u8>, ProcessGroupError> {
    let mut line = Vec::with_capacity(256);
    loop {
        let mut byte = [0_u8; 1];
        let count = reader
            .read(&mut byte)
            .await
            .map_err(ProcessGroupError::ReportRead)?;
        if count == 0 {
            return Err(ProcessGroupError::InvalidReport(serde_json::Error::io(
                io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "child-tree report ended before newline",
                ),
            )));
        }
        if byte[0] == b'\n' {
            return Ok(line);
        }
        if line.len() == MAX_REPORT_BYTES {
            return Err(ProcessGroupError::ReportTooLarge);
        }
        line.push(byte[0]);
    }
}

async fn read_bounded_and_drain(
    mut reader: impl AsyncRead + Unpin,
    limit: usize,
) -> io::Result<Vec<u8>> {
    let mut captured = Vec::with_capacity(limit);
    let mut buffer = [0_u8; 1024];
    loop {
        let count = reader.read(&mut buffer).await?;
        if count == 0 {
            return Ok(captured);
        }
        let remaining = limit.saturating_sub(captured.len());
        captured.extend_from_slice(&buffer[..count.min(remaining)]);
    }
}
