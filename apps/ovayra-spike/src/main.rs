mod cli;
mod preview_app;

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
use spike_gemini::{GeminiClient, UploadSession};
use spike_media::{
    AttemptOutcome, Backend, CpuFallback, DowngradeCode, ExecutionPolicy, FORCED_FAILURE_DEVICE,
    FfmpegError, FfmpegRunner, FfprobeReport, HardwarePlan, ProgressParser, content_sha256_bytes,
};
use spike_platform::{EncryptedRecord, EnvelopeCipher, OsSecretStore};

use crate::cli::{Cli, Command, GeminiCommand, MediaCommand};

const UPLOAD_CHECKPOINT_ACCOUNT: &str = "phase-0-upload-checkpoint-v1";

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Version => println!("ovayra-spike {}", env!("CARGO_PKG_VERSION")),
        Command::Preview {
            ffmpeg,
            input,
            duration_seconds,
            automation,
            evidence,
        } => preview_app::run_preview(
            ffmpeg,
            input,
            duration_seconds,
            automation,
            &evidence,
            evidence_target()?,
        )?,
        Command::Media {
            command:
                MediaCommand::CpuFallback {
                    ffmpeg,
                    ffprobe,
                    seconds,
                    output,
                    evidence,
                },
        } => cpu_fallback(
            ffmpeg,
            ffprobe,
            seconds,
            &output,
            &evidence,
            evidence_target()?,
        )?,
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
        Command::Gemini {
            command:
                GeminiCommand::StageUpload {
                    input,
                    checkpoint,
                    pause_after_chunks,
                    evidence,
                },
        } => stage_gemini_upload(&input, &checkpoint, pause_after_chunks, &evidence)?,
        Command::Gemini {
            command:
                GeminiCommand::ResumeAnalyze {
                    input,
                    checkpoint,
                    model,
                    evidence,
                },
        } => resume_gemini_upload(&input, &checkpoint, &model, &evidence)?,
    }
    Ok(())
}

fn stage_gemini_upload(
    input: &Path,
    checkpoint_path: &Path,
    pause_after_chunks: u8,
    evidence_path: &Path,
) -> Result<()> {
    if pause_after_chunks != 1 {
        anyhow::bail!("stage-upload must pause after exactly one chunk")
    }
    let target = evidence_target()?;
    let started = Instant::now();
    let bytes = fs::read(input).context("unable to read synthetic Gemini input")?;
    let api_key = gemini_api_key()?;
    let runtime = gemini_runtime()?;
    let (client, session) = runtime
        .block_on(async {
            let client = GeminiClient::new(api_key)?;
            let session = client
                .start_upload("phase-0-synthetic", "video/webm", bytes.len() as u64)
                .await?;
            Ok::<_, spike_gemini::GeminiError>((client, session))
        })
        .context("unable to start Gemini resumable upload")?;
    let chunk_size = client.chunk_size(&session);
    if bytes.len() as u64 <= chunk_size {
        anyhow::bail!("synthetic input must exceed the first upload chunk to prove process restart")
    }
    let first_chunk = &bytes
        [..usize::try_from(chunk_size).context("Gemini chunk size does not fit this platform")?];
    runtime
        .block_on(client.upload_chunk(&session, 0, first_chunk))
        .context("unable to stage the first Gemini chunk")?;
    let staged_offset = runtime
        .block_on(client.query_offset(&session))
        .context("unable to verify staged Gemini offset")?;
    if staged_offset == 0 {
        anyhow::bail!("Gemini did not accept the staged upload chunk")
    }
    let cipher = EnvelopeCipher::load_or_create(&OsSecretStore, UPLOAD_CHECKPOINT_ACCOUNT)
        .context("unable to load OS-keyring checkpoint encryption key")?;
    let record = client
        .checkpoint(&cipher, &session, staged_offset)
        .context("unable to encrypt Gemini checkpoint")?;
    write_checkpoint(checkpoint_path, &record)?;
    let mut evidence = Evidence::new(SpikeId::Gemini, target);
    evidence.measure("staged_offset", staged_offset)?;
    evidence.measure(
        "chunk_granularity",
        session.chunk_granularity().unwrap_or(chunk_size),
    )?;
    evidence.finish(
        Verdict::Pass,
        started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
    );
    write_finished_evidence(evidence_path, &evidence)?;
    println!("UPLOAD_PAUSED={staged_offset}");
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn resume_gemini_upload(
    input: &Path,
    checkpoint_path: &Path,
    model: &str,
    evidence_path: &Path,
) -> Result<()> {
    let target = evidence_target()?;
    let started = Instant::now();
    let bytes = fs::read(input).context("unable to read synthetic Gemini input")?;
    let record = read_checkpoint(checkpoint_path)?;
    let api_key = gemini_api_key()?;
    let cipher = EnvelopeCipher::load_or_create(&OsSecretStore, UPLOAD_CHECKPOINT_ACCOUNT)
        .context("unable to load OS-keyring checkpoint encryption key")?;
    let runtime = gemini_runtime()?;
    let client = GeminiClient::new(api_key).context("unable to configure Gemini client")?;
    let resumed = client
        .resume_checkpoint(&cipher, &record)
        .context("unable to decrypt Gemini checkpoint")?;
    let resumed_remote = runtime
        .block_on(resume_from_server(
            &client,
            resumed.session(),
            resumed.staged_offset(),
            &bytes,
        ))
        .context("unable to resume and finalize Gemini upload")?;
    let observed_offset = resumed_remote.observed_offset;
    let persisted_offset = resumed_remote.persisted_offset;
    let remote = resumed_remote.remote;
    let (analysis, remote_cleanup) =
        runtime.block_on(analyze_then_cleanup(&client, &remote, model));
    let checkpoint_cleanup = if remote_cleanup.is_ok() {
        fs::remove_file(checkpoint_path).context("unable to remove encrypted checkpoint")
    } else {
        // Retain only the encrypted record when remote cleanup failed: it is the recovery handle.
        Ok(())
    };
    let mut evidence = Evidence::new(SpikeId::Gemini, target);
    evidence.measure("persisted_offset", persisted_offset)?;
    evidence.measure("resumed_offset", observed_offset)?;
    evidence.measure("offset_mismatch", observed_offset != persisted_offset)?;
    evidence.measure(
        "remote_state",
        if analysis.is_ok() {
            "ACTIVE"
        } else {
            "TERMINAL_FAILURE"
        },
    )?;
    evidence.measure(
        "analysis_nonempty",
        analysis
            .as_ref()
            .is_ok_and(spike_gemini::GenerationResult::analysis_nonempty),
    )?;
    if let Ok(generation) = &analysis {
        evidence.measure("response_bytes", generation.response_bytes())?;
        evidence.measure("model", generation.model())?;
        evidence.measure("http_status", generation.status())?;
        evidence.measure(
            "analysis_latency_ms",
            generation
                .latency()
                .as_millis()
                .try_into()
                .unwrap_or(u64::MAX),
        )?;
    }
    evidence.measure(
        "remote_delete",
        if remote_cleanup.is_ok() {
            "PASS"
        } else {
            "FAIL"
        },
    )?;
    evidence.measure(
        "checkpoint_cleanup",
        if checkpoint_cleanup.is_ok() {
            "PASS"
        } else {
            "FAIL"
        },
    )?;
    evidence.finish(
        if analysis.is_ok() && remote_cleanup.is_ok() && checkpoint_cleanup.is_ok() {
            Verdict::Pass
        } else {
            Verdict::Fail
        },
        started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
    );
    write_finished_evidence(evidence_path, &evidence)?;
    checkpoint_cleanup?;
    remote_cleanup.context("unable to delete Gemini remote file")?;
    let generation = analysis.context("Gemini poll or analysis request failed")?;
    if !generation.analysis_nonempty() {
        anyhow::bail!("Gemini returned an empty analysis")
    }
    println!("UPLOAD_RESUMED=true");
    println!("REMOTE_STATE=ACTIVE");
    println!("ANALYSIS_NONEMPTY=true");
    println!("REMOTE_DELETE=PASS");
    Ok(())
}

async fn resume_and_finalize(
    client: &GeminiClient,
    session: &UploadSession,
    mut offset: u64,
    bytes: &[u8],
) -> Result<spike_gemini::RemoteFile, spike_gemini::GeminiError> {
    let total = u64::try_from(bytes.len()).map_err(|_| spike_gemini::GeminiError::Protocol)?;
    if offset > total {
        return Err(spike_gemini::GeminiError::Protocol);
    }
    let chunk_size = client.chunk_size(session);
    if session
        .chunk_granularity()
        .is_some_and(|granularity| offset < total && !offset.is_multiple_of(granularity))
    {
        return Err(spike_gemini::GeminiError::Protocol);
    }
    while total.saturating_sub(offset) > chunk_size {
        let end = offset + chunk_size;
        let start_index =
            usize::try_from(offset).map_err(|_| spike_gemini::GeminiError::Protocol)?;
        let end_index = usize::try_from(end).map_err(|_| spike_gemini::GeminiError::Protocol)?;
        client
            .upload_chunk(session, offset, &bytes[start_index..end_index])
            .await?;
        let expected = end;
        offset = if session.chunk_granularity().is_none() {
            client.query_offset(session).await?
        } else {
            expected
        };
        if offset > total {
            return Err(spike_gemini::GeminiError::Protocol);
        }
    }
    let start_index = usize::try_from(offset).map_err(|_| spike_gemini::GeminiError::Protocol)?;
    client
        .finalize_chunk(session, offset, &bytes[start_index..])
        .await
}

struct ResumedRemote {
    persisted_offset: u64,
    observed_offset: u64,
    remote: spike_gemini::RemoteFile,
}

async fn analyze_then_cleanup(
    client: &GeminiClient,
    remote: &spike_gemini::RemoteFile,
    model: &str,
) -> (
    Result<spike_gemini::GenerationResult, spike_gemini::GeminiError>,
    Result<(), spike_gemini::GeminiError>,
) {
    let analysis = async {
        let active = client
            .poll_until_ready(
                &remote.name,
                Duration::from_secs(2),
                Duration::from_secs(300),
            )
            .await?;
        client.generate_content(&active, model).await
    }
    .await;
    // A remote file exists even if polling or generation fails; cleanup is intentionally unconditional.
    let cleanup = client.delete_file(&remote.name).await;
    (analysis, cleanup)
}

async fn resume_from_server(
    client: &GeminiClient,
    session: &UploadSession,
    persisted_offset: u64,
    bytes: &[u8],
) -> Result<ResumedRemote, spike_gemini::GeminiError> {
    let observed_offset = client.query_offset(session).await?;
    let total = u64::try_from(bytes.len()).map_err(|_| spike_gemini::GeminiError::Protocol)?;
    if observed_offset > total
        || session.chunk_granularity().is_some_and(|granularity| {
            observed_offset < total && !observed_offset.is_multiple_of(granularity)
        })
    {
        return Err(spike_gemini::GeminiError::Protocol);
    }
    let remote = resume_and_finalize(client, session, observed_offset, bytes).await?;
    Ok(ResumedRemote {
        persisted_offset,
        observed_offset,
        remote,
    })
}

fn gemini_api_key() -> Result<String> {
    env::var("OVAYRA_GEMINI_API_KEY")
        .context("OVAYRA_GEMINI_API_KEY must be set in the environment or OS keyring")
}

fn gemini_runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("unable to create bounded Gemini runtime")
}

fn write_checkpoint(path: &Path, record: &EncryptedRecord) -> Result<()> {
    let json =
        serde_json::to_string_pretty(record).context("unable to serialize encrypted checkpoint")?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).context("unable to create checkpoint directory")?;
    write_evidence_atomic(path, &json).context("unable to persist encrypted checkpoint")?;
    Ok(())
}

fn read_checkpoint(path: &Path) -> Result<EncryptedRecord> {
    let bytes = fs::read(path).context("unable to read encrypted checkpoint")?;
    serde_json::from_slice(&bytes).context("encrypted checkpoint record is malformed")
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
    evidence_target_from_values(
        env::var("OVAYRA_TARGET_ID").ok().as_deref(),
        env::var("OVAYRA_EVIDENCE_TARGET").ok().as_deref(),
    )
}

/// Uses the Task 12 environment name; the legacy variable remains only for existing local runs.
fn evidence_target_from_values(primary: Option<&str>, legacy: Option<&str>) -> Result<TargetId> {
    let target = primary
        .or(legacy)
        .context("OVAYRA_TARGET_ID must name a supported Phase 0 target")?;
    TargetId::new(target).context("OVAYRA_TARGET_ID is not a supported target")
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
    target: TargetId,
) -> Result<()> {
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

    use spike_gemini::{FileState, GeminiClient, RemoteFile};
    use spike_platform::{EnvelopeCipher, MemorySecretStore};
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers};

    use super::{
        analyze_then_cleanup, evidence_target_from_values, resume_from_server,
        write_evidence_atomic,
    };

    #[tokio::test]
    async fn terminal_analysis_failure_still_attempts_remote_cleanup() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("GET")).and(matchers::path("/v1beta/files/1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"name":"files/1","uri":"gemini://files/1","mimeType":"video/webm","state":"FAILED","error":{"code":13,"message":"failure","status":"INTERNAL"}})))
            .mount(&server).await;
        Mock::given(matchers::method("DELETE"))
            .and(matchers::path("/v1beta/files/1"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        let client = GeminiClient::for_endpoints("test-key", &server.uri(), &server.uri()).unwrap();
        let remote = RemoteFile {
            name: "files/1".to_owned(),
            uri: "gemini://files/1".to_owned(),
            mime_type: "video/webm".to_owned(),
            state: FileState::Processing,
        };
        let (analysis, cleanup) =
            analyze_then_cleanup(&client, &remote, "gemini-3.1-flash-lite").await;
        assert!(analysis.is_err());
        assert!(cleanup.is_ok());
    }

    #[tokio::test]
    async fn resume_orchestration_uses_server_offset_when_checkpoint_hint_differs() {
        let server = MockServer::start().await;
        let session_url = format!("{}/session/1", server.uri());
        Mock::given(matchers::path("/upload/v1beta/files"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("x-goog-upload-url", session_url)
                    .insert_header("x-goog-upload-chunk-granularity", "4"),
            )
            .mount(&server)
            .await;
        Mock::given(matchers::header("x-goog-upload-command", "query"))
            .respond_with(
                ResponseTemplate::new(200).insert_header("x-goog-upload-size-received", "4"),
            )
            .mount(&server)
            .await;
        Mock::given(matchers::headers("x-goog-upload-command", vec!["upload", "finalize"]))
            .and(matchers::header("x-goog-upload-offset", "4"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"file":{"name":"files/1","uri":"gemini://files/1","mimeType":"video/webm","state":"PROCESSING"}})))
            .mount(&server).await;
        let client = GeminiClient::for_endpoints("test-key", &server.uri(), &server.uri()).unwrap();
        let session = client
            .start_upload("synthetic", "video/webm", 8)
            .await
            .unwrap();
        let cipher =
            EnvelopeCipher::load_or_create(&MemorySecretStore::default(), "resume-test").unwrap();
        let record = client.checkpoint(&cipher, &session, 0).unwrap();
        let resumed = client.resume_checkpoint(&cipher, &record).unwrap();
        let outcome = resume_from_server(
            &client,
            resumed.session(),
            resumed.staged_offset(),
            b"12345678",
        )
        .await
        .unwrap();
        assert_eq!(outcome.persisted_offset, 0);
        assert_eq!(outcome.observed_offset, 4);
        assert_eq!(outcome.remote.name, "files/1");
    }

    #[test]
    fn atomically_replaces_evidence_without_leaving_temporary_files() {
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("evidence.json");
        fs::write(&destination, "old").unwrap();
        write_evidence_atomic(&destination, "new").unwrap();
        assert_eq!(fs::read_to_string(&destination).unwrap(), "new");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn cpu_fallback_and_preview_handoffs_prefer_the_task_twelve_environment_name() {
        let target =
            evidence_target_from_values(Some("macos-arm64-vt"), Some("linux-x64-nvidia")).unwrap();
        assert_eq!(target.as_str(), "macos-arm64-vt");
    }
}
