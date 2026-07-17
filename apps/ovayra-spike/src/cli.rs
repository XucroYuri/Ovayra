use std::path::PathBuf;

use clap::{Parser, Subcommand};
use spike_media::Backend;

#[derive(Debug, Parser)]
#[command(name = "ovayra-spike", version, about = "Ovayra Phase 0 proof runner")]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    Version,
    /// Render a bounded `FFmpeg` preview through Slint's main-thread event loop.
    Preview {
        #[arg(long)]
        ffmpeg: PathBuf,
        #[arg(long)]
        input: PathBuf,
        #[arg(long, default_value_t = 120, value_parser = clap::value_parser!(u64).range(20..))]
        duration_seconds: u64,
        #[arg(long)]
        automation: bool,
        #[arg(long)]
        evidence: PathBuf,
    },
    Media {
        #[command(subcommand)]
        command: MediaCommand,
    },
    Gemini {
        #[command(subcommand)]
        command: GeminiCommand,
    },
    Platform {
        #[command(subcommand)]
        command: PlatformCommand,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum MediaCommand {
    /// Run the six exact `FFmpeg` inventory commands and persist redacted evidence.
    Inventory {
        #[arg(long)]
        ffmpeg: PathBuf,
        #[arg(long)]
        evidence: PathBuf,
    },
    /// Run one selected hardware backend without silently falling back.
    SelfTest {
        #[arg(long, value_parser = parse_hardware_backend)]
        backend: Backend,
        #[arg(long)]
        ffmpeg: PathBuf,
        #[arg(long)]
        ffprobe: PathBuf,
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        render_device: Option<PathBuf>,
        #[arg(long)]
        evidence: PathBuf,
    },
    /// Prove a forced hardware error switches to the synthetic CPU `WebM` fallback exactly once.
    ForcedFallback {
        #[arg(long, value_parser = parse_hardware_backend)]
        backend: Backend,
        #[arg(long)]
        ffmpeg: PathBuf,
        #[arg(long)]
        ffprobe: PathBuf,
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        evidence: PathBuf,
    },
    /// Generate and validate an LGPL CPU VP9/Opus `WebM` fallback.
    CpuFallback {
        #[arg(long)]
        ffmpeg: PathBuf,
        #[arg(long)]
        ffprobe: PathBuf,
        #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u64).range(1..))]
        seconds: u64,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        evidence: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum GeminiCommand {
    /// Start a resumable upload, stage exactly one chunk, and persist an encrypted checkpoint.
    StageUpload {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        checkpoint: PathBuf,
        #[arg(long, value_parser = clap::value_parser!(u8).range(1..=1))]
        pause_after_chunks: u8,
        #[arg(long)]
        evidence: PathBuf,
    },
    /// Resume an encrypted checkpoint in a separate process, analyze, delete remotely, and clean up.
    ResumeAnalyze {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        checkpoint: PathBuf,
        #[arg(long)]
        model: String,
        #[arg(long)]
        evidence: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum PlatformCommand {
    /// Exercise the native OS keyring with a disposable binary credential.
    Keyring {
        #[arg(long)]
        evidence: PathBuf,
    },
}

fn parse_hardware_backend(input: &str) -> Result<Backend, String> {
    let backend = input.parse::<Backend>().map_err(str::to_owned)?;
    if backend.is_cpu() {
        return Err("cpu is only an actual fallback backend".to_owned());
    }
    Ok(backend)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::Parser;

    use super::{Cli, Command, GeminiCommand, MediaCommand, PlatformCommand};

    #[test]
    fn parses_the_keyring_smoke_evidence_contract() {
        let cli = Cli::try_parse_from([
            "ovayra-spike",
            "platform",
            "keyring",
            "--evidence",
            "keyring.json",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Command::Platform {
                command: PlatformCommand::Keyring { .. }
            }
        ));
    }

    #[test]
    fn gemini_commands_require_a_real_two_process_checkpoint_contract() {
        let stage = Cli::try_parse_from([
            "ovayra-spike",
            "gemini",
            "stage-upload",
            "--input",
            "fallback.webm",
            "--checkpoint",
            "checkpoint.json",
            "--pause-after-chunks",
            "1",
            "--evidence",
            "stage.json",
        ])
        .unwrap();
        assert!(matches!(
            stage.command,
            Command::Gemini {
                command: GeminiCommand::StageUpload { .. }
            }
        ));
        assert!(
            Cli::try_parse_from([
                "ovayra-spike",
                "gemini",
                "stage-upload",
                "--input",
                "fallback.webm",
                "--checkpoint",
                "checkpoint.json",
                "--pause-after-chunks",
                "2",
                "--evidence",
                "stage.json",
            ])
            .is_err()
        );
        let resume = Cli::try_parse_from([
            "ovayra-spike",
            "gemini",
            "resume-analyze",
            "--input",
            "fallback.webm",
            "--checkpoint",
            "checkpoint.json",
            "--model",
            "gemini-3.1-flash-lite",
            "--evidence",
            "resume.json",
        ])
        .unwrap();
        assert!(matches!(
            resume.command,
            Command::Gemini {
                command: GeminiCommand::ResumeAnalyze { .. }
            }
        ));
    }

    #[test]
    fn preview_uses_the_measurement_contract_defaults() {
        let cli = Cli::try_parse_from([
            "ovayra-spike",
            "preview",
            "--ffmpeg",
            "bundle/ffmpeg",
            "--input",
            "fallback.webm",
            "--evidence",
            "preview.json",
        ])
        .unwrap();
        let Command::Preview {
            ffmpeg,
            input,
            duration_seconds,
            automation,
            evidence,
        } = cli.command
        else {
            panic!("expected preview");
        };
        assert_eq!(ffmpeg, PathBuf::from("bundle/ffmpeg"));
        assert_eq!(input, PathBuf::from("fallback.webm"));
        assert_eq!(duration_seconds, 120);
        assert!(!automation);
        assert_eq!(evidence, PathBuf::from("preview.json"));
    }

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

    #[test]
    fn rejects_zero_seconds() {
        assert!(
            Cli::try_parse_from([
                "ovayra-spike",
                "media",
                "cpu-fallback",
                "--ffmpeg",
                "ffmpeg",
                "--ffprobe",
                "ffprobe",
                "--seconds",
                "0",
                "--output",
                "fallback.webm",
                "--evidence",
                "evidence.json",
            ])
            .is_err()
        );
    }

    #[test]
    fn preview_rejects_durations_that_cannot_collect_the_twenty_second_rss_sample() {
        assert!(
            Cli::try_parse_from([
                "ovayra-spike",
                "preview",
                "--ffmpeg",
                "ffmpeg",
                "--input",
                "input.webm",
                "--duration-seconds",
                "19",
                "--evidence",
                "preview.json",
            ])
            .is_err()
        );
    }

    #[test]
    fn parses_the_hardware_inventory_and_self_test_contract_flags() {
        let inventory = Cli::try_parse_from([
            "ovayra-spike",
            "media",
            "inventory",
            "--ffmpeg",
            "bundle/ffmpeg",
            "--evidence",
            "inventory.json",
        ])
        .unwrap();
        assert!(matches!(
            inventory.command,
            Command::Media {
                command: MediaCommand::Inventory { .. }
            }
        ));

        let self_test = Cli::try_parse_from([
            "ovayra-spike",
            "media",
            "self-test",
            "--backend",
            "d3d11va-mf",
            "--ffmpeg",
            "bundle/ffmpeg",
            "--ffprobe",
            "bundle/ffprobe",
            "--input",
            "hardware-input.mp4",
            "--output",
            "hardware-output.mp4",
            "--render-device",
            "device",
            "--evidence",
            "self-test.json",
        ])
        .unwrap();
        assert!(matches!(
            self_test.command,
            Command::Media {
                command: MediaCommand::SelfTest { .. }
            }
        ));
    }

    #[test]
    fn parses_forced_fallback_and_rejects_cpu_as_a_requested_hardware_backend() {
        let cli = Cli::try_parse_from([
            "ovayra-spike",
            "media",
            "forced-fallback",
            "--backend",
            "nvenc-nvdec",
            "--ffmpeg",
            "bundle/ffmpeg",
            "--ffprobe",
            "bundle/ffprobe",
            "--input",
            "hardware-input.mp4",
            "--output",
            "forced-fallback.webm",
            "--evidence",
            "fallback.json",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Command::Media {
                command: MediaCommand::ForcedFallback { .. }
            }
        ));
        assert!(
            Cli::try_parse_from([
                "ovayra-spike",
                "media",
                "self-test",
                "--backend",
                "cpu",
                "--ffmpeg",
                "bundle/ffmpeg",
                "--ffprobe",
                "bundle/ffprobe",
                "--input",
                "hardware-input.mp4",
                "--output",
                "hardware-output.mp4",
                "--evidence",
                "self-test.json",
            ])
            .is_err()
        );
    }
}
