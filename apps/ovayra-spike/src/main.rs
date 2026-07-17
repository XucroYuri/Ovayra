mod child_tree;
mod cli;
mod evidence_lint;
mod gate;
mod gemini_orchestration;
mod preview_app;

use std::{
    env, fs,
    io::{BufReader, BufWriter},
    path::Path,
    path::PathBuf,
    time::{Duration, Instant},
};

use aes_gcm::aead::Generate;
use anyhow::{Context, Result};
use clap::Parser;
use semver::Version;
use sha2::Digest;
use spike_contracts::{
    Evidence, MediaCpuProof, MediaForcedFallbackProof, MediaHardwareProof, PhaseZeroProof, SpikeId,
    TargetId, Verdict,
};
use spike_contracts::{
    PlatformProcessProof, ProofComponent, ProofPayload, ProofRow, phase_zero_session,
};
use spike_gemini::GeminiClient;
use spike_media::{
    AttemptOutcome, Backend, CpuFallback, ExecutionPolicy, FORCED_FAILURE_DEVICE, FfmpegError,
    FfmpegRunner, FfprobeReport, HardwarePlan, ProgressParser, content_sha256_bytes,
};
use spike_platform::{
    EncryptedRecord, EnvelopeCipher, OsSecretStore, SecretStore, SecretStoreError,
};
use spike_platform::{GroupedProcess, ProcessTreeProbe};
use spike_release::{FfmpegBundle, PackageRelease};
use zeroize::Zeroizing;

use crate::cli::{
    Cli, Command, EvidenceCommand, GeminiCommand, MediaCommand, PlatformCommand, ReleaseCommand,
};
use crate::gemini_orchestration::{ResumeRequest, resume_analyze_with_evidence, write_atomic};

const UPLOAD_CHECKPOINT_ACCOUNT: &str = "phase-0-upload-checkpoint-v1";
const KEYRING_SMOKE_SERVICE: &str = "com.ovayra.desktop";
const KEYRING_SMOKE_ACCOUNT_PREFIX: &str = "phase-0-keyring-smoke";

#[allow(clippy::too_many_lines)] // Composition-root command dispatch is intentionally explicit.
fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Version => println!("ovayra-spike {}", env!("CARGO_PKG_VERSION")),
        Command::Gate {
            evidence_dir,
            matrix,
            report,
        } => gate::run(&evidence_dir, &matrix, &report)?,
        Command::ChildTree {
            malformed_report,
            delay_report,
            exit_before_report,
            hold_stderr,
        } => child_tree::run_child_tree(child_tree::ChildTreeOptions {
            malformed_report,
            delay_report,
            exit_before_report,
            hold_stderr,
        })?,
        Command::ChildLeaf => child_tree::run_child_leaf(),
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
        Command::Evidence {
            command: EvidenceCommand::Lint { dir, text },
        } => evidence_lint::lint_dir(&dir, text)?,
        Command::Evidence {
            command:
                EvidenceCommand::VerifyPreview {
                    file,
                    expected_target,
                },
        } => evidence_lint::verify_preview(&file, &expected_target)?,
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
        Command::Platform {
            command: PlatformCommand::Keyring { evidence },
        } => keyring_smoke(&OsSecretStore, &evidence, evidence_target()?)?,
        Command::Platform {
            command: PlatformCommand::Process { evidence },
        } => process_smoke(&evidence, &evidence_target()?)?,
        Command::Platform {
            command:
                PlatformCommand::Tray {
                    automation,
                    force_no_tray,
                    evidence,
                },
        } => preview_app::run_tray_lifecycle(
            automation,
            force_no_tray,
            &evidence,
            evidence_target()?,
        )?,
        Command::Release {
            command: ReleaseCommand::VerifyFfmpeg { bundle, evidence },
        } => verify_ffmpeg_bundle(&bundle, &evidence, evidence_target()?)?,
        Command::Release {
            command:
                ReleaseCommand::PreparePackage {
                    bundle,
                    target,
                    output_root,
                },
        } => prepare_package(&bundle, &target, &output_root)?,
        Command::Release {
            command:
                ReleaseCommand::Manifest {
                    packages,
                    base_url,
                    output,
                    version,
                    release_tag,
                    pub_date,
                    notes,
                },
        } => {
            let version = Version::parse(&version).context("release version must be SemVer")?;
            if version.to_string() != env!("CARGO_PKG_VERSION")
                || release_tag != format!("phase-0-v{version}")
            {
                anyhow::bail!(
                    "release version must equal the workspace package version and protected phase-0 tag"
                );
            }
            PackageRelease::generate_manifest(
                &packages, &base_url, &output, &version, &pub_date, &notes,
            )
            .context("release manifest generation failed")?;
            println!("MANIFEST=PASS");
        }
        Command::Release {
            command:
                ReleaseCommand::VerifyManifest {
                    manifest,
                    packages,
                    public_key,
                    installed_version,
                },
        } => verify_package_manifest(&manifest, &packages, &public_key, &installed_version, false)?,
        Command::Release {
            command:
                ReleaseCommand::VerifyTamperRejection {
                    manifest,
                    packages,
                    public_key,
                    installed_version,
                },
        } => verify_package_manifest(&manifest, &packages, &public_key, &installed_version, true)?,
        Command::Release {
            command:
                ReleaseCommand::VerifyArtifact {
                    package,
                    public_key,
                },
        } => {
            let public_key =
                fs::read_to_string(public_key).context("cannot read update public key")?;
            PackageRelease::verify_artifact(&package, &public_key)
                .context("native artifact signature verification failed")?;
            println!("ARTIFACT_SIGNATURE=PASS");
        }
    }
    Ok(())
}

fn verify_package_manifest(
    manifest: &Path,
    packages: &Path,
    public_key_path: &Path,
    installed_version: &str,
    tamper: bool,
) -> Result<()> {
    let public_key =
        fs::read_to_string(public_key_path).context("cannot read update public key")?;
    let installed =
        Version::parse(installed_version).context("installed version must be SemVer")?;
    if tamper {
        PackageRelease::verify_tamper_rejection(manifest, packages, &public_key, &installed)
            .context("update tamper rejection failed")?;
        println!("TAMPER_REJECTION=PASS");
    } else {
        PackageRelease::verify_manifest(manifest, packages, &public_key, &installed)
            .context("update manifest verification failed")?;
        println!("MANIFEST=PASS");
    }
    Ok(())
}

fn prepare_package(bundle: &Path, target: &str, output_root: &Path) -> Result<()> {
    let expected_marker = match target {
        "aarch64-apple-darwin" => "macos-arm64-vt",
        "x86_64-pc-windows-msvc" => "windows-x64-mf",
        "x86_64-unknown-linux-gnu" => "linux-x64-vaapi-wayland",
        _ => anyhow::bail!("unsupported package target triple"),
    };
    FfmpegBundle::validate_layout(bundle)
        .context("FFmpeg bundle failed locked layout validation")?;
    if fs::read_to_string(bundle.join(".ovayra-target"))?.trim() != expected_marker {
        anyhow::bail!("FFmpeg bundle target marker does not match requested package triple");
    }
    fs::create_dir_all(output_root)?;
    let temporary = tempfile::Builder::new()
        .prefix("ovayra-package-")
        .tempdir_in(output_root)?;
    let generation = temporary.path().join("generation");
    let staged_bundle = generation.join("ffmpeg-stage");
    copy_validated_tree(bundle, &staged_bundle)?;
    generate_icons(&generation.join("icons"))?;
    let current = output_root.join("current");
    let previous = output_root.join("previous-generation");
    if previous.exists() {
        fs::remove_dir_all(&previous)?;
    }
    if current.exists() {
        fs::rename(&current, &previous)?;
    }
    if let Err(error) = fs::rename(&generation, &current) {
        if previous.exists() {
            fs::rename(&previous, &current)?;
        }
        return Err(error.into());
    }
    if previous.exists() {
        fs::remove_dir_all(previous)?;
    }
    println!("PACKAGE_PREPARE=PASS target={target}");
    Ok(())
}

fn copy_validated_tree(source: &Path, destination: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        anyhow::bail!("unsafe FFmpeg source directory");
    }
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&source_path)?;
        if metadata.file_type().is_symlink() {
            anyhow::bail!("FFmpeg bundle contains symlink during staging");
        }
        if metadata.is_dir() {
            copy_validated_tree(&source_path, &destination_path)?;
        } else if metadata.is_file() {
            fs::copy(&source_path, &destination_path)?;
            fs::set_permissions(&destination_path, metadata.permissions())?;
            let source_hash = sha2::Sha256::digest(fs::read(&source_path)?);
            let destination_hash = sha2::Sha256::digest(fs::read(&destination_path)?);
            if source_hash != destination_hash {
                anyhow::bail!("FFmpeg file changed while being staged");
            }
        } else {
            anyhow::bail!("FFmpeg bundle contains non-regular path");
        }
    }
    Ok(())
}

fn generate_icons(directory: &Path) -> Result<()> {
    use icns::{IconFamily, Image, PixelFormat};
    use ico::{IconDir, IconDirEntry, IconImage, ResourceType};
    use resvg::{tiny_skia, usvg};

    fs::create_dir_all(directory)?;
    let svg =
        fs::read("packaging/icons/spike.svg").context("cannot read deterministic SVG icon")?;
    let tree =
        usvg::Tree::from_data(&svg, &usvg::Options::default()).context("cannot parse SVG icon")?;
    let mut images = Vec::new();
    for size in [16_u32, 32, 128, 256, 512] {
        let mut pixmap =
            tiny_skia::Pixmap::new(size, size).context("cannot allocate icon pixmap")?;
        let scale = f32::from(u16::try_from(size).expect("icon size is bounded"))
            / tree.size().width().max(tree.size().height());
        resvg::render(
            &tree,
            tiny_skia::Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );
        let path = directory.join(format!("{size}x{size}.png"));
        pixmap.save_png(&path)?;
        let reader = png::Decoder::new(BufReader::new(fs::File::open(&path)?)).read_info()?;
        if reader.info().width != size
            || reader.info().height != size
            || sha2::Sha256::digest(fs::read(&path)?)
                .iter()
                .all(|byte| *byte == 0)
        {
            anyhow::bail!("invalid deterministic PNG icon");
        }
        images.push((size, pixmap.data().to_vec()));
    }
    let mut ico = IconDir::new(ResourceType::Icon);
    for (size, rgba) in &images {
        ico.add_entry(IconDirEntry::encode_as_png(&IconImage::from_rgba_data(
            *size,
            *size,
            rgba.clone(),
        ))?);
    }
    ico.write(BufWriter::new(fs::File::create(
        directory.join("spike.ico"),
    )?))?;
    let mut family = IconFamily::new();
    for (size, rgba) in images.into_iter().filter(|(size, _)| *size >= 128) {
        family.add_icon(&Image::from_data(PixelFormat::RGBA, size, size, rgba)?)?;
    }
    family.write(BufWriter::new(fs::File::create(
        directory.join("spike.icns"),
    )?))?;
    Ok(())
}

fn verify_ffmpeg_bundle(bundle: &Path, evidence_path: &Path, target: TargetId) -> Result<()> {
    let started = Instant::now();
    let result = FfmpegBundle::validate(bundle);
    let mut evidence = Evidence::new(SpikeId::Distribution, target);
    evidence.measure(
        "bundle_validation",
        if result.is_ok() { "pass" } else { "fail" },
    )?;
    evidence.measure("license_policy", "LGPL-only")?;
    evidence.measure(
        "source_correspondence",
        if result.is_ok() { "pass" } else { "fail" },
    )?;
    evidence.finish(
        if result.is_ok() {
            Verdict::Pass
        } else {
            Verdict::Fail
        },
        duration_ms(started),
    );
    write_finished_evidence(evidence_path, &evidence)?;
    result.context("FFmpeg bundle policy validation failed")?;
    println!("FFMPEG_BUNDLE=PASS license=LGPL-only source_correspondence=PASS");
    Ok(())
}

fn process_smoke(evidence_path: &Path, target: &TargetId) -> Result<()> {
    let executable = std::env::current_exe()?;
    let executable = executable.to_string_lossy().into_owned();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let started = Instant::now();
    let (parent_dead, grandchild_dead) = runtime.block_on(async {
        let mut process = GroupedProcess::spawn(&executable, &["child-tree"]).await?;
        let tree = process
            .wait_for_reported_tree(Duration::from_secs(5))
            .await?;
        process.kill_and_wait(Duration::from_secs(5)).await?;
        Ok::<_, spike_platform::ProcessGroupError>((
            !ProcessTreeProbe::any_alive(&tree),
            !ProcessTreeProbe::any_alive(&tree),
        ))
    })?;
    let proof = PhaseZeroProof {
        schema_version: 2,
        component: ProofComponent::PlatformProcess,
        row: ProofRow {
            spike: SpikeId::Platform,
            target: target.clone(),
            session: phase_zero_session(target).map(str::to_owned),
            backend: None,
        },
        proof: ProofPayload::PlatformProcess(PlatformProcessProof {
            parent_dead,
            grandchild_dead,
            elapsed_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
        }),
    };
    write_evidence_atomic(evidence_path, &proof.to_pretty_json()?)?;
    Ok(())
}

/// Exercises the production `SecretStore` boundary without putting a credential identifier or
/// bytes in evidence. The cleanup guard provides best-effort deletion during unwinding.
fn keyring_smoke(store: &impl SecretStore, evidence_path: &Path, target: TargetId) -> Result<()> {
    let started = Instant::now();
    let secret = Zeroizing::new(<[u8; 32]>::generate());
    let account_id = u128::from_le_bytes(
        secret[..16]
            .try_into()
            .expect("the fixed random account identifier has 16 bytes"),
    );
    let account = format!("{KEYRING_SMOKE_ACCOUNT_PREFIX}-{account_id:032x}");
    let mut cleanup = KeyringCleanup::new(store, account);
    let mut evidence = Evidence::new(SpikeId::Platform, target);
    let mut operation_error = None;

    let set_started = Instant::now();
    match store.set(KEYRING_SMOKE_SERVICE, cleanup.account(), secret.as_slice()) {
        Ok(()) => evidence.measure("write_status", "pass")?,
        Err(error) => {
            evidence.measure("write_status", error.category())?;
            operation_error = Some(error);
        }
    }
    evidence.measure("set_duration_ms", duration_ms(set_started))?;

    if operation_error.is_none() {
        let get_started = Instant::now();
        match store.get(KEYRING_SMOKE_SERVICE, cleanup.account()) {
            Ok(Some(value)) => {
                let value = Zeroizing::new(value);
                if constant_time_eq(secret.as_slice(), value.as_slice()) {
                    evidence.measure("read_status", "pass")?;
                } else {
                    evidence.measure("read_status", "mismatch")?;
                    operation_error = Some(SecretStoreError::Rejected);
                }
            }
            Ok(None) => {
                evidence.measure("read_status", "missing")?;
                operation_error = Some(SecretStoreError::Rejected);
            }
            Err(error) => {
                evidence.measure("read_status", error.category())?;
                operation_error = Some(error);
            }
        }
        evidence.measure("get_duration_ms", duration_ms(get_started))?;
    }

    let cleanup_started = Instant::now();
    let cleanup_error = cleanup.cleanup_and_confirm().err();
    evidence.measure("cleanup_duration_ms", duration_ms(cleanup_started))?;
    evidence.measure(
        "cleanup_status",
        cleanup_error
            .as_ref()
            .map_or("pass", SecretStoreError::category),
    )?;
    evidence.finish(
        if operation_error.is_none() && cleanup_error.is_none() {
            Verdict::Pass
        } else {
            Verdict::Fail
        },
        duration_ms(started),
    );
    write_finished_evidence(evidence_path, &evidence)?;

    if let Some(error) = cleanup_error {
        anyhow::bail!("keyring smoke cleanup failed ({})", error.category());
    }
    if let Some(error) = operation_error {
        anyhow::bail!("keyring smoke failed ({})", error.category());
    }
    println!("KEYRING=PASS cleanup=PASS");
    Ok(())
}

struct KeyringCleanup<'a, Store: SecretStore> {
    store: &'a Store,
    account: String,
    armed: bool,
}

impl<'a, Store: SecretStore> KeyringCleanup<'a, Store> {
    fn new(store: &'a Store, account: String) -> Self {
        Self {
            store,
            account,
            armed: true,
        }
    }

    fn account(&self) -> &str {
        &self.account
    }

    fn cleanup_and_confirm(&mut self) -> Result<(), SecretStoreError> {
        self.store.delete(KEYRING_SMOKE_SERVICE, &self.account)?;
        match self.store.get(KEYRING_SMOKE_SERVICE, &self.account)? {
            None => {
                self.armed = false;
                Ok(())
            }
            Some(value) => {
                drop(Zeroizing::new(value));
                Err(SecretStoreError::Rejected)
            }
        }
    }
}

impl<Store: SecretStore> Drop for KeyringCleanup<'_, Store> {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.store.delete(KEYRING_SMOKE_SERVICE, &self.account);
        }
    }
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

fn duration_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().try_into().unwrap_or(u64::MAX)
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
    let bytes = fs::read(input).context("unable to read synthetic Gemini input")?;
    let api_key = gemini_api_key()?;
    let cipher = EnvelopeCipher::load_or_create(&OsSecretStore, UPLOAD_CHECKPOINT_ACCOUNT)
        .context("unable to load OS-keyring checkpoint encryption key")?;
    let runtime = gemini_runtime()?;
    let client = GeminiClient::new(api_key).context("unable to configure Gemini client")?;
    runtime
        .block_on(resume_analyze_with_evidence(ResumeRequest {
            client: &client,
            cipher: &cipher,
            input: &bytes,
            checkpoint_path,
            model,
            evidence_path,
            target,
            poll_policy: spike_gemini::PollPolicy::bounded(
                Duration::from_secs(2),
                Duration::from_secs(300),
            ),
        }))
        .map_err(anyhow::Error::from)?;
    println!("UPLOAD_RESUMED=true");
    println!("REMOTE_STATE=ACTIVE");
    println!("ANALYSIS_NONEMPTY=true");
    println!("REMOTE_DELETE=PASS");
    Ok(())
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
    let output_bytes = fs::read(output).context("unable to read hardware self-test output")?;
    let report = FfprobeReport::read(ffprobe, output)?;
    let proof = PhaseZeroProof::media_hardware(
        target,
        backend.as_str().to_owned(),
        MediaHardwareProof {
            requested_backend: backend.as_str().to_owned(),
            actual_backend: actual.as_str().to_owned(),
            output_duration_seconds: rounded_duration_seconds(report.duration_seconds)?,
            output_sha256: content_sha256_bytes(&output_bytes),
        },
    );
    write_evidence_atomic(evidence_path, &proof.to_pretty_json()?)?;
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
    let _generated = fallback
        .transcode_synthetic_input(input, output, 10)
        .context("CPU fallback failed after the forced hardware failure")?;
    let report = FfprobeReport::read(ffprobe, output)
        .context("CPU fallback output did not pass the VP9/Opus WebM ffprobe contract")?;
    let actual = policy.observe(AttemptOutcome::Succeeded)?;
    let output_bytes = fs::read(output).context("unable to read CPU fallback output")?;
    let proof = PhaseZeroProof::media_forced_fallback(
        target,
        backend.as_str().to_owned(),
        MediaForcedFallbackProof {
            requested_backend: backend.as_str().to_owned(),
            cpu_restarts: 1,
            session_quarantined: policy.downgrade_code().is_some() && actual.is_cpu(),
            video_codec: report.video_codec.unwrap_or_default(),
            audio_codec: report.audio_codec.unwrap_or_default(),
            output_sha256: content_sha256_bytes(&output_bytes),
        },
    );
    write_evidence_atomic(evidence_path, &proof.to_pretty_json()?)?;
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
    let fallback = CpuFallback::new(ffmpeg, ffprobe);
    let generated = fallback.generate_synthetic(output, seconds)?;
    let report = FfprobeReport::read(fallback.ffprobe_path(), output)?;
    let output_bytes = fs::read(output).context("unable to read generated output")?;

    let proof = PhaseZeroProof::media_cpu(
        target,
        MediaCpuProof {
            actual_backend: "cpu".to_owned(),
            output_duration_seconds: rounded_duration_seconds(report.duration_seconds)?,
            video_codec: report.video_codec.unwrap_or_default(),
            audio_codec: report.audio_codec.unwrap_or_default(),
            progress_complete: generated.average_speed.is_some_and(f64::is_finite),
            output_sha256: content_sha256_bytes(&output_bytes),
        },
    );
    write_evidence_atomic(evidence_path, &proof.to_pretty_json()?)
        .context("unable to write evidence")?;
    println!("CPU_FALLBACK=PASS codec=vp9 audio=opus");
    Ok(())
}

fn write_evidence_atomic(destination: &Path, json: &str) -> std::io::Result<()> {
    write_atomic(destination, json)
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn rounded_duration_seconds(value: f64) -> Result<u64> {
    if !value.is_finite() || value < 0.0 {
        anyhow::bail!("ffprobe reported invalid duration")
    }
    Ok(value.ceil() as u64)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use spike_contracts::TargetId;
    use spike_platform::MemorySecretStore;

    use super::{evidence_target_from_values, keyring_smoke, write_evidence_atomic};

    #[test]
    fn keyring_smoke_round_trips_binary_data_and_writes_redacted_evidence() {
        let directory = tempfile::tempdir().unwrap();
        let evidence_path = directory.path().join("keyring.json");
        let store = MemorySecretStore::default();

        keyring_smoke(
            &store,
            &evidence_path,
            TargetId::new("macos-arm64-vt").unwrap(),
        )
        .unwrap();

        let evidence = fs::read_to_string(evidence_path).unwrap();
        assert!(evidence.contains("\"spike\": \"platform\""));
        assert!(evidence.contains("\"verdict\": \"pass\""));
        assert!(evidence.contains("\"cleanup_status\": \"pass\""));
        assert!(!evidence.contains("account"));
        assert!(!evidence.contains("secret"));
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
