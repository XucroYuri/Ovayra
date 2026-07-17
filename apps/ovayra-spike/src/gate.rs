use std::{fmt::Write as _, fs, io::Write as _, path::Path};

use anyhow::Result;
use spike_contracts::{PhaseZeroMatrix, PhaseZeroProof};

use crate::evidence_lint;

struct SourceEvidence {
    proof: PhaseZeroProof,
    sha256: String,
}

/// Runs the final acceptance gate. A successful lint is necessary but never
/// sufficient: all records must also be valid, exact, and complete.
pub(crate) fn run(evidence_dir: &Path, matrix_path: &Path, report_path: &Path) -> Result<()> {
    let Ok(matrix) = PhaseZeroMatrix::load(matrix_path) else {
        return reject(report_path, "FAIL", "matrix validation failed", &[]);
    };
    let Ok(verified) = evidence_lint::lint_verified(evidence_dir, false) else {
        return reject(
            report_path,
            "FAIL",
            "evidence lint rejected one or more bounded entries",
            &[],
        );
    };
    let Ok(sources) = verified
        .into_iter()
        .map(|verified| {
            let contents = std::str::from_utf8(&verified.bytes).map_err(anyhow::Error::from)?;
            Ok(SourceEvidence {
                proof: PhaseZeroProof::from_json(contents)?,
                sha256: verified.sha256,
            })
        })
        .collect::<Result<Vec<_>>>()
    else {
        return reject(report_path, "FAIL", "typed proof parse failed", &[]);
    };
    let proofs = sources
        .iter()
        .map(|source| source.proof.clone())
        .collect::<Vec<_>>();
    match matrix.evaluate_proofs(&proofs) {
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
