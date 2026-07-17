mod cli;

use std::{env, fs, time::Instant};

use anyhow::{Context, Result};
use clap::Parser;
use spike_contracts::{Evidence, SpikeId, TargetId, Verdict};
use spike_media::{CpuFallback, FfprobeReport, content_sha256_bytes};

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
        } => cpu_fallback(ffmpeg, ffprobe, seconds, &output, evidence)?,
    }
    Ok(())
}

fn cpu_fallback(
    ffmpeg: std::path::PathBuf,
    ffprobe: std::path::PathBuf,
    seconds: u64,
    output: &std::path::Path,
    evidence_path: std::path::PathBuf,
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
    let parent = evidence_path
        .parent()
        .context("evidence path must have a parent directory")?;
    fs::create_dir_all(parent).context("unable to create evidence directory")?;
    fs::write(evidence_path, json).context("unable to write evidence")?;
    println!("CPU_FALLBACK=PASS codec=vp9 audio=opus");
    Ok(())
}
