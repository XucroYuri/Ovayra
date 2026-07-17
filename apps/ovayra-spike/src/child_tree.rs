use std::{
    io::{self, Write},
    process::{Command, Stdio},
    thread,
    time::Duration,
};

use anyhow::{Context, Result};
use serde_json::json;

#[allow(clippy::struct_excessive_bools)] // Hidden probe flags intentionally compose fault modes.
#[derive(Clone, Copy)]
pub(crate) struct ChildTreeOptions {
    pub(crate) malformed_report: bool,
    pub(crate) delay_report: bool,
    pub(crate) exit_before_report: bool,
    pub(crate) hold_stderr: bool,
}

pub(crate) fn run_child_tree(options: ChildTreeOptions) -> Result<()> {
    if options.exit_before_report {
        return Ok(());
    }
    let executable = std::env::current_exe().context("unable to locate child-tree executable")?;
    let mut command = Command::new(executable);
    command
        .arg("child-leaf")
        .stdin(Stdio::null())
        .stdout(Stdio::null());
    if options.hold_stderr {
        command.stderr(Stdio::inherit());
    } else {
        command.stderr(Stdio::null());
    }
    let leaf = command.spawn().context("unable to spawn child-tree leaf")?;

    if options.delay_report {
        thread::sleep(Duration::from_secs(60));
        return Ok(());
    }
    let mut stdout = io::stdout().lock();
    if options.malformed_report {
        stdout
            .write_all(b"not-json\n")
            .context("unable to write malformed child-tree report")?;
    } else {
        let report = json!({
            "parent_pid": std::process::id(),
            "grandchild_pid": leaf.id(),
        });
        serde_json::to_writer(&mut stdout, &report)
            .context("unable to serialize child-tree report")?;
        stdout
            .write_all(b"\n")
            .context("unable to terminate child-tree report")?;
    }
    stdout
        .flush()
        .context("unable to flush child-tree report")?;
    thread::sleep(Duration::from_secs(60));
    Ok(())
}

pub(crate) fn run_child_leaf() {
    thread::sleep(Duration::from_secs(60));
}
