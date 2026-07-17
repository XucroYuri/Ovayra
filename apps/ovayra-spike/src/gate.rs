use std::{
    fmt::Write as _,
    fs,
    io::Write as _,
    path::{Path, PathBuf},
};

use anyhow::Result;
use sha2::{Digest, Sha256};
use spike_contracts::{Evidence, PhaseZeroMatrix};

use crate::evidence_lint;

const MAX_GATE_FILES: usize = 1_024;
const MAX_GATE_DEPTH: usize = 32;

struct SourceEvidence {
    evidence: Evidence,
    sha256: String,
}

/// Runs the final acceptance gate. A successful lint is necessary but never
/// sufficient: all records must also be valid, exact, and complete.
pub(crate) fn run(evidence_dir: &Path, matrix_path: &Path, report_path: &Path) -> Result<()> {
    let Ok(matrix) = PhaseZeroMatrix::load(matrix_path) else {
        return reject(report_path, "FAIL", "matrix validation failed", &[]);
    };
    if evidence_lint::lint_dir_quiet(evidence_dir, false).is_err() {
        return reject(
            report_path,
            "FAIL",
            "evidence lint rejected one or more bounded entries",
            &[],
        );
    }
    let Ok(sources) = read_sources(evidence_dir) else {
        return reject(report_path, "FAIL", "evidence source read failed", &[]);
    };
    let reports = sources
        .iter()
        .map(|source| source.evidence.clone())
        .collect::<Vec<_>>();
    match matrix.evaluate(&reports) {
        Ok(()) => {
            write_report_atomic(report_path, &render_pass_report(&matrix, &sources))?;
            println!("PHASE_0_GATE=PASS");
            Ok(())
        }
        Err(error) => {
            let reason = error.to_string();
            reject(report_path, "NO_GO", &reason, &sources)
        }
    }
}

fn reject(
    report_path: &Path,
    verdict: &str,
    reason: &str,
    sources: &[SourceEvidence],
) -> Result<()> {
    write_report_atomic(report_path, &render_no_go_report(reason, sources))?;
    if verdict == "NO_GO" {
        println!("PHASE_0_GATE=NO_GO");
    } else {
        eprintln!("PHASE_0_GATE=FAIL");
    }
    anyhow::bail!("phase 0 gate did not accept evidence")
}

fn read_sources(root: &Path) -> Result<Vec<SourceEvidence>> {
    let mut paths = Vec::new();
    collect_json_files(root, root, 0, &mut paths)?;
    paths.sort();
    paths
        .into_iter()
        .filter(|path| path.file_name().is_none_or(|name| name != ".gitkeep"))
        .map(|path| {
            let bytes = fs::read(path)?;
            let contents = std::str::from_utf8(&bytes)?;
            Ok(SourceEvidence {
                evidence: Evidence::from_json(contents)?,
                sha256: hex_sha256(&bytes),
            })
        })
        .collect()
}

fn collect_json_files(
    root: &Path,
    current: &Path,
    depth: usize,
    paths: &mut Vec<PathBuf>,
) -> Result<()> {
    if depth > MAX_GATE_DEPTH || paths.len() > MAX_GATE_FILES {
        anyhow::bail!("bounded evidence traversal rejected")
    }
    let metadata = fs::symlink_metadata(current)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        anyhow::bail!("bounded evidence traversal rejected")
    }
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        if current == root && path.file_name().is_some_and(|name| name == ".gitkeep") {
            continue;
        }
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            anyhow::bail!("bounded evidence traversal rejected")
        }
        if metadata.is_dir() {
            collect_json_files(root, &path, depth + 1, paths)?;
        } else if metadata.is_file()
            && path.extension().and_then(|extension| extension.to_str()) == Some("json")
        {
            if !path.starts_with(root) {
                anyhow::bail!("bounded evidence traversal rejected")
            }
            paths.push(path);
        } else {
            anyhow::bail!("bounded evidence traversal rejected")
        }
    }
    Ok(())
}

fn render_pass_report(matrix: &PhaseZeroMatrix, sources: &[SourceEvidence]) -> String {
    let mut output = String::from("# Phase 0 Feasibility Report\n\nStatus: PASS\n\n");
    output.push_str("| Spike | Target | Session | Backend |\n");
    output.push_str("| --- | --- | --- | --- |\n");
    for required in &matrix.required {
        let _ = writeln!(
            output,
            "| {:?} | {} | {} | {} |",
            required.id,
            required.target.as_str(),
            required.session.as_deref().unwrap_or("-"),
            required.backend.as_deref().unwrap_or("-"),
        );
    }
    output.push_str(
        "\nEvidence inventory (source JSON SHA-256):\n\n| Source JSON SHA-256 |\n| --- |\n",
    );
    for source in sources {
        let _ = writeln!(output, "| `{}` |", source.sha256);
    }
    output
}

fn render_no_go_report(reason: &str, sources: &[SourceEvidence]) -> String {
    let mut output = String::from("# Phase 0 Feasibility NO-GO\n\n");
    output.push_str("Status: NO-GO\n\n");
    output.push_str("Gate result: ");
    output.push_str(reason);
    output.push_str("\n\n");
    output.push_str("Locally proven controls: frozen matrix validation, strict evidence schema, bounded redaction lint, and deterministic gate evaluation.\n\n");
    output.push_str("Protected evidence not collected:\n\n");
    output
        .push_str("- Six-device preview, hardware media, keyring, tray, and child-process runs.\n");
    output.push_str(
        "- Credentialed Gemini resumable upload, ACTIVE analysis, and remote-file cleanup run.\n",
    );
    output.push_str(
        "- Native FFmpeg source/keyring/SBOM correspondence and two-clean-build comparison.\n",
    );
    output.push_str("- Native package formats, platform signing, macOS notarization, and update-tamper rejection.\n");
    output.push_str("\nEvidence inventory (opaque source hashes only):\n\n");
    output.push_str("| Source JSON SHA-256 |\n| --- |\n");
    for source in sources {
        let _ = writeln!(output, "| `{}` |", source.sha256);
    }
    output
}

fn write_report_atomic(destination: &Path, contents: &str) -> Result<()> {
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(contents.as_bytes())?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(destination)
        .map_err(|error| error.error)?;
    #[cfg(unix)]
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

fn hex_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        let _ = write!(output, "{byte:02x}");
    }
    output
}
