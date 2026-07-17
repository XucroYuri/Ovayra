use std::{
    io::{self, Write},
    process::{Command, Stdio},
    thread,
    time::Duration,
};

use anyhow::{Context, Result};
use serde_json::json;

pub(crate) fn run_child_tree(
    malformed_report: bool,
    delay_report: bool,
    exit_before_report: bool,
) -> Result<()> {
    if exit_before_report {
        return Ok(());
    }
    let executable = std::env::current_exe().context("unable to locate child-tree executable")?;
    let leaf = Command::new(executable)
        .arg("child-leaf")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("unable to spawn child-tree leaf")?;

    if delay_report {
        thread::sleep(Duration::from_secs(60));
        return Ok(());
    }
    let mut stdout = io::stdout().lock();
    if malformed_report {
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
