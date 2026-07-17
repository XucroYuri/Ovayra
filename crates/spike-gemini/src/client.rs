use std::{fmt, time::Duration};

use reqwest::{
    Client, Response, StatusCode,
    header::{HeaderMap, RETRY_AFTER},
};
use spike_platform::{EncryptedRecord, EnvelopeCipher};
use thiserror::Error;
use tokio::time::{Instant, sleep};
use url::Url;

use crate::{
    FileState, RemoteFile, ResumedUpload, UploadSession,
    dto::{
        FileData, FileDto, GenerateContent, GeneratePart, GenerateRequest, GenerateResponse,
        StartFile, StartUploadRequest, UploadResponse,
    },
    session::CheckpointPayload,
};

const DEFAULT_API_BASE: &str = "https://generativelanguage.googleapis.com";
const CHECKPOINT_AAD: &[u8] = b"ovayra-upload-session-v1";
const FALLBACK_CHUNK_BYTES: u64 = 8 * 1024 * 1024;
const MAX_ATTEMPTS: u8 = 3;
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

/// Bounded retry behavior for transient Gemini HTTP responses.
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    max_attempts: u8,
    max_delay: Duration,
}

impl RetryPolicy {
    #[must_use]
    pub const fn bounded(max_attempts: u8, max_delay: Duration) -> Self {
        Self {
            max_attempts,
            max_delay,
        }
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::bounded(MAX_ATTEMPTS, Duration::from_secs(2))
    }
}

/// Bounded polling cadence for a remote Gemini file.
#[derive(Debug, Clone, Copy)]
pub struct PollPolicy {
    interval: Duration,
    timeout: Duration,
}

impl PollPolicy {
    #[must_use]
    pub const fn bounded(interval: Duration, timeout: Duration) -> Self {
        Self { interval, timeout }
    }
}

/// Redacted generation measurements; analysis text is intentionally not exposed.
pub struct GenerationResult {
    analysis_nonempty: bool,
    response_bytes: usize,
    status: u16,
    model: String,
    latency: Duration,
}

impl fmt::Debug for GenerationResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("GenerationResult([REDACTED])")
    }
}

impl GenerationResult {
    #[must_use]
    pub const fn analysis_nonempty(&self) -> bool {
        self.analysis_nonempty
    }
    #[must_use]
    pub const fn response_bytes(&self) -> usize {
        self.response_bytes
    }
    #[must_use]
    pub const fn status(&self) -> u16 {
        self.status
    }
    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }
    #[must_use]
    pub const fn latency(&self) -> Duration {
        self.latency
    }
}

pub struct GeminiClient {
    client: Client,
    api_key: String,
    api_base: Url,
    upload_base: Url,
    retry_policy: RetryPolicy,
}

impl fmt::Debug for GeminiClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("GeminiClient([REDACTED])")
    }
}

#[derive(Debug, Error)]
pub enum GeminiError {
    #[error("Gemini endpoint configuration is invalid")]
    InvalidEndpoint,
    #[error("Gemini upload protocol response was malformed")]
    Protocol,
    #[error("Gemini upload chunk is not aligned to the server granularity")]
    ChunkMisaligned,
    #[error("Gemini request returned HTTP status {0}")]
    HttpStatus(u16),
    #[error("Gemini transport request failed")]
    Transport,
    #[error("Gemini request response could not be decoded")]
    Decode,
    #[error("Gemini response exceeded the bounded size limit")]
    ResponseTooLarge,
    #[error("Gemini polling timed out")]
    PollTimeout,
    #[error("Gemini remote file entered FAILED state")]
    RemoteFailed,
    #[error("Gemini response did not contain analysis text")]
    EmptyAnalysis,
    #[error("encrypted upload checkpoint is invalid")]
    InvalidCheckpoint,
    #[error("encrypted upload checkpoint could not be opened")]
    Checkpoint,
}

#[allow(clippy::missing_errors_doc)]
impl GeminiClient {
    pub fn new(api_key: impl Into<String>) -> Result<Self, GeminiError> {
        Self::for_endpoints(api_key, DEFAULT_API_BASE, DEFAULT_API_BASE)
    }

    pub fn for_endpoints(
        api_key: impl Into<String>,
        api_base: &str,
        upload_base: &str,
    ) -> Result<Self, GeminiError> {
        Self::for_endpoints_with_retry_policy(
            api_key,
            api_base,
            upload_base,
            RetryPolicy::default(),
        )
    }

    pub fn for_endpoints_with_retry_policy(
        api_key: impl Into<String>,
        api_base: &str,
        upload_base: &str,
        retry_policy: RetryPolicy,
    ) -> Result<Self, GeminiError> {
        let api_base = Url::parse(api_base).map_err(|_| GeminiError::InvalidEndpoint)?;
        let upload_base = Url::parse(upload_base).map_err(|_| GeminiError::InvalidEndpoint)?;
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .pool_max_idle_per_host(0)
            .build()
            .map_err(|_| GeminiError::Transport)?;
        Ok(Self {
            client,
            api_key: api_key.into(),
            api_base,
            upload_base,
            retry_policy,
        })
    }

    pub async fn start_upload(
        &self,
        display_name: &str,
        mime_type: &str,
        content_length: u64,
    ) -> Result<UploadSession, GeminiError> {
        let url = Self::url(&self.upload_base, "/upload/v1beta/files")?;
        let response = self
            .retry(|| {
                self.client
                    .post(url.clone())
                    .header("x-goog-api-key", &self.api_key)
                    .header("x-goog-upload-protocol", "resumable")
                    .header("x-goog-upload-command", "start")
                    .header("x-goog-upload-header-content-length", content_length)
                    .header("x-goog-upload-header-content-type", mime_type)
                    .json(&StartUploadRequest {
                        file: StartFile { display_name },
                    })
                    .send()
            })
            .await?;
        let upload_url = response
            .headers()
            .get("x-goog-upload-url")
            .and_then(|value| value.to_str().ok())
            .ok_or(GeminiError::Protocol)?;
        let url = Url::parse(upload_url).map_err(|_| GeminiError::Protocol)?;
        let chunk_granularity = response
            .headers()
            .get("x-goog-upload-chunk-granularity")
            .and_then(|value| value.to_str().ok())
            .map(str::parse)
            .transpose()
            .map_err(|_| GeminiError::Protocol)?;
        if chunk_granularity == Some(0) {
            return Err(GeminiError::Protocol);
        }
        Ok(UploadSession {
            url,
            chunk_granularity,
        })
    }

    pub async fn upload_chunk(
        &self,
        session: &UploadSession,
        offset: u64,
        chunk: &[u8],
    ) -> Result<(), GeminiError> {
        if session.chunk_granularity.is_some_and(|granularity| {
            !u64::try_from(chunk.len()).is_ok_and(|length| length.is_multiple_of(granularity))
        }) {
            return Err(GeminiError::ChunkMisaligned);
        }
        let expected = offset
            .checked_add(u64::try_from(chunk.len()).map_err(|_| GeminiError::Protocol)?)
            .ok_or(GeminiError::Protocol)?;
        for attempt in 0..self.retry_policy.max_attempts {
            match self.upload_once(session, offset, chunk, "upload").await {
                Ok(response) if response.status().is_success() => return Ok(()),
                Ok(response)
                    if transient(response.status())
                        && attempt + 1 < self.retry_policy.max_attempts =>
                {
                    if response.status().is_server_error() {
                        let observed = self.query_offset(session).await?;
                        if observed == expected {
                            return Ok(());
                        }
                        if observed != offset {
                            return Err(GeminiError::Protocol);
                        }
                    }
                    sleep(retry_delay(
                        response.headers(),
                        attempt,
                        self.retry_policy.max_delay,
                    ))
                    .await;
                }
                Ok(response) => return Err(GeminiError::HttpStatus(response.status().as_u16())),
                Err(GeminiError::Transport) => {
                    let observed = self.query_offset(session).await?;
                    if observed == expected {
                        return Ok(());
                    }
                    if observed != offset {
                        return Err(GeminiError::Protocol);
                    }
                }
                Err(error) => return Err(error),
            }
        }
        Err(GeminiError::Transport)
    }

    pub async fn finalize_chunk(
        &self,
        session: &UploadSession,
        offset: u64,
        chunk: &[u8],
    ) -> Result<RemoteFile, GeminiError> {
        let response = self
            .upload(session, offset, chunk, "upload, finalize")
            .await?;
        let dto: UploadResponse = response.json().await.map_err(|_| GeminiError::Decode)?;
        Ok(remote_file(dto.file))
    }

    pub async fn query_offset(&self, session: &UploadSession) -> Result<u64, GeminiError> {
        let response = self
            .retry(|| {
                self.client
                    .post(session.url.clone())
                    .header("x-goog-api-key", &self.api_key)
                    .header("x-goog-upload-command", "query")
                    .send()
            })
            .await?;
        response
            .headers()
            .get("x-goog-upload-size-received")
            .and_then(|value| value.to_str().ok())
            .ok_or(GeminiError::Protocol)?
            .parse()
            .map_err(|_| GeminiError::Protocol)
    }

    pub async fn poll_until_ready(
        &self,
        name: &str,
        interval: Duration,
        timeout: Duration,
    ) -> Result<RemoteFile, GeminiError> {
        self.poll_until_ready_with_policy(name, PollPolicy::bounded(interval, timeout))
            .await
    }

    pub async fn poll_until_ready_with_policy(
        &self,
        name: &str,
        policy: PollPolicy,
    ) -> Result<RemoteFile, GeminiError> {
        let deadline = Instant::now() + policy.timeout;
        loop {
            let remote = self.get_file(name).await?;
            match remote.state {
                FileState::Active => return Ok(remote),
                FileState::Failed => return Err(GeminiError::RemoteFailed),
                FileState::Processing | FileState::Unspecified => {}
            }
            if Instant::now() >= deadline {
                return Err(GeminiError::PollTimeout);
            }
            sleep(
                policy
                    .interval
                    .min(deadline.saturating_duration_since(Instant::now())),
            )
            .await;
        }
    }

    pub async fn generate_content(
        &self,
        remote: &RemoteFile,
        model: &str,
    ) -> Result<GenerationResult, GeminiError> {
        let path = format!("/v1beta/models/{model}:generateContent");
        let url = Self::url(&self.api_base, &path)?;
        let request = GenerateRequest {
            contents: [GenerateContent {
                role: "user",
                parts: [
                    GeneratePart::FileData {
                        file_data: FileData {
                            file_uri: &remote.uri,
                            mime_type: &remote.mime_type,
                        },
                    },
                    GeneratePart::Text {
                        text: "Describe the synthetic test video in one sentence.",
                    },
                ],
            }],
        };
        let started = Instant::now();
        let response = self
            .retry(|| {
                self.client
                    .post(url.clone())
                    .header("x-goog-api-key", &self.api_key)
                    .json(&request)
                    .send()
            })
            .await?;
        let status = response.status().as_u16();
        let response_bytes = response.bytes().await.map_err(|_| GeminiError::Transport)?;
        if response_bytes.len() > MAX_RESPONSE_BYTES {
            return Err(GeminiError::ResponseTooLarge);
        }
        let response: GenerateResponse =
            serde_json::from_slice(&response_bytes).map_err(|_| GeminiError::Decode)?;
        let analysis_nonempty = response
            .candidates
            .into_iter()
            .flat_map(|candidate| candidate.content.parts)
            .filter_map(|part| part.text)
            .any(|text| !text.trim().is_empty());
        Ok(GenerationResult {
            analysis_nonempty,
            response_bytes: response_bytes.len(),
            status,
            model: model.to_owned(),
            latency: started.elapsed(),
        })
    }

    pub async fn delete_file(&self, name: &str) -> Result<(), GeminiError> {
        let url = Self::url(&self.api_base, &format!("/v1beta/{name}"))?;
        self.retry(|| {
            self.client
                .delete(url.clone())
                .header("x-goog-api-key", &self.api_key)
                .send()
        })
        .await?;
        Ok(())
    }

    pub fn checkpoint(
        &self,
        cipher: &EnvelopeCipher,
        session: &UploadSession,
        staged_offset: u64,
    ) -> Result<EncryptedRecord, GeminiError> {
        let payload = CheckpointPayload {
            url: session.url.to_string(),
            chunk_granularity: session.chunk_granularity,
            staged_offset,
        };
        let json = serde_json::to_vec(&payload).map_err(|_| GeminiError::InvalidCheckpoint)?;
        cipher
            .seal(&json, CHECKPOINT_AAD)
            .map_err(|_| GeminiError::Checkpoint)
    }

    pub fn resume_checkpoint(
        &self,
        cipher: &EnvelopeCipher,
        record: &EncryptedRecord,
    ) -> Result<ResumedUpload, GeminiError> {
        let plaintext = cipher
            .open(record, CHECKPOINT_AAD)
            .map_err(|_| GeminiError::Checkpoint)?;
        let payload: CheckpointPayload =
            serde_json::from_slice(&plaintext).map_err(|_| GeminiError::InvalidCheckpoint)?;
        let url = Url::parse(&payload.url).map_err(|_| GeminiError::InvalidCheckpoint)?;
        Ok(ResumedUpload {
            session: UploadSession {
                url,
                chunk_granularity: payload.chunk_granularity,
            },
            staged_offset: payload.staged_offset,
        })
    }

    #[must_use]
    pub fn chunk_size(&self, session: &UploadSession) -> u64 {
        session.chunk_granularity.unwrap_or(FALLBACK_CHUNK_BYTES)
    }

    async fn upload(
        &self,
        session: &UploadSession,
        offset: u64,
        chunk: &[u8],
        command: &'static str,
    ) -> Result<Response, GeminiError> {
        self.retry(|| {
            self.client
                .post(session.url.clone())
                .header("x-goog-api-key", &self.api_key)
                .header("x-goog-upload-command", command)
                .header("x-goog-upload-offset", offset)
                .body(chunk.to_vec())
                .send()
        })
        .await
    }

    async fn upload_once(
        &self,
        session: &UploadSession,
        offset: u64,
        chunk: &[u8],
        command: &'static str,
    ) -> Result<Response, GeminiError> {
        self.client
            .post(session.url.clone())
            .header("x-goog-api-key", &self.api_key)
            .header("x-goog-upload-command", command)
            .header("x-goog-upload-offset", offset)
            .body(chunk.to_vec())
            .send()
            .await
            .map_err(|_| GeminiError::Transport)
    }

    async fn get_file(&self, name: &str) -> Result<RemoteFile, GeminiError> {
        let url = Self::url(&self.api_base, &format!("/v1beta/{name}"))?;
        let response = self
            .retry(|| {
                self.client
                    .get(url.clone())
                    .header("x-goog-api-key", &self.api_key)
                    .send()
            })
            .await?;
        let dto: FileDto = response.json().await.map_err(|_| GeminiError::Decode)?;
        Ok(remote_file(dto))
    }

    fn url(base: &Url, path: &str) -> Result<Url, GeminiError> {
        base.join(path).map_err(|_| GeminiError::InvalidEndpoint)
    }

    async fn retry<F, Fut>(&self, mut request: F) -> Result<Response, GeminiError>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<Response, reqwest::Error>>,
    {
        for attempt in 0..self.retry_policy.max_attempts {
            let response = request().await.map_err(|_| GeminiError::Transport)?;
            if response.status().is_success() {
                return Ok(response);
            }
            if !transient(response.status()) || attempt + 1 == self.retry_policy.max_attempts {
                return Err(GeminiError::HttpStatus(response.status().as_u16()));
            }
            sleep(retry_delay(
                response.headers(),
                attempt,
                self.retry_policy.max_delay,
            ))
            .await;
        }
        Err(GeminiError::Transport)
    }
}

fn remote_file(file: FileDto) -> RemoteFile {
    let state = match file.state.as_deref() {
        Some("PROCESSING") => FileState::Processing,
        Some("ACTIVE") => FileState::Active,
        Some("FAILED") => FileState::Failed,
        _ => FileState::Unspecified,
    };
    RemoteFile {
        name: file.name,
        uri: file.uri,
        mime_type: file.mime_type,
        state,
    }
}

fn transient(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

fn retry_delay(headers: &HeaderMap, attempt: u8, maximum: Duration) -> Duration {
    headers
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map_or_else(
            || Duration::from_millis(100 * 2_u64.pow(u32::from(attempt))),
            Duration::from_secs,
        )
        .min(maximum)
}
