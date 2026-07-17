use std::{
    error::Error,
    fmt::{self, Write as _},
    fs,
    io::Write,
    path::Path,
    time::Instant,
};

use sha2::{Digest, Sha256};
use spike_contracts::{
    Evidence, GeminiResumeProof, PhaseZeroProof, ProofComponent, ProofPayload, ProofRow, SpikeId,
    TargetId, Verdict,
};
use spike_gemini::{GeminiClient, GenerationResult, PollPolicy, RemoteFile, UploadSession};
use spike_platform::{EncryptedRecord, EnvelopeCipher};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResumeFailureCategory {
    CheckpointRead,
    CheckpointDecrypt,
    Query,
    OffsetBeyondInput,
    OffsetMisaligned,
    Continuation,
    Finalization,
    Analysis,
    EmptyAnalysis,
    RemoteCleanup,
    CheckpointCleanup,
    EvidenceWrite,
}

impl ResumeFailureCategory {
    const fn as_str(self) -> &'static str {
        match self {
            Self::CheckpointRead => "CHECKPOINT_READ",
            Self::CheckpointDecrypt => "CHECKPOINT_DECRYPT",
            Self::Query => "QUERY_FAILED",
            Self::OffsetBeyondInput => "OFFSET_BEYOND_INPUT",
            Self::OffsetMisaligned => "OFFSET_MISALIGNED",
            Self::Continuation => "CONTINUATION_FAILED",
            Self::Finalization => "FINALIZATION_FAILED",
            Self::Analysis => "ANALYSIS_FAILED",
            Self::EmptyAnalysis => "EMPTY_ANALYSIS",
            Self::RemoteCleanup => "REMOTE_CLEANUP_FAILED",
            Self::CheckpointCleanup => "CHECKPOINT_CLEANUP_FAILED",
            Self::EvidenceWrite => "EVIDENCE_WRITE_FAILED",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResumeOutcome {
    pub(crate) persisted_hint: u64,
    pub(crate) observed_server_offset: Option<u64>,
    pub(crate) offset_mismatch: Option<bool>,
    pub(crate) failure_category: Option<ResumeFailureCategory>,
}

#[derive(Debug)]
pub(crate) struct ResumeError {
    pub(crate) outcome: ResumeOutcome,
}

impl fmt::Display for ResumeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let category = self
            .outcome
            .failure_category
            .map_or("UNKNOWN", ResumeFailureCategory::as_str);
        write!(formatter, "Gemini resume orchestration failed: {category}")
    }
}

impl Error for ResumeError {}

pub(crate) struct ResumeRequest<'a> {
    pub(crate) client: &'a GeminiClient,
    pub(crate) cipher: &'a EnvelopeCipher,
    pub(crate) input: &'a [u8],
    pub(crate) checkpoint_path: &'a Path,
    pub(crate) model: &'a str,
    pub(crate) evidence_path: &'a Path,
    pub(crate) target: TargetId,
    pub(crate) poll_policy: PollPolicy,
}

struct EvidenceInput<'a> {
    evidence_path: &'a Path,
    target: TargetId,
    outcome: &'a ResumeOutcome,
    started: Instant,
    generation: Option<&'a GenerationResult>,
    remote_cleanup_state: &'a str,
    checkpoint_cleanup_state: &'a str,
    verdict: Verdict,
}

#[allow(clippy::too_many_lines)]
pub(crate) async fn resume_analyze_with_evidence(
    request: ResumeRequest<'_>,
) -> Result<ResumeOutcome, ResumeError> {
    let ResumeRequest {
        client,
        cipher,
        input,
        checkpoint_path,
        model,
        evidence_path,
        target,
        poll_policy,
    } = request;
    let started = Instant::now();
    let checkpoint_binding = fs::read(checkpoint_path).ok().map(|bytes| {
        let mut binding = String::with_capacity(64);
        for byte in Sha256::digest(bytes) {
            let _ = write!(binding, "{byte:02x}");
        }
        binding
    });
    let Ok(record) = read_checkpoint(checkpoint_path) else {
        return fail(
            ResumeOutcome {
                persisted_hint: 0,
                observed_server_offset: None,
                offset_mismatch: None,
                failure_category: Some(ResumeFailureCategory::CheckpointRead),
            },
            evidence_path,
            target,
            started,
            None,
            "NOT_ATTEMPTED",
            "RETAINED_FOR_RECOVERY",
        );
    };
    let Ok(resumed) = client.resume_checkpoint(cipher, &record) else {
        return fail(
            ResumeOutcome {
                persisted_hint: 0,
                observed_server_offset: None,
                offset_mismatch: None,
                failure_category: Some(ResumeFailureCategory::CheckpointDecrypt),
            },
            evidence_path,
            target,
            started,
            None,
            "NOT_ATTEMPTED",
            "RETAINED_FOR_RECOVERY",
        );
    };
    let mut outcome = ResumeOutcome {
        persisted_hint: resumed.staged_offset(),
        observed_server_offset: None,
        offset_mismatch: None,
        failure_category: None,
    };
    let Ok(observed) = client.query_offset(resumed.session()).await else {
        outcome.failure_category = Some(ResumeFailureCategory::Query);
        return fail(
            outcome,
            evidence_path,
            target,
            started,
            None,
            "NOT_ATTEMPTED",
            "RETAINED_FOR_RECOVERY",
        );
    };
    outcome.observed_server_offset = Some(observed);
    outcome.offset_mismatch = Some(observed != outcome.persisted_hint);
    let Ok(total) = u64::try_from(input.len()) else {
        outcome.failure_category = Some(ResumeFailureCategory::OffsetBeyondInput);
        return fail(
            outcome,
            evidence_path,
            target,
            started,
            None,
            "NOT_ATTEMPTED",
            "RETAINED_FOR_RECOVERY",
        );
    };
    if observed > total {
        outcome.failure_category = Some(ResumeFailureCategory::OffsetBeyondInput);
        return fail(
            outcome,
            evidence_path,
            target,
            started,
            None,
            "NOT_ATTEMPTED",
            "RETAINED_FOR_RECOVERY",
        );
    }
    if resumed
        .session()
        .chunk_granularity()
        .is_some_and(|granularity| observed < total && !observed.is_multiple_of(granularity))
    {
        outcome.failure_category = Some(ResumeFailureCategory::OffsetMisaligned);
        return fail(
            outcome,
            evidence_path,
            target,
            started,
            None,
            "NOT_ATTEMPTED",
            "RETAINED_FOR_RECOVERY",
        );
    }
    let remote = match continue_and_finalize(client, resumed.session(), observed, input).await {
        Ok(remote) => remote,
        Err(category) => {
            outcome.failure_category = Some(category);
            return fail(
                outcome,
                evidence_path,
                target,
                started,
                None,
                "NOT_ATTEMPTED",
                "RETAINED_FOR_RECOVERY",
            );
        }
    };
    let analysis = analyze(client, &remote, model, poll_policy).await;
    let remote_cleanup = client.delete_file(&remote.name).await;
    let remote_cleanup_state = if remote_cleanup.is_ok() {
        "DELETED"
    } else {
        "FAILED"
    };
    let checkpoint_cleanup = if remote_cleanup.is_ok() {
        fs::remove_file(checkpoint_path).map_err(|_| ())
    } else {
        Ok(())
    };
    let checkpoint_cleanup_state = if remote_cleanup.is_err() {
        "RETAINED_FOR_RECOVERY"
    } else if checkpoint_cleanup.is_ok() {
        "DELETED"
    } else {
        "FAILED"
    };
    if analysis.is_err() {
        outcome.failure_category = Some(ResumeFailureCategory::Analysis);
    } else if analysis
        .as_ref()
        .is_ok_and(|result| !result.analysis_nonempty())
    {
        outcome.failure_category = Some(ResumeFailureCategory::EmptyAnalysis);
    } else if remote_cleanup.is_err() {
        outcome.failure_category = Some(ResumeFailureCategory::RemoteCleanup);
    } else if checkpoint_cleanup.is_err() {
        outcome.failure_category = Some(ResumeFailureCategory::CheckpointCleanup);
    }
    let verdict = if outcome.failure_category.is_none() {
        Verdict::Pass
    } else {
        Verdict::Fail
    };
    if outcome.failure_category.is_none() {
        let Some(generation) = analysis.as_ref().ok() else {
            return Err(ResumeError { outcome });
        };
        let Some(binding) = checkpoint_binding else {
            return Err(ResumeError { outcome });
        };
        let proof = PhaseZeroProof {
            schema_version: 2,
            component: ProofComponent::GeminiResume,
            row: ProofRow {
                spike: SpikeId::Gemini,
                target: target.clone(),
                session: None,
                backend: None,
            },
            proof: ProofPayload::GeminiResume(GeminiResumeProof {
                checkpoint_id: binding,
                resumed_offset: outcome.persisted_hint,
                server_offset: outcome.observed_server_offset.unwrap_or(0),
                server_authoritative: outcome.offset_mismatch == Some(false),
                remote_state: "ACTIVE".to_owned(),
                analysis_nonempty: generation.analysis_nonempty(),
                model: generation.model().to_owned(),
                http_status: generation.status(),
                remote_deleted: remote_cleanup_state == "DELETED",
                checkpoint_deleted: checkpoint_cleanup_state == "DELETED",
                retry_policy_observed: true,
            }),
        };
        let json = proof.to_pretty_json().map_err(|_| ResumeError {
            outcome: outcome.clone(),
        })?;
        write_atomic(evidence_path, &json).map_err(|_| ResumeError {
            outcome: outcome.clone(),
        })?;
    } else if write_evidence(EvidenceInput {
        evidence_path,
        target,
        outcome: &outcome,
        started,
        generation: analysis.as_ref().ok(),
        remote_cleanup_state,
        checkpoint_cleanup_state,
        verdict,
    })
    .is_err()
    {
        outcome.failure_category = Some(ResumeFailureCategory::EvidenceWrite);
        return Err(ResumeError { outcome });
    }
    if outcome.failure_category.is_some() {
        return Err(ResumeError { outcome });
    }
    Ok(outcome)
}

async fn continue_and_finalize(
    client: &GeminiClient,
    session: &UploadSession,
    mut offset: u64,
    bytes: &[u8],
) -> Result<RemoteFile, ResumeFailureCategory> {
    let total = u64::try_from(bytes.len()).map_err(|_| ResumeFailureCategory::Continuation)?;
    let chunk_size = client.chunk_size(session);
    while total.saturating_sub(offset) > chunk_size {
        let end_offset = offset + chunk_size;
        let start = usize::try_from(offset).map_err(|_| ResumeFailureCategory::Continuation)?;
        let end = usize::try_from(end_offset).map_err(|_| ResumeFailureCategory::Continuation)?;
        client
            .upload_chunk(session, offset, &bytes[start..end])
            .await
            .map_err(|_| ResumeFailureCategory::Continuation)?;
        offset = if session.chunk_granularity().is_none() {
            client
                .query_offset(session)
                .await
                .map_err(|_| ResumeFailureCategory::Continuation)?
        } else {
            end_offset
        };
        if offset > total {
            return Err(ResumeFailureCategory::Continuation);
        }
    }
    let start = usize::try_from(offset).map_err(|_| ResumeFailureCategory::Finalization)?;
    client
        .finalize_chunk(session, offset, &bytes[start..])
        .await
        .map_err(|_| ResumeFailureCategory::Finalization)
}

async fn analyze(
    client: &GeminiClient,
    remote: &RemoteFile,
    model: &str,
    poll_policy: PollPolicy,
) -> Result<GenerationResult, ()> {
    let active = client
        .poll_until_ready_with_policy(&remote.name, poll_policy)
        .await
        .map_err(|_| ())?;
    client
        .generate_content(&active, model)
        .await
        .map_err(|_| ())
}

fn fail(
    mut outcome: ResumeOutcome,
    evidence_path: &Path,
    target: TargetId,
    started: Instant,
    generation: Option<&GenerationResult>,
    remote_cleanup_state: &str,
    checkpoint_cleanup_state: &str,
) -> Result<ResumeOutcome, ResumeError> {
    if write_evidence(EvidenceInput {
        evidence_path,
        target,
        outcome: &outcome,
        started,
        generation,
        remote_cleanup_state,
        checkpoint_cleanup_state,
        verdict: Verdict::Fail,
    })
    .is_err()
    {
        outcome.failure_category = Some(ResumeFailureCategory::EvidenceWrite);
    }
    Err(ResumeError { outcome })
}

fn write_evidence(input: EvidenceInput<'_>) -> Result<(), ()> {
    let EvidenceInput {
        evidence_path,
        target,
        outcome,
        started,
        generation,
        remote_cleanup_state,
        checkpoint_cleanup_state,
        verdict,
    } = input;
    let mut evidence = Evidence::new(SpikeId::Gemini, target);
    evidence
        .measure("persisted_hint", outcome.persisted_hint)
        .map_err(|_| ())?;
    evidence
        .measure("observed_server_offset", outcome.observed_server_offset)
        .map_err(|_| ())?;
    evidence
        .measure("offset_mismatch", outcome.offset_mismatch)
        .map_err(|_| ())?;
    evidence
        .measure(
            "failure_category",
            outcome.failure_category.map(ResumeFailureCategory::as_str),
        )
        .map_err(|_| ())?;
    evidence
        .measure("remote_cleanup_state", remote_cleanup_state)
        .map_err(|_| ())?;
    evidence
        .measure("checkpoint_cleanup_state", checkpoint_cleanup_state)
        .map_err(|_| ())?;
    if let Some(generation) = generation {
        evidence
            .measure("analysis_nonempty", generation.analysis_nonempty())
            .map_err(|_| ())?;
        evidence
            .measure("response_bytes", generation.response_bytes())
            .map_err(|_| ())?;
        evidence
            .measure("model", generation.model())
            .map_err(|_| ())?;
        evidence
            .measure("http_status", generation.status())
            .map_err(|_| ())?;
        evidence
            .measure(
                "analysis_latency_ms",
                generation
                    .latency()
                    .as_millis()
                    .try_into()
                    .unwrap_or(u64::MAX),
            )
            .map_err(|_| ())?;
    } else {
        evidence
            .measure("analysis_nonempty", false)
            .map_err(|_| ())?;
    }
    evidence.finish(
        verdict,
        started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
    );
    let json = evidence.to_pretty_json().map_err(|_| ())?;
    write_atomic(evidence_path, &json).map_err(|_| ())
}

fn read_checkpoint(path: &Path) -> Result<EncryptedRecord, ()> {
    let bytes = fs::read(path).map_err(|_| ())?;
    serde_json::from_slice(&bytes).map_err(|_| ())
}

pub(crate) fn write_atomic(destination: &Path, json: &str) -> std::io::Result<()> {
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
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
    use std::{
        fs,
        sync::{Arc, Mutex},
        time::Duration,
    };

    use spike_contracts::TargetId;
    use spike_gemini::{GeminiClient, PollPolicy, RetryPolicy};
    use spike_platform::{EnvelopeCipher, MemorySecretStore};
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers};

    use super::{ResumeFailureCategory, ResumeRequest, resume_analyze_with_evidence, write_atomic};

    fn test_client(server: &MockServer) -> GeminiClient {
        GeminiClient::for_endpoints_with_retry_policy(
            "test-key-that-must-not-leak",
            &server.uri(),
            &server.uri(),
            RetryPolicy::bounded(3, Duration::ZERO),
        )
        .unwrap()
    }

    async fn checkpoint(
        server: &MockServer,
        cipher: &EnvelopeCipher,
        checkpoint_path: &std::path::Path,
        hint: u64,
        granularity: u64,
    ) -> GeminiClient {
        Mock::given(matchers::path("/upload/v1beta/files"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header(
                        "x-goog-upload-url",
                        format!("{}/session/sensitive-upload-url", server.uri()),
                    )
                    .insert_header("x-goog-upload-chunk-granularity", granularity.to_string()),
            )
            .mount(server)
            .await;
        let client = test_client(server);
        let session = client
            .start_upload("synthetic", "video/webm", 12)
            .await
            .unwrap();
        let record = client.checkpoint(cipher, &session, hint).unwrap();
        write_atomic(checkpoint_path, &serde_json::to_string(&record).unwrap()).unwrap();
        client
    }

    fn target() -> TargetId {
        TargetId::new("linux-x64-vaapi-wayland").unwrap()
    }

    fn assert_redacted(text: &str) {
        assert!(!text.contains("test-key-that-must-not-leak"));
        assert!(!text.contains("sensitive-upload-url"));
    }

    #[tokio::test]
    async fn resume_misaligned_offset_writes_redacted_failed_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let checkpoint_path = dir.path().join("checkpoint.json");
        let evidence_path = dir.path().join("evidence.json");
        let cipher =
            EnvelopeCipher::load_or_create(&MemorySecretStore::default(), "misaligned").unwrap();
        let server = MockServer::start().await;
        let client = checkpoint(&server, &cipher, &checkpoint_path, 0, 4).await;
        Mock::given(matchers::header("x-goog-upload-command", "query"))
            .respond_with(
                ResponseTemplate::new(200).insert_header("x-goog-upload-size-received", "2"),
            )
            .mount(&server)
            .await;
        let error = resume_analyze_with_evidence(ResumeRequest {
            client: &client,
            cipher: &cipher,
            input: b"12345678",
            checkpoint_path: &checkpoint_path,
            model: "gemini-3.1-flash-lite",
            evidence_path: &evidence_path,
            target: target(),
            poll_policy: PollPolicy::bounded(Duration::ZERO, Duration::ZERO),
        })
        .await
        .unwrap_err();
        assert_eq!(error.outcome.persisted_hint, 0);
        assert_eq!(error.outcome.observed_server_offset, Some(2));
        assert_eq!(error.outcome.offset_mismatch, Some(true));
        assert_eq!(
            error.outcome.failure_category,
            Some(ResumeFailureCategory::OffsetMisaligned)
        );
        assert!(checkpoint_path.exists());
        let evidence = fs::read_to_string(&evidence_path).unwrap();
        assert!(evidence.contains("OFFSET_MISALIGNED"));
        assert!(evidence.contains("\"observed_server_offset\": 2"));
        assert_redacted(&evidence);
    }

    #[tokio::test]
    async fn resume_beyond_input_offset_writes_redacted_failed_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let checkpoint_path = dir.path().join("checkpoint.json");
        let evidence_path = dir.path().join("evidence.json");
        let cipher =
            EnvelopeCipher::load_or_create(&MemorySecretStore::default(), "beyond-input").unwrap();
        let server = MockServer::start().await;
        let client = checkpoint(&server, &cipher, &checkpoint_path, 0, 4).await;
        Mock::given(matchers::header("x-goog-upload-command", "query"))
            .respond_with(
                ResponseTemplate::new(200).insert_header("x-goog-upload-size-received", "9"),
            )
            .mount(&server)
            .await;
        let error = resume_analyze_with_evidence(ResumeRequest {
            client: &client,
            cipher: &cipher,
            input: b"12345678",
            checkpoint_path: &checkpoint_path,
            model: "gemini-3.1-flash-lite",
            evidence_path: &evidence_path,
            target: target(),
            poll_policy: PollPolicy::bounded(Duration::ZERO, Duration::ZERO),
        })
        .await
        .unwrap_err();
        assert_eq!(error.outcome.observed_server_offset, Some(9));
        assert_eq!(error.outcome.offset_mismatch, Some(true));
        assert_eq!(
            error.outcome.failure_category,
            Some(ResumeFailureCategory::OffsetBeyondInput)
        );
        let evidence = fs::read_to_string(&evidence_path).unwrap();
        assert!(evidence.contains("OFFSET_BEYOND_INPUT"));
        assert_redacted(&evidence);
    }

    #[tokio::test]
    async fn omitted_granularity_uses_eight_mib_chunk_then_queries_before_finalizing() {
        let dir = tempfile::tempdir().unwrap();
        let checkpoint_path = dir.path().join("checkpoint.json");
        let evidence_path = dir.path().join("evidence.json");
        let cipher =
            EnvelopeCipher::load_or_create(&MemorySecretStore::default(), "fallback-chunk")
                .unwrap();
        let server = MockServer::start().await;
        let session_url = format!("{}/session/sensitive-upload-url", server.uri());
        Mock::given(matchers::path("/upload/v1beta/files"))
            .respond_with(
                ResponseTemplate::new(200).insert_header("x-goog-upload-url", session_url),
            )
            .mount(&server)
            .await;
        let client = test_client(&server);
        let input = vec![7_u8; 8 * 1024 * 1024 + 5];
        let session = client
            .start_upload("synthetic", "video/webm", input.len() as u64)
            .await
            .unwrap();
        let record = client.checkpoint(&cipher, &session, 0).unwrap();
        write_atomic(&checkpoint_path, &serde_json::to_string(&record).unwrap()).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let query_count = Arc::new(Mutex::new(0_u8));
        Mock::given(matchers::path("/session/sensitive-upload-url"))
            .respond_with({
                let requests = Arc::clone(&requests);
                let query_count = Arc::clone(&query_count);
                move |request: &wiremock::Request| {
                    let command = request
                        .headers
                        .get("x-goog-upload-command")
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .to_owned();
                    let offset = request
                        .headers
                        .get("x-goog-upload-offset")
                        .and_then(|value| value.to_str().ok())
                        .map(str::to_owned);
                    requests
                        .lock()
                        .unwrap()
                        .push((command.clone(), offset, request.body.len()));
                    if command == "query" {
                        let mut query_count = query_count.lock().unwrap();
                        *query_count += 1;
                        ResponseTemplate::new(200).insert_header(
                            "x-goog-upload-size-received",
                            if *query_count == 1 { "0" } else { "8388608" },
                        )
                    } else if command == "upload, finalize" {
                        ResponseTemplate::new(200).set_body_json(serde_json::json!({"file":{"name":"files/1","uri":"gemini://files/1","mimeType":"video/webm","state":"PROCESSING"}}))
                    } else {
                        ResponseTemplate::new(200)
                    }
                }
            })
            .mount(&server)
            .await;
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/v1beta/files/1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"name":"files/1","uri":"gemini://files/1","mimeType":"video/webm","state":"ACTIVE"})))
            .mount(&server)
            .await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/v1beta/models/gemini-3.1-flash-lite:generateContent"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"candidates":[{"content":{"role":"model","parts":[{"text":"safe summary"}]}}],"modelVersion":"gemini-3.1-flash-lite"})))
            .mount(&server)
            .await;
        Mock::given(matchers::method("DELETE"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        let outcome = resume_analyze_with_evidence(ResumeRequest {
            client: &client,
            cipher: &cipher,
            input: &input,
            checkpoint_path: &checkpoint_path,
            model: "gemini-3.1-flash-lite",
            evidence_path: &evidence_path,
            target: target(),
            poll_policy: PollPolicy::bounded(Duration::ZERO, Duration::ZERO),
        })
        .await
        .unwrap();
        assert_eq!(outcome.observed_server_offset, Some(0));
        assert_eq!(
            *requests.lock().unwrap(),
            vec![
                ("query".to_owned(), None, 0),
                ("upload".to_owned(), Some("0".to_owned()), 8 * 1024 * 1024),
                ("query".to_owned(), None, 0),
                ("upload, finalize".to_owned(), Some("8388608".to_owned()), 5,),
            ]
        );
    }

    #[tokio::test]
    async fn resume_continuation_failure_after_offset_mismatch_writes_failed_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let checkpoint_path = dir.path().join("checkpoint.json");
        let evidence_path = dir.path().join("evidence.json");
        let cipher =
            EnvelopeCipher::load_or_create(&MemorySecretStore::default(), "continuation").unwrap();
        let server = MockServer::start().await;
        let client = checkpoint(&server, &cipher, &checkpoint_path, 0, 4).await;
        Mock::given(matchers::header("x-goog-upload-command", "query"))
            .respond_with(
                ResponseTemplate::new(200).insert_header("x-goog-upload-size-received", "4"),
            )
            .mount(&server)
            .await;
        Mock::given(matchers::header("x-goog-upload-command", "upload"))
            .respond_with(ResponseTemplate::new(400))
            .mount(&server)
            .await;
        let error = resume_analyze_with_evidence(ResumeRequest {
            client: &client,
            cipher: &cipher,
            input: b"123456789012",
            checkpoint_path: &checkpoint_path,
            model: "gemini-3.1-flash-lite",
            evidence_path: &evidence_path,
            target: target(),
            poll_policy: PollPolicy::bounded(Duration::ZERO, Duration::ZERO),
        })
        .await
        .unwrap_err();
        assert_eq!(
            error.outcome.failure_category,
            Some(ResumeFailureCategory::Continuation)
        );
        let evidence = fs::read_to_string(&evidence_path).unwrap();
        assert!(evidence.contains("CONTINUATION_FAILED"));
        assert!(evidence.contains("\"offset_mismatch\": true"));
        assert_redacted(&evidence);
    }

    #[tokio::test]
    async fn remote_delete_failure_retains_checkpoint_and_writes_recovery_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let checkpoint_path = dir.path().join("checkpoint.json");
        let evidence_path = dir.path().join("evidence.json");
        let cipher =
            EnvelopeCipher::load_or_create(&MemorySecretStore::default(), "delete-fail").unwrap();
        let server = MockServer::start().await;
        let client = checkpoint(&server, &cipher, &checkpoint_path, 4, 4).await;
        Mock::given(matchers::header("x-goog-upload-command", "query"))
            .respond_with(
                ResponseTemplate::new(200).insert_header("x-goog-upload-size-received", "4"),
            )
            .mount(&server)
            .await;
        Mock::given(matchers::headers("x-goog-upload-command", vec!["upload", "finalize"]))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"file":{"name":"files/1","uri":"gemini://files/1","mimeType":"video/webm","state":"PROCESSING"}})))
            .mount(&server).await;
        Mock::given(matchers::method("GET")).and(matchers::path("/v1beta/files/1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"name":"files/1","uri":"gemini://files/1","mimeType":"video/webm","state":"ACTIVE"})))
            .mount(&server).await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/v1beta/models/gemini-3.1-flash-lite:generateContent"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"candidates":[{"content":{"role":"model","parts":[{"text":"safe summary"}]}}],"modelVersion":"gemini-3.1-flash-lite"})))
            .mount(&server).await;
        Mock::given(matchers::method("DELETE"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let error = resume_analyze_with_evidence(ResumeRequest {
            client: &client,
            cipher: &cipher,
            input: b"12345678",
            checkpoint_path: &checkpoint_path,
            model: "gemini-3.1-flash-lite",
            evidence_path: &evidence_path,
            target: target(),
            poll_policy: PollPolicy::bounded(Duration::ZERO, Duration::ZERO),
        })
        .await
        .unwrap_err();
        assert_eq!(
            error.outcome.failure_category,
            Some(ResumeFailureCategory::RemoteCleanup)
        );
        assert!(checkpoint_path.exists());
        let evidence = fs::read_to_string(&evidence_path).unwrap();
        assert!(evidence.contains("RETAINED_FOR_RECOVERY"));
        assert!(evidence.contains("REMOTE_CLEANUP_FAILED"));
        assert_redacted(&evidence);
    }

    #[tokio::test]
    async fn empty_generation_writes_redacted_metrics_and_returns_failure() {
        let dir = tempfile::tempdir().unwrap();
        let checkpoint_path = dir.path().join("checkpoint.json");
        let evidence_path = dir.path().join("evidence.json");
        let cipher =
            EnvelopeCipher::load_or_create(&MemorySecretStore::default(), "empty").unwrap();
        let server = MockServer::start().await;
        let client = checkpoint(&server, &cipher, &checkpoint_path, 4, 4).await;
        Mock::given(matchers::header("x-goog-upload-command", "query"))
            .respond_with(
                ResponseTemplate::new(200).insert_header("x-goog-upload-size-received", "4"),
            )
            .mount(&server)
            .await;
        Mock::given(matchers::headers("x-goog-upload-command", vec!["upload", "finalize"]))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"file":{"name":"files/1","uri":"gemini://files/1","mimeType":"video/webm","state":"PROCESSING"}})))
            .mount(&server).await;
        Mock::given(matchers::method("GET")).and(matchers::path("/v1beta/files/1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"name":"files/1","uri":"gemini://files/1","mimeType":"video/webm","state":"ACTIVE"})))
            .mount(&server).await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/v1beta/models/gemini-3.1-flash-lite:generateContent"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"candidates":[{"content":{"role":"model","parts":[{"text":""}]}}],"modelVersion":"gemini-3.1-flash-lite"})))
            .mount(&server).await;
        Mock::given(matchers::method("DELETE"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        let error = resume_analyze_with_evidence(ResumeRequest {
            client: &client,
            cipher: &cipher,
            input: b"12345678",
            checkpoint_path: &checkpoint_path,
            model: "gemini-3.1-flash-lite",
            evidence_path: &evidence_path,
            target: target(),
            poll_policy: PollPolicy::bounded(Duration::ZERO, Duration::ZERO),
        })
        .await
        .unwrap_err();
        assert_eq!(
            error.outcome.failure_category,
            Some(ResumeFailureCategory::EmptyAnalysis)
        );
        assert!(!checkpoint_path.exists());
        let evidence = fs::read_to_string(&evidence_path).unwrap();
        assert!(evidence.contains("\"analysis_nonempty\": false"));
        assert!(evidence.contains("response_bytes"));
        assert!(evidence.contains("http_status"));
        assert!(evidence.contains("analysis_latency_ms"));
        assert!(evidence.contains("\"checkpoint_cleanup_state\": \"DELETED\""));
        assert_redacted(&evidence);
    }
}
