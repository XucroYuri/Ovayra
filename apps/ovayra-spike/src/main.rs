mod cli;

use std::{
    env, fs,
    io::Write,
    path::Path,
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use clap::Parser;
use spike_contracts::{Evidence, SpikeId, TargetId, Verdict};
use spike_media::{
    AttemptOutcome, Backend, CpuFallback, DowngradeCode, ExecutionPolicy, FORCED_FAILURE_DEVICE,
    FfmpegError, FfmpegRunner, FfprobeReport, HardwarePlan, ProgressParser, content_sha256_bytes,
};

use crate::cli::{Cli, Command, MediaCommand};

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Version => println!("ovayra-spike {}", env!("CARGO_PKG_VERSION")),
        Command::Media {
            command:
                MediaCommand::CpuFallback {
                    ffmpeg,
                    ffprobe,
                    seconds,
                    output,
                    evidence,
                },
        } => cpu_fallback(ffmpeg, ffprobe, seconds, &output, &evidence)?,
        Command::Media {
            command: MediaCommand::Inventory { ffmpeg, evidence },
        } => inventory(ffmpeg, &evidence)?,
        Command::Media {
            command:
                MediaCommand::SelfTest {
                    backend,
                    ffmpeg,
                    ffprobe,
                    input,
                    output,
                    render_device,
                    evidence,
                },
        } => self_test(
            backend,
            &ffmpeg,
            &ffprobe,
            &input,
            &output,
            render_device.as_deref(),
            &evidence,
        )?,
        Command::Media {
            command:
                MediaCommand::ForcedFallback {
                    backend,
                    ffmpeg,
                    ffprobe,
                    input,
                    output,
                    evidence,
                },
        } => forced_fallback(backend, &ffmpeg, &ffprobe, &input, &output, &evidence)?,
    }
    Ok(())
}

fn inventory(ffmpeg: PathBuf, evidence_path: &Path) -> Result<()> {
    let target = evidence_target()?;
    let started = Instant::now();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("unable to create bounded FFmpeg runtime")?;
    runtime
        .block_on(FfmpegRunner::new(ffmpeg).collect_inventory())
        .context("FFmpeg inventory did not complete all six required commands")?;
    let mut evidence = Evidence::new(SpikeId::Media, target);
    evidence.measure("inventory_command_count", 6_u8)?;
    evidence.finish(
        Verdict::Pass,
        started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
    );
    write_finished_evidence(evidence_path, &evidence)?;
    println!("INVENTORY=PASS commands=6");
    Ok(())
}

fn self_test(
    backend: Backend,
    ffmpeg: &Path,
    ffprobe: &Path,
    input: &Path,
    output: &Path,
    render_device: Option<&Path>,
    evidence_path: &Path,
) -> Result<()> {
    let target = evidence_target()?;
    let started = Instant::now();
    let mut policy = ExecutionPolicy::prefer(backend);
    if policy.next_backend() != Some(backend) {
        anyhow::bail!("hardware backend is quarantined; ordinary self-test will not fall back")
    }
    let outcome = run_hardware_attempt(backend, ffmpeg, ffprobe, input, output, render_device);
    let actual = match outcome {
        AttemptOutcome::Succeeded => policy.observe(AttemptOutcome::Succeeded)?,
        failure => {
            // Ordinary self-test deliberately does not execute the CPU attempt.
            let _ = policy.observe(failure)?;
            anyhow::bail!("hardware self-test failed; CPU fallback is intentionally disabled")
        }
    };
    let mut evidence = Evidence::new(SpikeId::Media, target);
    record_backend_evidence(&mut evidence, backend, actual, policy.downgrade_code())?;
    let output_bytes = fs::read(output).context("unable to read hardware self-test output")?;
    evidence.measure("content_sha256", content_sha256_bytes(&output_bytes))?;
    evidence.finish(
        Verdict::Pass,
        started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
    );
    write_finished_evidence(evidence_path, &evidence)?;
    println!("ACTUAL_BACKEND={}", actual.as_str());
    Ok(())
}

fn forced_fallback(
    backend: Backend,
    ffmpeg: &Path,
    ffprobe: &Path,
    input: &Path,
    output: &Path,
    evidence_path: &Path,
) -> Result<()> {
    let target = evidence_target()?;
    let started = Instant::now();
    let mut policy = ExecutionPolicy::prefer(backend);
    if policy.next_backend() != Some(backend) {
        anyhow::bail!(
            "hardware backend is quarantined; forced fallback requires a hardware attempt"
        )
    }
    let invalid_device = Path::new(FORCED_FAILURE_DEVICE);
    let outcome = run_forced_hardware_attempt(
        backend,
        ffmpeg,
        ffprobe,
        input,
        output,
        Some(invalid_device),
    );
    if matches!(outcome, AttemptOutcome::Succeeded) {
        anyhow::bail!("forced hardware failure unexpectedly succeeded")
    }
    let next = policy.observe(outcome)?;
    debug_assert!(next.is_cpu());
    let fallback = CpuFallback::new(ffmpeg, ffprobe);
    let generated = fallback
        .transcode_synthetic_input(input, output, 10)
        .context("CPU fallback failed after the forced hardware failure")?;
    let report = FfprobeReport::read(ffprobe, output)
        .context("CPU fallback output did not pass the VP9/Opus WebM ffprobe contract")?;
    let actual = policy.observe(AttemptOutcome::Succeeded)?;
    let output_bytes = fs::read(output).context("unable to read CPU fallback output")?;
    let mut evidence = Evidence::new(SpikeId::Media, target);
    record_backend_evidence(&mut evidence, backend, actual, policy.downgrade_code())?;
    evidence.measure("content_sha256", content_sha256_bytes(&output_bytes))?;
    evidence.measure("video_codec", report.video_codec)?;
    evidence.measure("audio_codec", report.audio_codec)?;
    evidence.measure("average_speed", generated.average_speed)?;
    evidence.finish(
        Verdict::Pass,
        started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
    );
    write_finished_evidence(evidence_path, &evidence)?;
    println!("ACTUAL_BACKEND=cpu");
    println!("DOWNGRADE_OBSERVED=true");
    Ok(())
}

fn run_hardware_attempt(
    backend: Backend,
    ffmpeg: &Path,
    ffprobe: &Path,
    input: &Path,
    output: &Path,
    render_device: Option<&Path>,
) -> AttemptOutcome {
    run_hardware_attempt_inner(backend, ffmpeg, ffprobe, input, output, render_device, true)
}

fn run_forced_hardware_attempt(
    backend: Backend,
    ffmpeg: &Path,
    ffprobe: &Path,
    input: &Path,
    output: &Path,
    render_device: Option<&Path>,
) -> AttemptOutcome {
    run_hardware_attempt_inner(
        backend,
        ffmpeg,
        ffprobe,
        input,
        output,
        render_device,
        false,
    )
}

fn run_hardware_attempt_inner(
    backend: Backend,
    ffmpeg: &Path,
    ffprobe: &Path,
    input: &Path,
    output: &Path,
    render_device: Option<&Path>,
    preflight: bool,
) -> AttemptOutcome {
    let plan = HardwarePlan::self_test(backend);
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return AttemptOutcome::SpawnFailed;
    };
    if preflight {
        let Ok(inventory) = runtime.block_on(FfmpegRunner::new(ffmpeg).collect_inventory()) else {
            return AttemptOutcome::ProbeFailed;
        };
        if !plan.is_available(&inventory, true, 1) {
            return AttemptOutcome::ProbeFailed;
        }
    }
    let command = runtime.block_on(FfmpegRunner::new(ffmpeg).run_os_with_timeout(
        plan.transcode_args(input, output, render_device),
        Duration::from_secs(30),
    ));
    let (progress, evidence) = match command {
        Ok(result) => result,
        Err(FfmpegError::Spawn(_)) => return AttemptOutcome::SpawnFailed,
        Err(FfmpegError::TimedOut) => return AttemptOutcome::TimedOut,
        Err(_) => return AttemptOutcome::NonZeroExit,
    };
    if evidence.exit_code != Some(0) {
        return AttemptOutcome::NonZeroExit;
    }
    let frames = ProgressParser::default()
        .push(&progress)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|event| event.frame)
        .max();
    if frames.unwrap_or(0) == 0 {
        return AttemptOutcome::MissingFrames;
    }
    match FfprobeReport::validate_any(ffprobe, output) {
        Ok(()) => AttemptOutcome::Succeeded,
        Err(_) => AttemptOutcome::InvalidFfprobe,
    }
}

fn evidence_target() -> Result<TargetId> {
    let target = env::var("OVAYRA_EVIDENCE_TARGET")
        .context("OVAYRA_EVIDENCE_TARGET must name a supported Phase 0 target")?;
    TargetId::new(target).context("OVAYRA_EVIDENCE_TARGET is not a supported target")
}

fn record_backend_evidence(
    evidence: &mut Evidence,
    requested: Backend,
    actual: Backend,
    downgrade_code: Option<DowngradeCode>,
) -> Result<()> {
    evidence.measure("requested_backend", requested.as_str())?;
    evidence.measure("actual_backend", actual.as_str())?;
    evidence.measure(
        "downgrade_code",
        downgrade_code.map_or("none", DowngradeCode::as_str),
    )?;
    Ok(())
}

fn write_finished_evidence(path: &Path, evidence: &Evidence) -> Result<()> {
    let json = evidence.to_pretty_json()?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).context("unable to create evidence directory")?;
    write_evidence_atomic(path, &json).context("unable to write evidence")
}

fn cpu_fallback(
    ffmpeg: std::path::PathBuf,
    ffprobe: std::path::PathBuf,
    seconds: u64,
    output: &std::path::Path,
    evidence_path: &std::path::Path,
) -> Result<()> {
    let target = env::var("OVAYRA_EVIDENCE_TARGET")
        .context("OVAYRA_EVIDENCE_TARGET must name a supported Phase 0 target")?;
    let target =
        TargetId::new(target).context("OVAYRA_EVIDENCE_TARGET is not a supported target")?;
    let started = Instant::now();
    let fallback = CpuFallback::new(ffmpeg, ffprobe);
    let generated = fallback.generate_synthetic(output, seconds)?;
    let report = FfprobeReport::read(fallback.ffprobe_path(), output)?;
    let output_bytes = fs::read(output).context("unable to read generated output")?;

    let mut evidence = Evidence::new(SpikeId::Media, target);
    evidence.measure("media_duration_seconds", report.duration_seconds)?;
    evidence.measure("output_bytes", output_bytes.len())?;
    evidence.measure("average_speed", generated.average_speed)?;
    evidence.measure("video_codec", report.video_codec)?;
    evidence.measure("audio_codec", report.audio_codec)?;
    evidence.measure("pixel_format", report.video_pixel_format)?;
    evidence.measure("ffmpeg_build_id", generated.ffmpeg_build_id)?;
    evidence.measure("content_sha256", content_sha256_bytes(&output_bytes))?;
    evidence.finish(
        Verdict::Pass,
        started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
    );

    let json = evidence.to_pretty_json()?;
    let parent = evidence_path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).context("unable to create evidence directory")?;
    write_evidence_atomic(evidence_path, &json).context("unable to write evidence")?;
    println!("CPU_FALLBACK=PASS codec=vp9 audio=opus");
    Ok(())
}

fn write_evidence_atomic(destination: &Path, json: &str) -> std::io::Result<()> {
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(json.as_bytes())?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(destination)
        .map_err(|error| error.error)?;
    #[cfg(unix)]
    fs::File::open(parent)?.sync_all()?;
    #[cfg(windows)]
    fs::File::open(destination)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::write_evidence_atomic;

    #[test]
    fn atomically_replaces_evidence_without_leaving_temporary_files() {
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("evidence.json");
        fs::write(&destination, "old").unwrap();
        write_evidence_atomic(&destination, "new").unwrap();
        assert_eq!(fs::read_to_string(&destination).unwrap(), "new");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
