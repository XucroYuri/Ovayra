use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "ovayra-spike", version, about = "Ovayra Phase 0 proof runner")]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    Version,
    Media {
        #[command(subcommand)]
        command: MediaCommand,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum MediaCommand {
    /// Generate and validate an LGPL CPU VP9/Opus `WebM` fallback.
    CpuFallback {
        #[arg(long)]
        ffmpeg: PathBuf,
        #[arg(long)]
        ffprobe: PathBuf,
        #[arg(long, default_value_t = 10)]
        seconds: u64,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        evidence: PathBuf,
    },
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::Parser;

    use super::{Cli, Command, MediaCommand};

    #[test]
    fn parses_only_the_cpu_fallback_contract_flags() {
        let cli = Cli::try_parse_from([
            "ovayra-spike",
            "media",
            "cpu-fallback",
            "--ffmpeg",
            "bundle/ffmpeg",
            "--ffprobe",
            "bundle/ffprobe",
            "--seconds",
            "3",
            "--output",
            "fallback.webm",
            "--evidence",
            "cpu-fallback.json",
        ])
        .unwrap();
        let Command::Media {
            command:
                MediaCommand::CpuFallback {
                    ffmpeg,
                    ffprobe,
                    seconds,
                    output,
                    evidence,
                },
        } = cli.command
        else {
            panic!("expected media cpu-fallback");
        };
        assert_eq!(ffmpeg, PathBuf::from("bundle/ffmpeg"));
        assert_eq!(ffprobe, PathBuf::from("bundle/ffprobe"));
        assert_eq!(seconds, 3);
        assert_eq!(output, PathBuf::from("fallback.webm"));
        assert_eq!(evidence, PathBuf::from("cpu-fallback.json"));
    }

    #[test]
    fn cpu_fallback_defaults_to_the_canonical_ten_seconds() {
        let cli = Cli::try_parse_from([
            "ovayra-spike",
            "media",
            "cpu-fallback",
            "--ffmpeg",
            "bundle/ffmpeg",
            "--ffprobe",
            "bundle/ffprobe",
            "--output",
            "fallback.webm",
            "--evidence",
            "cpu-fallback.json",
        ])
        .unwrap();
        let Command::Media {
            command: MediaCommand::CpuFallback { seconds, .. },
        } = cli.command
        else {
            panic!("expected media cpu-fallback");
        };
        assert_eq!(seconds, 10);
    }
}
